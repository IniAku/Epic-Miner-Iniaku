use std::string;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use keccak_hash::keccak_256;

use core::config::{MinerConfig, GpuConfig};
use core::errors::MinerError;
use core::miner::Miner;
use core::types::AlgorithmParams;
use core::util;
use core::{ControlMessage, JobSharedData, JobSharedDataType, Solution, Stats};

use bigint::uint::U256;
use util::LOGGER;

use progpow::hardware::PpGPU;
use progpow::hardware::PpCPU;
use progpow::types::PpCompute;

const ALGORITHM_NAME: &str = "progpow";
const GLOBAL_WORK_SIZE: u64 = 2048;
const LOCAL_WORK_SIZE: u64 = 256;
const WORK_PER_CALL: u64 = GLOBAL_WORK_SIZE * LOCAL_WORK_SIZE;
const PROGPOW_PERIOD: u64 = 50;
/// GPU solutions failing the CPU check (with none passing) before the kernel is rebuilt safer
const BAD_SOLUTIONS_BEFORE_SAFE_MODE: u32 = 3;
const MAX_SAFE_MODE: i32 = 2;

/// Watches GPU solutions per ProgPoW period. Some drivers build a broken kernel for some
/// periods: the GPU hashes at full speed but every solution fails the CPU check, so the
/// rig finds nothing for hours. Then this period's kernel is rebuilt in a safe mode.
struct PeriodGuard {
	period: u64,
	bad: u32,
	good: u32,
	level: i32,
	gave_up: bool,
}

impl PeriodGuard {
	fn new() -> Self {
		PeriodGuard { period: u64::MAX, bad: 0, good: 0, level: 0, gave_up: false }
	}

	/// Returns the safe mode level to switch to, if any.
	fn record(&mut self, instance: usize, height: u64, ok: bool) -> Option<i32> {
		let period = height / PROGPOW_PERIOD;
		if period != self.period {
			// The miner builds the next period's kernel in normal mode again.
			*self = PeriodGuard::new();
			self.period = period;
		}
		if ok {
			self.good += 1;
			if self.level > 0 && self.good == 1 {
				warn!(LOGGER, "ProgPoW GPU {}: safe mode {} works for period {}, solutions pass the check again", instance, self.level, period);
			}
			return None;
		}
		self.bad += 1;
		if self.good > 0 || self.bad < BAD_SOLUTIONS_BEFORE_SAFE_MODE {
			return None;
		}
		if self.level >= MAX_SAFE_MODE {
			if !self.gave_up {
				self.gave_up = true;
				error!(LOGGER, "ProgPoW GPU {}: solutions still fail the check in safe mode for period {} (heights {}-{}); this GPU finds no shares until the next period", instance, period, period * PROGPOW_PERIOD, period * PROGPOW_PERIOD + PROGPOW_PERIOD - 1);
			}
			return None;
		}
		self.level += 1;
		self.bad = 0;
		warn!(LOGGER, "ProgPoW GPU {}: {} solutions in a row failed the CPU check for period {}: rebuilding the kernel in safe mode {}", instance, BAD_SOLUTIONS_BEFORE_SAFE_MODE, period, self.level);
		Some(self.level)
	}
}

fn timestamp() -> u64 {
	let start = SystemTime::now();
	let since_the_epoch = start
		.duration_since(UNIX_EPOCH)
		.expect("Time went backwards");
	since_the_epoch.as_millis() as u64
}

pub struct PpMiner {
	/// Data shared across threads
	pub shared_data: Arc<RwLock<JobSharedData>>,

	pub gpus: Vec<GpuConfig>,

	/// Job control tx
	control_txs: Vec<mpsc::Sender<ControlMessage>>,

	/// solver loop tx
	solver_loop_txs: Vec<mpsc::Sender<ControlMessage>>,

	/// Solver has stopped and cleanly shutdown
	solver_stopped_rxs: Vec<mpsc::Receiver<ControlMessage>>,
}

unsafe impl Send for PpMiner {}
unsafe impl Sync for PpMiner {}

