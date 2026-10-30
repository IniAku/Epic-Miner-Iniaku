#!/bin/sh
# Versi rilis: dari tag (v2.3.1-iniaku.1 -> 2.3.1-iniaku.1), selain itu <versi Cargo>-iniaku-dev.<commit>.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
tag=$(git -C "$root" describe --tags --exact-match 2>/dev/null || true)
case "$tag" in
v*) echo "${tag#v}" ;;
*) echo "$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)-iniaku-dev.$(git -C "$root" rev-parse --short HEAD)" ;;
esac
