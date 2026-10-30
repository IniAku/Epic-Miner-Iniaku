extern crate blake2_rfc;
extern crate byteorder;
extern crate epic_miner_plugin as plugin;
extern crate hashbrown;
extern crate libc;
extern crate ocl;

use blake2_rfc::blake2b::blake2b;
use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt, WriteBytesExt};
use libc::*;
use plugin::*;
use std::collections::VecDeque;
use std::io::Cursor;
use std::io::Error;
use std::mem;
use std::time::{Duration, SystemTime};

pub use self::batch::BatchTrimmer;
pub use self::finder::Graph;
pub use self::trimmer::Trimmer;

mod batch;
mod finder;
mod trimmer;

#[repr(C)]
struct Solver {
	edge_bits: u8,
	/// C31 (dan C19 tanpa batch): trimmer lama, satu graf per panggilan.
	trimmer: Option<Trimmer>,
	/// C19: banyak graf per panggilan (lihat batch.rs).
	batch: Option<BatchTrimmer>,
	/// Solusi batch yg belum dikembalikan (SolverSolutions muat MAX_SOLS per panggilan):
	/// (nonce, proof), berlaku hanya utk header yg sama (`queue_key` = header tanpa nonce).
	queue: VecDeque<(u64, Vec<u64>)>,
	queue_key: Vec<u8>,
	last_graph_ns: u64,
	graph: Option<Graph>,
	mutate_nonce: bool,
}

/// Graf per batch (env OCL_BATCH, default 256) & ronde trimming C19 (env OCL_TRIMS, default 96).
fn env_u32(name: &str, default: u32, min: u32, max: u32) -> u32 {
	std::env::var(name)
		.ok()
		.and_then(|v| v.trim().parse::<u32>().ok())
		.map(|v| v.max(min).min(max))
		.unwrap_or(default)
}

/// OCL_BATCH di-set user → dipakai; selain itu None = otomatis per GPU (batch::auto_batch).
fn ocl_batch() -> Option<usize> {
	std::env::var("OCL_BATCH").ok().map(|_| env_u32("OCL_BATCH", 256, 1, 4096) as usize)
}

fn ocl_trims() -> u32 {
	env_u32("OCL_TRIMS", 96, 8, 512)
}

/// Dibaca miner (simbol opsional) saat plugin dimuat: > 1 = run_solver mode batch. Jumlah
/// graf sebenarnya ditentukan plugin per GPU (OCL_BATCH atau otomatis), bukan nilai ini.
#[no_mangle]
pub extern "C" fn batch_size() -> uint32_t {
	ocl_batch().unwrap_or(256) as uint32_t
}

#[no_mangle]
pub unsafe extern "C" fn create_solver_ctx(params: *mut SolverParams) -> *mut SolverCtx {
	let platform = match (*params).platform {
		1 => Some("AMD"),
		2 => Some("NVIDIA"),
		_ => None,
	};
	let device_id = Some((*params).device as usize);
	// Epic menerima Cuckatoo 19 dan 31; C19 bernilai jauh lebih tinggi per solusi (graph_weight),
	// jadi 19 = default. Dulu dipaksa >= 31 sehingga GPU OpenCL tidak pernah menambang C19.
	let mut edge_bits = (*params).edge_bits as u8;
	if edge_bits != 19 && (edge_bits < 31 || edge_bits > 64) {
		edge_bits = 19;
	}
	let (trimmer, batch) = if edge_bits == 19 {
		let bt = BatchTrimmer::build(platform, device_id, edge_bits, ocl_batch(), ocl_trims())
			.expect("can't build batch trimmer");
		eprintln!(
			"ocl_cuckatoo: {} - C19 batch {} graphs, {} trims{}",
			bt.device_name,
			bt.batch,
			bt.trims,
			if ocl_batch().is_some() { " (OCL_BATCH)" } else { " (auto)" }
		);
		(None, Some(bt))
	} else {
		let t = Trimmer::build(platform, device_id, edge_bits).expect("can't build trimmer");
		(Some(t), None)
	};
	let solver = Solver {
		edge_bits,
		trimmer,
		batch,
		queue: VecDeque::new(),
		queue_key: vec![],
		last_graph_ns: 0,
		graph: None,
		mutate_nonce: (*params).mutate_nonce,
	};
	let solver_box = Box::new(solver);
	let solver_ref = Box::leak(solver_box);
	mem::transmute::<&mut Solver, *mut SolverCtx>(solver_ref)
}

