#!/bin/sh
# Kemas rilis Linux epic-miner edisi epic.iniaku.my.id: ./iniaku/package-linux.sh <opencl|cuda> <versi>
# Jalankan dari root repo setelah `cargo build --release` (+ -p ocl_cuckatoo / fitur cuda).
set -eu

variant=${1:?usage: package-linux.sh <opencl|cuda> <version>}
version=${2:?usage: package-linux.sh <opencl|cuda> <version>}
root=$(cd "$(dirname "$0")/.." && pwd)
rel="${EPIC_TARGET_DIR:-$root/target}/release"
name="epic-miner-iniaku-$version-linux-x64-$variant"
out="$root/dist/$name"

rm -rf "$out"
mkdir -p "$out/plugins"

cp "$rel/epic-miner" "$out/"
# Build iniaku (--no-default-features --features cpu) tanpa RandomX; build default butuh
# librandomx.so di sebelah binary (rpath $ORIGIN).
if ldd "$rel/epic-miner" | grep -q librandomx; then
	cp "$(find "$rel/build" -name librandomx.so | head -1)" "$out/"
fi
# ProgPoW (opencl feature): libppow.so next to the binary (rpath $ORIGIN)
if ldd "$rel/epic-miner" | grep -q libppow; then
	cp "$(find "$rel/build" -name libppow.so | head -1)" "$out/"
fi
for p in cuckatoo_mean_cpu_compat_19 cuckatoo_mean_cpu_avx2_19 cuckatoo_lean_cpu_compat_19; do
	cp "$rel/plugins/$p.cuckooplugin" "$out/plugins/"
done

case "$variant" in
opencl)
	cp "$rel/libocl_cuckatoo.so" "$out/plugins/ocl_cuckatoo.cuckooplugin"
	;;
cuda)
	for p in cuckatoo_lean_cuda_19; do
		cp "$rel/plugins/$p.cuckooplugin" "$out/plugins/"
	done
	;;
*)
	echo "unknown variant: $variant" >&2
	exit 1
	;;
esac

# Config default: plugin GPU aktif, CPU sebagai contoh (dikomentari).
{
	cat "$root/iniaku/plugins-$variant.toml"
	echo
	sed 's/^/#/' "$root/iniaku/plugins-cpu.toml"
} > "$out/.plugins.toml"
awk -v f="$out/.plugins.toml" '/^@PLUGINS@$/ { while ((getline l < f) > 0) print l; next } { print }' \
	"$root/iniaku/epic-miner.toml.in" > "$out/epic-miner.toml"
rm "$out/.plugins.toml"

cp "$root/iniaku/README.txt" "$root/LICENSE" "$out/"
tar -C "$root/dist" -czf "$root/dist/$name.tar.gz" "$name"
sha256sum "$root/dist/$name.tar.gz" | sed "s#$root/dist/##" > "$root/dist/$name.tar.gz.sha256"
echo "$root/dist/$name.tar.gz"
