#!/bin/sh
# Terapkan perbaikan build ke submodule (patches/submodules/*.patch); aman dijalankan ulang.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
apply() {
	dir=$1 patch=$2
	[ -s "$patch" ] || return 0
	if git -C "$root/$dir" apply --check "$patch" 2>/dev/null; then
		git -C "$root/$dir" apply "$patch"
		echo "applied $patch"
	else
		echo "skip $patch (already applied or not applicable)"
	fi
}
apply randomx-rust "$root/patches/submodules/randomx-rust.patch"
apply randomx-rust/randomx "$root/patches/submodules/randomx-rust-randomx.patch"
apply progpow-rust "$root/patches/submodules/progpow-rust.patch"