#[no_mangle]
pub unsafe extern "C" fn destroy_solver_ctx(solver_ctx_ptr: *mut SolverCtx) {
	// create box to clear memory
	let solver_ptr = mem::transmute::<*mut SolverCtx, *mut Solver>(solver_ctx_ptr);
	let _solver_box = Box::from_raw(solver_ptr);
}

#[no_mangle]
pub unsafe extern "C" fn stop_solver(_solver_ctx_ptr: *mut SolverCtx) {}

#[no_mangle]
pub unsafe extern "C" fn fill_default_params(params: *mut SolverParams) {
	(*params).device = 0;
	(*params).platform = 0;
	(*params).edge_bits = 19;
}

#[no_mangle]
pub unsafe extern "C" fn run_solver(
	ctx: *mut SolverCtx,
	header_ptr: *const c_uchar,
	header_length: uint32_t,
	nonce: uint64_t,
	range: uint32_t,
	solutions: *mut SolverSolutions,
	stats: *mut SolverStats,
) -> uint32_t {
	let start = SystemTime::now();
	let solver_ptr = mem::transmute::<*mut SolverCtx, *mut Solver>(ctx);
	let solver = &mut *solver_ptr;
	let header = std::slice::from_raw_parts(header_ptr, header_length as usize).to_vec();

	let (found, device_id, device_name, graph_ns) = if solver.batch.is_some() {
		// Batch: `nonce` = offset byte nonce (u64 BE) di header, `range` = jumlah graf.
		if range < 2 || nonce as usize + 8 > header.len() {
			return 2;
		}
		let off = nonce as usize;
		let mut key = header.clone();
		key[off..off + 8].copy_from_slice(&[0u8; 8]);
		if solver.queue.is_empty() || solver.queue_key != key {
			solver.queue.clear();
			solver.queue_key = key;
			let bt = solver.batch.as_ref().unwrap();
			let base = BigEndian::read_u64(&header[off..off + 8]);
			let mut h = header.clone();
			let mut keys = Vec::with_capacity(bt.batch);
			for i in 0..bt.batch {
				BigEndian::write_u64(&mut h[off..off + 8], base.wrapping_add(i as u64));
				match create_siphash_keys(&h) {
					Ok(k) => keys.push(k),
					Err(_) => return 2,
				}
			}
			let res = match bt.run(&keys) {
				Ok(r) => r,
				Err(_) => return 2,
			};
			// Cari cycle (CPU, ~0.2 ms/graf) paralel di beberapa core supaya tidak menahan GPU.
			let nthreads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8).max(1);
			let chunk = (bt.batch + nthreads - 1) / nthreads;
			let res = &res;
			let found: Vec<(u64, Vec<u64>)> = std::thread::scope(|sc| {
				let handles: Vec<_> = (0..bt.batch)
					.step_by(chunk)
					.map(|from| {
						let to = std::cmp::min(from + chunk, bt.batch);
						sc.spawn(move || {
							let mut out = vec![];
							for g in from..to {
								let mut a = res[g * batch::AUX_STRIDE..(g + 1) * batch::AUX_STRIDE].to_vec();
								a[1] = std::cmp::min(a[1] as usize, batch::MAX_EDGES) as u32;
								if a[1] == 0 {
									continue;
								}
								if let Ok(sols) = Graph::search(&a) {
									for sol in sols {
										if sol.nonces.len() == PROOFSIZE {
											let mut p = sol.nonces.clone();
											p.sort();
											out.push((base.wrapping_add(g as u64), p));
										}
									}
								}
							}
							out
						})
					})
					.collect();
				handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
			});
			solver.queue.extend(found);
			solver.last_graph_ns =
				duration_to_u64(SystemTime::now().duration_since(start).unwrap()) / bt.batch as u64;
		}
		let mut found = vec![];
		while found.len() < MAX_SOLS {
			match solver.queue.pop_front() {
				Some(s) => found.push(s),
				None => break,
			}
		}
		let bt = solver.batch.as_ref().unwrap();
		(found, bt.device_id, bt.device_name.clone(), solver.last_graph_ns)
	} else {
		let t = match solver.trimmer.as_ref() {
			Some(t) => t,
			None => return 2,
		};
		let k = match set_header_nonce(&header, Some(nonce as u32), solver.mutate_nonce) {
			Err(_e) => return 2,
			Ok(v) => v,
		};
		let res = t.run(&k).unwrap();
		let sols = Graph::search(&res).unwrap();
		// nonce 0 = miner memakai nonce header-nya sendiri.
		let found = sols.into_iter().take(MAX_SOLS).map(|s| (nonce, s.nonces)).collect();
		let ns = duration_to_u64(SystemTime::now().duration_since(start).unwrap());
		(found, t.device_id, t.device_name.clone(), ns)
	};

	let end = SystemTime::now();
	(*solutions).edge_bits = solver.edge_bits as u32;
	(*solutions).num_sols = found.len() as u32;
	for (i, (n, proof)) in found.iter().enumerate() {
		(*solutions).sols[i].nonce = *n;
		(*solutions).sols[i].proof.copy_from_slice(&proof[..PROOFSIZE]);
	}
	let stats_ref = &mut *stats;
	stats_ref.edge_bits = solver.edge_bits as u32;
	stats_ref.device_id = device_id as u32;
	let name_bytes = device_name.as_bytes();
	let n = std::cmp::min(stats_ref.device_name.len(), name_bytes.len());
	stats_ref.device_name[..n].copy_from_slice(&name_bytes[..n]);
	// Per graf (batch: waktu batch / jumlah graf) — miner menghitung gps = 1 / waktu ini.
	stats_ref.last_solution_time = graph_ns;
	stats_ref.last_start_time = duration_to_u64(start.duration_since(SystemTime::UNIX_EPOCH).unwrap());
	stats_ref.last_end_time = duration_to_u64(end.duration_since(SystemTime::UNIX_EPOCH).unwrap());
	0
}

