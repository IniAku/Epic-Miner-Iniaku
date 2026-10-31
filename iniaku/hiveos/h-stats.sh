#!/usr/bin/env bash
# Sourced by the HiveOS agent: sets $khs and $stats.
# epic-miner has no API, so everything comes from the log since the last start (h-run.sh marker).
# Hashrate unit = graphs per second (gps), shown by HiveOS as H/s. Outside a Cuckoo turn
# (~96% of the time) the miner idles; we keep reporting the gps of the last turn so the
# rig does not look dead (and the HiveOS hashrate watchdog does not restart it).

. "$(dirname "${BASH_SOURCE[0]}")/h-manifest.conf"
log="$CUSTOM_LOG_BASENAME.log"

run=$(tac "$log" 2>/dev/null | sed "/^=== $CUSTOM_NAME .* start /q")
gps=$(grep -oP 'Cuck\(at\)oo at \K[0-9.]+(?= gps)' <<< "$run" | grep -v '^[0.]*$' | head -1)
gps=${gps:-0}
ac=$(grep -c 'Share Accepted' <<< "$run")
rj=$(grep -c 'Failed to submit a solution' <<< "$run")
uptime=$(ps -o etimes= -C epic-miner 2>/dev/null | sort -n | tail -1 | tr -d ' ')

gpu_stats=$(cat "${GPU_STATS_JSON:-/run/hive/gpu-stats.json}" 2>/dev/null)
# GPUs only (HiveOS also lists the CPU/iGPU as brand "cpu").
idx=$(jq -c '[.brand | to_entries[] | select(.value == "amd" or .value == "nvidia") | .key]' <<< "$gpu_stats" 2>/dev/null)
[[ -z $idx ]] && idx='[]'
temp=$(jq -c --argjson i "$idx" '[.temp[$i[]]]' <<< "$gpu_stats" 2>/dev/null || echo '[]')
fan=$(jq -c --argjson i "$idx" '[.fan[$i[]]]' <<< "$gpu_stats" 2>/dev/null || echo '[]')
bus=$(for b in $(jq -r --argjson i "$idx" '.busids[$i[]] | sub(":.*"; "")' <<< "$gpu_stats" 2>/dev/null); do echo $((16#$b)); done | jq -sc .)
n=$(jq length <<< "$idx")
((n < 1)) && n=1

# The miner only logs the rig total, so it is split evenly over the GPUs.
hs=$(jq -nc --argjson g "$gps" --argjson n "$n" '[range($n) | ($g / $n * 100 | round / 100)]')
khs=$(jq -n --argjson g "$gps" '$g / 1000')
stats=$(jq -nc \
	--argjson hs "$hs" --argjson temp "${temp:-[]}" --argjson fan "${fan:-[]}" --argjson bus "${bus:-[]}" \
	--argjson ac "$ac" --argjson rj "$rj" --argjson up "${uptime:-0}" --arg ver "$CUSTOM_VERSION" \
	'{hs: $hs, hs_units: "hs", temp: $temp, fan: $fan, bus_numbers: $bus, uptime: $up, ver: $ver,
	  ar: [$ac, $rj], algo: "cuckatoo19"}')
