// ProgPoW GPU self-test: mines fixed headers at given heights on one GPU and checks
// every GPU solution with the CPU light verifier (the same check the miner and the
// node use). A healthy GPU passes 100%; a broken kernel shows up as failed solutions.
//
//   ppcheck [--device N] [--seconds S] [--difficulty D] [--safe L] HEIGHT...
//
// --safe L rebuilds each period's kernel in the miner's safe mode L after the first call.
//
// INIAKU_PP_BARRIER2=1 builds the OpenCL kernel with an extra barrier after the
// shared-offset read (test for a local-memory race).

extern crate bigint;
extern crate progpow;

use bigint::uint::U256;
use progpow::hardware::{PpCPU, PpGPU};
use progpow::types::PpCompute;
use std::time::{Duration, Instant};

fn main() {
	let mut device: u32 = 0;
	let mut seconds: u64 = 60;
	let mut difficulty: u64 = 2_000_000;
	let mut heights: Vec<u64> = Vec::new();
	let mut safe: i32 = 0;
	let args: Vec<String> = std::env::args().skip(1).collect();
	let mut i = 0;
	while i < args.len() {
		match args[i].as_str() {
			"--device" => { i += 1; device = args[i].parse().unwrap(); }
			"--seconds" => { i += 1; seconds = args[i].parse().unwrap(); }
			"--difficulty" => { i += 1; difficulty = args[i].parse().unwrap(); }
			"--safe" => { i += 1; safe = args[i].parse().unwrap(); }
			h => heights.push(h.parse().expect("height")),
		}
		i += 1;
	}
	if heights.is_empty() {
		eprintln!("usage: ppcheck [--device N] [--seconds S] [--difficulty D] HEIGHT...");
		std::process::exit(2);
	}

	let driver: u8 = if cfg!(feature = "cuda") { 1 } else { 2 };
	let mut gpu = PpGPU::new(device, driver);
	gpu.init().unwrap();
	let cpu = PpCPU::new();

	let boundary = U256::max_value() / U256::from(difficulty);
	let target = (boundary >> 192).low_u64();
	println!(
		"device {} driver {} difficulty {} seconds/height {} safe {} barrier2 {}",
		device,
		driver,
		difficulty,
		seconds,
		safe,
		std::env::var("INIAKU_PP_BARRIER2").unwrap_or_default()
	);

	let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
	let mut next = move || {
		seed ^= seed << 13;
		seed ^= seed >> 7;
		seed ^= seed << 17;
		seed
	};

	for &height in &heights {
		let epoch = (height / 30000) as i32;
		let mut header = [0u8; 32];
		for b in header.iter_mut() {
			*b = next() as u8;
		}
		let (mut found, mut ok, mut bad_value, mut bad_mix) = (0u32, 0u32, 0u32, 0u32);
		let mut calls = 0u64;
		let start = Instant::now();
		let mut first = true;
		while start.elapsed() < Duration::from_secs(seconds) {
			let t = Instant::now();
			gpu.compute_with_startnonce(header, height, epoch, target, next());
			calls += 1;
			if first {
				println!("height {} period {} first call {:.1}s (DAG + kernel build)", height, height / 50, t.elapsed().as_secs_f64());
				first = false;
				if safe > 0 {
					gpu.set_safe_mode(safe);
					let t = Instant::now();
					gpu.compute_with_startnonce(header, height, epoch, target, next());
					let _ = gpu.get_solutions();
					println!("  safe mode {} rebuild {:.1}s", safe, t.elapsed().as_secs_f64());
				}
			}
			if !gpu.ready() {
				println!("height {}: GPU not ready", height);
				break;
			}
			if let Some((nonce, mix)) = gpu.get_solutions() {
				found += 1;
				let (v, cpu_mix) = cpu.verify(&header, height, nonce).unwrap();
				let digest: [u8; 32] = unsafe { std::mem::transmute(v) };
				let value_ok = U256::from(digest) <= boundary;
				let gpu_mix: [u32; 8] = unsafe { std::mem::transmute(mix) };
				let mix_ok = gpu_mix == cpu_mix;
				if value_ok && mix_ok {
					ok += 1;
				}
				if !value_ok {
					bad_value += 1;
				}
				if !mix_ok {
					bad_mix += 1;
				}
				if found <= 3 && !(value_ok && mix_ok) {
					println!("  nonce {:016x} value_ok {} mix_ok {}", nonce, value_ok, mix_ok);
				}
			}
		}
		let pct = if found > 0 { ok as f64 * 100.0 / found as f64 } else { 0.0 };
		println!(
			"RESULT height {} period {} calls {} gpu_solutions {} cpu_ok {} ({:.1}%) bad_value {} bad_mix {}",
			height,
			height / 50,
			calls,
			found,
			ok,
			pct,
			bad_value,
			bad_mix
		);
	}
}
