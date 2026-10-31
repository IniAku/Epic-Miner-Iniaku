#!/usr/bin/env bash
cd "$(dirname "$0")" || exit 1
. h-manifest.conf

[[ -s .ocl_batch ]] && export OCL_BATCH=$(<.ocl_batch)
mkdir -p "$(dirname "$CUSTOM_LOG_BASENAME")"
# Marker: h-stats.sh only counts shares after the last start.
echo "=== $CUSTOM_NAME $CUSTOM_VERSION start $(date -u '+%F %T') ===" >> "$CUSTOM_LOG_BASENAME.log"
./epic-miner -c "$CUSTOM_CONFIG_FILENAME" 2>&1 | tee --append "$CUSTOM_LOG_BASENAME.log"
