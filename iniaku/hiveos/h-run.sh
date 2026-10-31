#!/usr/bin/env bash
cd "$(dirname "$0")" || exit 1
. h-manifest.conf

b=$(cat .ocl_batch 2>/dev/null); [[ -n $b ]] && export OCL_BATCH=$b
# lan= from the flight sheet: pin the region hostname to the LAN IP (and drop an old pin).
read -r lan_ip lan_host < .lan_host 2>/dev/null
sed -i '/# iniaku-lan-'"$CUSTOM_NAME"'$/d' /etc/hosts 2>/dev/null
[[ -n $lan_ip ]] && echo "$lan_ip $lan_host # iniaku-lan-$CUSTOM_NAME" >> /etc/hosts
mkdir -p "$(dirname "$CUSTOM_LOG_BASENAME")"
# Marker: h-stats.sh only counts shares after the last start.
echo "=== $CUSTOM_NAME $CUSTOM_VERSION start $(date -u '+%F %T') ===" >> "$CUSTOM_LOG_BASENAME.log"
./epic-miner -c "$CUSTOM_CONFIG_FILENAME" 2>&1 | tee --append "$CUSTOM_LOG_BASENAME.log"
