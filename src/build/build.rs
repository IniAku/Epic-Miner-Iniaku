// Copyright 2018 The Grin Developers
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

//! Build hooks to spit out version+build time info

extern crate built;
use std::env;

fn main() {
	// Hash commit utk banner versi (pengganti built::GIT_VERSION/libgit2).
	if let Ok(out) = std::process::Command::new("git")
		.args(&["rev-parse", "--short", "HEAD"])
		.output()
	{
		if out.status.success() {
			let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
			println!("cargo:rustc-env=EPIC_MINER_GIT_HASH={}", hash);
		}
	}
	let mut opts = built::Options::default();
	opts.set_dependencies(true);
	let built_path = format!("{}{}", env::var("OUT_DIR").unwrap(), "/built.rs");
	built::write_built_file_with_opts(&opts, env!("CARGO_MANIFEST_DIR"), &built_path)
		.expect("Failed to acquire build-time information");
	// built 0.2 menulis path Windows (RUSTC = "C:\Users\...") tanpa escape → "unknown
	// character escape". Ganti backslash di konstanta berisi path drive dgn slash.
	let src = std::fs::read_to_string(&built_path).unwrap();
	let fixed: Vec<String> = src
		.lines()
		.map(|l| {
			if l.starts_with("pub const ") && l.contains(":\\") {
				l.replace('\\', "/")
			} else {
				l.to_string()
			}
		})
		.collect();
	std::fs::write(&built_path, fixed.join("\n") + "\n").unwrap();
}
