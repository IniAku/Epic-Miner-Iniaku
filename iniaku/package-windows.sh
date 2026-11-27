#!/bin/sh
# Kemas rilis Windows di GitHub Actions: ./iniaku/package-windows.sh <versi> [opencl|cuda]
# cuda (2026-11-27): epic-miner with ProgPoW CUDA (EPIC_TARGET_DIR) + the NVRTC DLLs it compiles its
# kernel with; the NVIDIA driver itself provides nvcuda.dll.
set -eu
version=${1:?usage: package-windows.sh <version> [opencl|cuda]}
variant=${2:-opencl}
root=$(cd "$(dirname "$0")/.." && pwd)
rel="${EPIC_TARGET_DIR:-$root/target}/release"
name="epic-miner-iniaku-$version-windows-x64-$variant"
out="$root/dist/$name"

rm -rf "$out"
mkdir -p "$out/plugins"
cp "$rel/epic-miner.exe" "$out/"
cp "$rel/ocl_cuckatoo.dll" "$out/plugins/ocl_cuckatoo.cuckooplugin"
if [ "$variant" = cuda ]; then
	cudabin=$(cygpath -u "${CUDA_PATH:?CUDA_PATH not set}")/bin
	found=0
	for f in "$cudabin"/nvrtc64_*.dll "$cudabin"/nvrtc-builtins64_*.dll; do
		[ -f "$f" ] && cp "$f" "$out/" && found=1
	done
	[ "$found" = 1 ] || { echo "no NVRTC DLLs in $cudabin" >&2; exit 1; }
fi

# Config: plugin OpenCL saja (plugin CPU tromp tidak dibangun di MSVC).
awk -v f="$root/iniaku/plugins-opencl.toml" \
	'/^@PLUGINS@$/ { while ((getline l < f) > 0) print l; next } { print }' \
	"$root/iniaku/epic-miner.toml.in" | sed 's/$/\r/' > "$out/epic-miner.toml"
sed 's/$/\r/' "$root/iniaku/README.txt" > "$out/README.txt"
cp "$root/LICENSE" "$out/LICENSE.txt"
printf 'epic-miner.exe\r\npause\r\n' > "$out/start.cmd"

cd "$root/dist"
7z a -tzip "$name.zip" "$name" > /dev/null
sha256sum "$name.zip" > "$name.zip.sha256"
echo "$root/dist/$name.zip"
