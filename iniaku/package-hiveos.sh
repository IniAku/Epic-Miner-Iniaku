#!/bin/sh
# HiveOS custom-miner package from a Linux release tarball:
#   ./iniaku/package-hiveos.sh <linux release .tar.gz> <version> [out dir]
# Result: <CUSTOM_NAME>-<hive version>.tar.gz with a single <CUSTOM_NAME>/ folder (HiveOS layout).
# The hive version has no "-" (2.3.1-iniaku.7 -> 2.3.1.7) so HiveOS reads the name correctly.
set -eu
src=${1:?usage: package-hiveos.sh <linux .tar.gz> <version> [out dir]}
version=${2:?usage: package-hiveos.sh <linux .tar.gz> <version> [out dir]}
root=$(cd "$(dirname "$0")/.." && pwd)
out=$(cd "${3:-$root/dist}" && pwd)
name=$(sed -n 's/^CUSTOM_NAME=//p' "$root/iniaku/hiveos/h-manifest.conf")
hive_version=$(echo "$version" | sed 's/-iniaku\./\./; s/-/./g')

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tar -C "$tmp" -xzf "$src"
mv "$tmp"/*/ "$tmp/$name"
for f in h-config.sh h-run.sh h-stats.sh; do
	install -m 755 "$root/iniaku/hiveos/$f" "$tmp/$name/$f"
done
sed "s/@VERSION@/$hive_version/" "$root/iniaku/hiveos/h-manifest.conf" > "$tmp/$name/h-manifest.conf"
tar -C "$tmp" --owner=0 --group=0 -czf "$out/$name-$hive_version.tar.gz" "$name"
(cd "$out" && sha256sum "$name-$hive_version.tar.gz" > "$name-$hive_version.tar.gz.sha256")
echo "$out/$name-$hive_version.tar.gz"
