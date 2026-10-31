# HiveOS flight sheet — epic-miner (Cuckatoo19, epic.iniaku.my.id)

| Field | Value |
|---|---|
| Coin | EPIC (add it as a custom coin if missing) |
| Wallet | your Epic address, e.g. `es...@epicbox.iniaku.my.id` |
| Pool | Configure in miner |
| Miner | Custom |
| Miner name | auto-filled from the URL |
| Installation URL | `https://epic.iniaku.my.id/storage/hiveos/iniaku_cuckoo-<version>.tar.gz` |
| Hash algorithm | `cuckatoo31` (HiveOS has no cuckatoo19 entry; it is only a dashboard label, the miner always mines C19) |
| Wallet and worker template | `%WAL%.%WORKER_NAME%` |
| Pool URL | `us.epic.iniaku.my.id` / `id.epic.iniaku.my.id` / `au.epic.iniaku.my.id` (anything else = auto) |
| Pass | `x` |
| Extra config arguments | optional: `region=us amd=0,1 nvidia=none batch=512` |

- Every AMD and NVIDIA GPU is used via OpenCL by default (one solver per GPU).
- Epic gives Cuckoo ~4% of blocks, so the GPUs only work during a Cuckoo turn. Between turns the
  dashboard keeps showing the speed of the last turn (graphs/s, shown as H/s), so the hashrate
  watchdog does not restart the miner.
- The miner logs only the rig total, so per-GPU speed is the total split evenly.
- Runs fine next to another miner (e.g. SRBMiner for ProgPoW) in a second flight sheet slot.