fn duration_to_u64(elapsed: Duration) -> u64 {
	elapsed.as_secs() * 1_000_000_000 + elapsed.subsec_nanos() as u64
}

pub fn set_header_nonce(
	header: &[u8],
	nonce: Option<u32>,
	mutate_nonce: bool,
) -> Result<[u64; 4], Error> {
	if let Some(n) = nonce {
		let len = header.len();
		let mut header = header.to_owned();
		if mutate_nonce {
			header.truncate(len - 4);
			header.write_u32::<LittleEndian>(n)?;
		}
		create_siphash_keys(&header)
	} else {
		create_siphash_keys(&header)
	}
}

pub fn create_siphash_keys(header: &[u8]) -> Result<[u64; 4], Error> {
	let h = blake2b(32, &[], &header);
	let hb = h.as_bytes();
	let mut rdr = Cursor::new(hb);
	Ok([
		rdr.read_u64::<LittleEndian>()?,
		rdr.read_u64::<LittleEndian>()?,
		rdr.read_u64::<LittleEndian>()?,
		rdr.read_u64::<LittleEndian>()?,
	])
}

#[cfg(test)]
mod tests {
	use super::*;
	// Solusi referensi dari solver CPU tromp (cuckatoo lean19x1) utk key yg sama.
	fn check_c19(k: [u64; 4], expected: &[u64]) {
		let trimmer = Trimmer::build(None, None, 19).expect("can't build trimmer");
		let res = trimmer.run(&k).unwrap();
		let sols = Graph::search(&res).unwrap();
		let found: Vec<Vec<u64>> = sols
			.iter()
			.map(|s| {
				let mut n = s.nonces.clone();
				n.sort();
				n
			})
			.collect();
		assert!(found.iter().any(|n| n[..] == expected[..]), "C19 solution not found: {:x?}", found);
	}

