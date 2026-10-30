epic-miner — epic.iniaku.my.id edition
======================================

Cuckatoo19 (Cuckoo) miner for Epic Cash, based on the official EpicCash/epic-miner.
This build mines ONLY to the epic.iniaku.my.id pool (ID / US / AU servers, TLS port 3334).

Quick start
-----------
1. Open epic-miner.toml and set:
       stratum_server_login = "<your Epic address>.<worker name>"
2. Linux:   ./epic-miner
   Windows: epic-miner.exe
3. Check your worker at https://epic.iniaku.my.id

Region
------
region = "auto" (default) pings id / us / au and uses the closest one, switching
automatically to another server if it goes down. Force one with region = "id" | "us" | "au"
in epic-miner.toml, or on the command line: --region us

Variants
--------
  opencl : AMD GPUs (also works on NVIDIA)  -> plugin "ocl_cuckatoo"
  cuda   : NVIDIA GPUs                      -> plugin "cuckatoo_lean_cuda_19"
  Linux packages also include CPU plugins (cuckatoo_mean_cpu_compat_19 / _avx2_19).

Several GPUs: copy the [[mining.miner_plugin_config]] block once per GPU and set
"device" to 0, 1, 2, ...

"Waiting for Cuckoo turn"
-------------------------
Epic rotates the algorithm every block (ProgPoW / RandomX / Cuckoo). Cuckoo gets about
4% of blocks, so the miner is idle most of the time and mines at full speed during a
Cuckoo turn. This is normal.

What this edition fixes compared to the official miner
------------------------------------------------------
- Cuckatoo19 on OpenCL (the official OpenCL plugin forced Cuckatoo31, worth ~94x less on Epic)
- Valid C19 shares were discarded by the miner (difficulty checked without graph weight)
- TLS: the official miner verified the wrong hostname, so TLS to subdomains always failed
- Graph weight for C19 now matches the Epic node exactly
- CPU plugins built for generic x86-64 (no crash on older CPUs)

Source & releases: https://github.com/IniAku/epic-miner-iniaku  —  License: Apache-2.0 (see LICENSE)
