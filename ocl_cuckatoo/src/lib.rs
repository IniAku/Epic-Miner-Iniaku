extern crate blake2_rfc;
extern crate byteorder;
extern crate epic_miner_plugin as plugin;
extern crate hashbrown;
extern crate libc;
extern crate ocl;

use blake2_rfc::blake2b::blake2b;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use libc::*;
use plugin::*;
use std::io::Cursor;
use std::io::Error;
use std::mem;
use std::ptr;
use std::time::{Duration, SystemTime};

pub use self::finder::Graph;
pub use self::trimmer::Trimmer;

mod finder;
mod trimmer;

#[repr(C)]
struct Solver {
	edge_bits: u8,
	trimmer: Trimmer,
	graph: Option<Graph>,
	mutate_nonce: bool,
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
	let trimmer = Trimmer::build(platform, device_id, edge_bits).expect("can't build trimmer");
	let solver = Solver {
		edge_bits,
		trimmer: trimmer,
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
	_range: uint32_t,
	solutions: *mut SolverSolutions,
	stats: *mut SolverStats,
) -> uint32_t {
	let start = SystemTime::now();
	let solver_ptr = mem::transmute::<*mut SolverCtx, *mut Solver>(ctx);
	let solver = &*solver_ptr;
	let mut header = Vec::with_capacity(header_length as usize);
	let r_ptr = header.as_mut_ptr();
	ptr::copy_nonoverlapping(header_ptr, r_ptr, header_length as usize);
	header.set_len(header_length as usize);
	let n = nonce as u32;
	let k = match set_header_nonce(&header, Some(n), solver.mutate_nonce) {
		Err(_e) => {
			return 2;
		}
		Ok(v) => v,
	};
	let res = solver.trimmer.run(&k).unwrap();

	let sols = Graph::search(&res).unwrap();
	let end = SystemTime::now();
	let elapsed = end.duration_since(start).unwrap();
	let mut i = 0;
	(*solutions).edge_bits = solver.edge_bits as u32;
	(*solutions).num_sols = sols.len() as u32;
	for sol in sols {
		(*solutions).sols[i].nonce = nonce;
		(*solutions).sols[i]
			.proof
			.copy_from_slice(&sol.nonces[..sol.nonces.len()]);
		i += 1;
	}
	(*stats).edge_bits = solver.edge_bits as u32;
	(*stats).device_id = solver.trimmer.device_id as u32;
	let name_bytes = solver.trimmer.device_name.as_bytes();
	let stats_ref = &mut *stats;
	let n = std::cmp::min(stats_ref.device_name.len(), name_bytes.len());
	stats_ref.device_name[..n].copy_from_slice(&solver.trimmer.device_name.as_bytes()[..n]);
	(*stats).last_solution_time = duration_to_u64(elapsed);
	(*stats).last_start_time =
		duration_to_u64(start.duration_since(SystemTime::UNIX_EPOCH).unwrap());
	(*stats).last_end_time = duration_to_u64(end.duration_since(SystemTime::UNIX_EPOCH).unwrap());
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