	// Batch: graf referensi di posisi 1 & 3 harus menghasilkan solusi yg sama dgn lean19x1.
	#[test]
	fn test_batch_c19() {
		let k74 = [0xd23109bd4dac0bdf, 0x76fbe03c31ad8133, 0xd17f301a7a865e22, 0xbaf57c804957a342];
		let k80 = [0x5a8792dce8508408, 0x0b253fd43e8d69ed, 0x821447be88274415, 0x1f209d183347b4f6];
		let other = [1u64, 2, 3, 4];
		let bt = BatchTrimmer::build(None, None, 19, Some(4), ocl_trims()).expect("can't build batch trimmer");
		let res = bt.run(&[other, k74, other, k80]).unwrap();
		let mut max_left = 0;
		for (g, first) in [(1usize, 0x19ceu64), (3, 0x1bc1)].iter() {
			let a = &res[g * batch::AUX_STRIDE..(g + 1) * batch::AUX_STRIDE];
			max_left = std::cmp::max(max_left, a[1]);
			let sols = Graph::search(a).unwrap();
			let mut found: Vec<Vec<u64>> = sols.iter().map(|s| { let mut n = s.nonces.clone(); n.sort(); n }).collect();
			found.retain(|n| n[0] == *first);
			assert_eq!(found.len(), 1, "graph {} solution missing", g);
		}
		println!("trims {} edges left max {}", ocl_trims(), max_left);
		let t0 = std::time::Instant::now();
		for _ in 0..50 {
			for g in 0..4 {
				let a = &res[g * batch::AUX_STRIDE..(g + 1) * batch::AUX_STRIDE];
				let _ = Graph::search(a);
			}
		}
		println!("cpu search per graph: {:?}", t0.elapsed() / 200);
	}

	#[test]
	fn test_solve_c19() {
		check_c19(
			[0xd23109bd4dac0bdf, 0x76fbe03c31ad8133, 0xd17f301a7a865e22, 0xbaf57c804957a342],
			&[
				0x19ce, 0xc1c9, 0xe95f, 0xfc42, 0x1034f, 0x1192c, 0x129a4, 0x12a44, 0x12a58, 0x178c4,
				0x1b393, 0x1eeb6, 0x1f047, 0x23272, 0x2449b, 0x25635, 0x263ca, 0x2b2ce, 0x2db7e, 0x2f224,
				0x3054d, 0x3468d, 0x374c4, 0x3c4e4, 0x451e1, 0x47cbb, 0x4c7d3, 0x53b59, 0x555af, 0x560c4,
				0x5a67c, 0x62c04, 0x65f3d, 0x695e0, 0x6fa68, 0x77639, 0x7a06b, 0x7b2f2, 0x7cba1, 0x7d03e,
				0x7d5b2, 0x7f26b,
			],
		);
		check_c19(
			[0x5a8792dce8508408, 0x0b253fd43e8d69ed, 0x821447be88274415, 0x1f209d183347b4f6],
			&[
				0x1bc1, 0x1f24, 0x2320, 0x3a4a, 0x3d20, 0x6831, 0x75c5, 0xfc66, 0x10db5, 0x12209,
				0x12457, 0x1337f, 0x161ca, 0x177f7, 0x1b2d7, 0x1c80a, 0x1d71c, 0x1e6e4, 0x25b57, 0x26706,
				0x26f29, 0x28b3f, 0x2975f, 0x2e969, 0x2f22d, 0x2f2ad, 0x3a4c0, 0x4f519, 0x5077d, 0x56913,
				0x60ba5, 0x61656, 0x68fa8, 0x6de89, 0x70187, 0x701b7, 0x761cf, 0x780f1, 0x7a6e8, 0x7bcbc,
				0x7d529, 0x7f2d9,
			],
		);
	}

	#[test]
	fn test_solve() {
		let trimmer = Trimmer::build(None, None, 29).expect("can't build trimmer");
		let k = [
			0x27580576fe290177,
			0xf9ea9b2031f4e76e,
			0x1663308c8607868f,
			0xb88839b0fa180d0e,
		];

		let res = trimmer.run(&k).unwrap();
		println!("Trimmed to {}", res.len());

		let sols = Graph::search(&res).unwrap();
		assert_eq!(1, sols.len());
		for sol in sols {
			println!("Solution: {:x?}", sol.nonces);
		}
	}
}
