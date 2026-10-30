#!/bin/sh
# Kemas rilis Windows (OpenCL) di GitHub Actions: ./iniaku/package-windows.sh <versi>
set -eu
version=${1:?usage: package-windows.sh <version>}
root=$(cd "$(dirname "$0")/.." && pwd)
rel="$root/target/release"
name="epic-miner-iniaku-$version-windows-x64-opencl"
out="$root/dist/$name"

rm -rf "$out"
mkdir -p "$out/plugins"
cp "$rel/epic-miner.exe" "$out/"
cp "$rel/ocl_cuckatoo.dll" "$out/plugins/ocl_cuckatoo.cuckooplugin"

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
