#!/bin/sh
# Build rilis Linux di container Ubuntu 20.04, lalu kemas: ./iniaku/build-linux-release.sh <versi>
set -eu
version=${1:?usage: build-linux-release.sh <version>}
root=$(cd "$(dirname "$0")/.." && pwd)
img=epic-miner-iniaku-builder:20.04
# Dijalankan via sudo (user tidak di grup docker): file hasil build tetap milik user asli.
uid=${SUDO_UID:-$(id -u)}
gid=${SUDO_GID:-$(id -g)}
userhome=$(getent passwd "${SUDO_USER:-$(id -un)}" | cut -d: -f6)
docker build -t "$img" -f "$root/iniaku/docker/Dockerfile.linux" "$root/iniaku/docker"
# target terpisah (target-u2004) supaya tidak tercampur build host yg butuh glibc baru.
docker run --rm --cpus=4 -u "$uid:$gid" -e HOME=/tmp/home \
	-e CARGO_HOME=/tmp/home/.cargo \
	-v "$root":/src -v "$userhome/.cargo/registry":/tmp/home/.cargo/registry \
	"$img" sh -euc '
		mkdir -p /tmp/home/.cargo
		export CARGO_TARGET_DIR=/src/target-u2004
		export CMAKE_POLICY_VERSION_MINIMUM=3.5 RUSTFLAGS="-C link-args=-Wl,-rpath,\$ORIGIN"
		cargo build --release --no-default-features --features cpu
		cargo build --release -p ocl_cuckatoo
		cargo build --release -p cuckoo_miner --features cuckoo_miner/build-cuda-plugins
	'
for v in opencl cuda; do
	setpriv --reuid="$uid" --regid="$gid" --clear-groups env EPIC_TARGET_DIR="$root/target-u2004" \
		"$root/iniaku/package-linux.sh" "$v" "$version"
done