impl PpMiner {
	fn solver_thread(
		instance: usize,
		config: GpuConfig,
		shared_data: JobSharedDataType,
		control_rx: mpsc::Receiver<ControlMessage>,
		solver_loop_rx: mpsc::Receiver<ControlMessage>,
		solver_stopped_tx: mpsc::Sender<ControlMessage>,
	) {
		{
			let mut s = shared_data.write().unwrap();
			s.stats[instance].set_plugin_name(ALGORITHM_NAME);
		}

		let mut last_solution_time = 0;
		let mut iter_count = 0;
		let mut paused = true;

		let mut cpu = PpCPU::new();
		let mut gpu = PpGPU::new(config.device, config.driver);
		gpu.init();
		let mut guard = PeriodGuard::new();
		// Testing only: treat this many GPU solutions per period as failed (exercises safe mode).
		let force_bad: u32 = std::env::var("INIAKU_PP_FORCE_BAD").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
		let mut forced: (u64, u32) = (u64::MAX, 0);

		loop {
			if let Some(message) = solver_loop_rx.try_iter().next() {
				//debug!(LOGGER, "solver_thread - solver_loop_rx got msg: {:?}", message);
				match message {
					ControlMessage::Stop => break,
					ControlMessage::Pause => paused = true,
					ControlMessage::Resume => paused = false,
					_ => {}
				}
			}

			if paused {
				thread::sleep(time::Duration::from_micros(100));
				continue;
			}
			{
				let mut s = shared_data.write().unwrap();
				s.stats[instance].set_plugin_name(ALGORITHM_NAME);
			}

			let header_pre =
				util::from_hex_string({ shared_data.read().unwrap().pre_nonce.clone() }.as_str());

			let height = { shared_data.read().unwrap().height.clone() };
			let job_id = { shared_data.read().unwrap().job_id.clone() };
			let target_difficulty = { shared_data.read().unwrap().difficulty.clone() };

			let boundary = U256::max_value() / U256::from(if target_difficulty > 0 { target_difficulty } else { 1 });

			let target = (boundary >> 192).low_u64();
			let mut header = [0u8; 32];

			keccak_256(&header_pre, &mut header);

			let start = timestamp();
			gpu.compute_with_startnonce(header, height, (height / 30000) as i32, target, util::get_random_nonce());
			let end = timestamp();

			if !gpu.ready() {
				// This GPU could not build its DAG: report no speed, the others keep mining.
				let mut stats = Stats {
					last_start_time: start,
					last_end_time: end,
					last_solution_time: last_solution_time,
					iterations: iter_count as u32,
					hashes_per_sec: 0,
					has_errored: true,
					..Default::default()
				};
				stats.set_plugin_name(ALGORITHM_NAME);
				shared_data.write().unwrap().stats[instance] = stats;
				continue;
			}

			iter_count += WORK_PER_CALL;
			let still_valid = { height == shared_data.read().unwrap().height };
			if still_valid {
				let mut s = shared_data.write().unwrap();
				let solutions = gpu.get_solutions();

				if let Some(solution) = solutions {
					last_solution_time = timestamp();
					let (nonce, mix) = solution;

					let (v, _) = cpu.verify(&header, height, nonce).unwrap();
					let digest: [u8; 32] = unsafe { ::std::mem::transmute(v) };
					let h256_digest: U256 = digest.into();

					let mut ok = h256_digest <= boundary;
					if force_bad > 0 {
						let period = height / PROGPOW_PERIOD;
						if forced.0 != period {
							forced = (period, 0);
						}
						if ok && forced.1 < force_bad {
							forced.1 += 1;
							ok = false;
						}
					}
					if let Some(level) = guard.record(instance, height, ok) {
						gpu.set_safe_mode(level);
					}

					if ok {
						s.solutions.push(Solution::new(
							job_id as u64,
							nonce,
							AlgorithmParams::ProgPow(mix),
						));
					}
				}

				let delta = end - start;
				let hps = if delta > 0 { (WORK_PER_CALL * 1000) / (end-start) } else { WORK_PER_CALL };

				let mut stats = Stats {
					last_start_time: start,
					last_end_time: end,
					last_solution_time: last_solution_time,
					iterations: iter_count as u32,
					hashes_per_sec: hps,
					..Default::default()
				};
 
				stats.set_plugin_name(ALGORITHM_NAME);
				s.stats[instance] = stats;
			}
		}

		let _ = solver_stopped_tx.send(ControlMessage::SolverStopped(instance));
	}
}

