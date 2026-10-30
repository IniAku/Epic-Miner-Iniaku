// Copyright 2026 epic.iniaku.my.id
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Server pool epic.iniaku.my.id yang ditanam di miner. Build ini hanya menambang ke pool
//! tersebut: `stratum_server_addr` di config diabaikan, yang bisa dipilih hanya region.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use util::LOGGER;

/// (region, host) — stratum TLS di port `POOL_PORT`.
pub const POOLS: [(&str, &str); 3] = [
	("id", "id.epic.iniaku.my.id"),
	("us", "us.epic.iniaku.my.id"),
	("au", "au.epic.iniaku.my.id"),
];
pub const POOL_PORT: u16 = 3334;
pub const REGIONS: &str = "auto, id, us, au";

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

pub fn is_valid_region(region: &str) -> bool {
	region == "auto" || POOLS.iter().any(|(r, _)| *r == region)
}

pub fn addr(host: &str) -> String {
	format!("{}:{}", host, POOL_PORT)
}

/// Waktu buka koneksi TCP ke server (perkiraan ping); None kalau tidak terjangkau.
pub fn probe(host: &str) -> Option<Duration> {
	let sock = addr(host).to_socket_addrs().ok()?.next()?;
	let start = Instant::now();
	TcpStream::connect_timeout(&sock, PROBE_TIMEOUT).ok()?;
	Some(start.elapsed())
}

/// Urutan server: region pilihan (atau yang ping-nya terkecil kalau "auto") di depan,
/// sisanya urut ping sebagai cadangan failover. Server yang tidak terjangkau di belakang.
pub fn ordered_servers(region: &str) -> Vec<String> {
	let mut measured: Vec<(&str, &str, Option<Duration>)> = POOLS
		.iter()
		.map(|(r, h)| (*r, *h, probe(h)))
		.collect();
	for (r, h, p) in &measured {
		match p {
			Some(d) => info!(LOGGER, "Pool {} ({}): {} ms", r, h, d.as_millis()),
			None => warn!(LOGGER, "Pool {} ({}): unreachable", r, h),
		}
	}
	measured.sort_by_key(|(_, _, p)| p.unwrap_or(Duration::from_secs(3600)));
	if region != "auto" {
		if let Some(pos) = measured.iter().position(|(r, _, _)| *r == region) {
			let chosen = measured.remove(pos);
			measured.insert(0, chosen);
		}
	}
	measured.iter().map(|(_, h, _)| addr(h)).collect()
}