impl Miner for PpMiner {
	fn new(configs: &MinerConfig) -> PpMiner {
		let count = configs.gpu_config.len();
		PpMiner {
			shared_data: Arc::new(RwLock::new(JobSharedData::new(count))),
			gpus: configs.gpu_config.clone(),
			control_txs: vec![],
			solver_loop_txs: vec![],
			solver_stopped_rxs: vec![],
		}
	}

	fn start_solvers(&mut self) -> Result<(), MinerError> {
		for i in 0..self.gpus.len() {
			let config = self.gpus[i].clone();
			let shared_data = self.shared_data.clone();
			let (control_tx, control_rx) = mpsc::channel::<ControlMessage>();
			let (solver_tx, solver_rx) = mpsc::channel::<ControlMessage>();
			let (solver_stopped_tx, solver_stopped_rx) = mpsc::channel::<ControlMessage>();

			self.control_txs.push(control_tx);
			self.solver_loop_txs.push(solver_tx);
			self.solver_stopped_rxs.push(solver_stopped_rx);

			thread::spawn(move || {
				let _ = PpMiner::solver_thread(
					i,
					config,
					shared_data,
					control_rx,
					solver_rx,
					solver_stopped_tx,
				);
			});
		}

		Ok(())
	}

	fn get_solutions(&self) -> Option<Vec<Solution>> {
		let mut s = self.shared_data.write().unwrap();

		let solutions = s.solutions.clone();
		s.solutions.clear();

		Some(solutions)
	}

	fn get_stats(&self) -> Result<Vec<Stats>, MinerError> {
		//println!("{:?}", self.shared_data.read().unwrap().stats[0].get_plugin_name());
		Ok(self.shared_data.read().unwrap().stats.clone())
	}

	fn notify(
		&mut self,
		job_id: u32,      // Job id
		height: u64,      // Job height
		pre_nonce: &str,  // Pre-nonce portion of header
		post_nonce: &str, // Post-nonce portion of header
		difficulty: u64,  /* The target difficulty, only sols greater than this difficulty will
		                   * be returned. */
	) -> Result<(), MinerError> {
		let mut sd = self.shared_data.write().unwrap();
		if height != sd.height {
			// stop/pause any existing jobs if job is for a new
			// height
			self.pause_solvers();
		}
		sd.job_id = job_id;
		sd.height = height;
		sd.pre_nonce = pre_nonce.to_owned();
		sd.post_nonce = post_nonce.to_owned();
		sd.difficulty = difficulty;
		// notify() only gets jobs of our own turn: always resume. Solvers paused by StopJob
		// (other algorithm's turn, or a lost connection) used to stay paused when the next job
		// had the same height, e.g. right after a reconnect, until the next block of our turn.
		self.resume_solvers();
		Ok(())
	}

	/// #Description
	///
	/// Stops the current job, and signals for the loaded plugin to stop
	/// processing and perform any cleanup it needs to do.
	///
	/// #Returns
	///
	/// Nothing
	fn stop_solvers(&self) {
		for t in self.control_txs.iter() {
			let _ = t.send(ControlMessage::Stop);
		}
		for t in self.solver_loop_txs.iter() {
			let _ = t.send(ControlMessage::Stop);
		}
		//debug!(LOGGER, "Stop message sent");
	}

	/// Tells current solvers to stop and wait
	fn pause_solvers(&self) {
		for t in self.control_txs.iter() {
			let _ = t.send(ControlMessage::Pause);
		}
		for t in self.solver_loop_txs.iter() {
			let _ = t.send(ControlMessage::Pause);
		}
		//debug!(LOGGER, "Pause message sent");
	}

	/// Tells current solvers to stop and wait
	fn resume_solvers(&self) {
		for t in self.control_txs.iter() {
			let _ = t.send(ControlMessage::Resume);
		}
		for t in self.solver_loop_txs.iter() {
			let _ = t.send(ControlMessage::Resume);
		}
		//debug!(LOGGER, "Resume message sent");
	}

	/// block until solvers have all exited
	fn wait_for_solver_shutdown(&self) {
		for r in self.solver_stopped_rxs.iter() {
			while let Some(message) = r.iter().next() {
				match message {
					ControlMessage::SolverStopped(i) => {
						//debug!(LOGGER, "Solver stopped: {}", i);
						break;
					}
					_ => {}
				}
			}
		}
	}
}
