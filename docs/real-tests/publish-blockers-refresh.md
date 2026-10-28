<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real checks for slot 362 publish-blockers-refresh (2026-10-28 05:47-05:50 CET, +01:00)

All read-only. No secrets read or printed. No VPS writes, no service change.

- VPS `systemctl is-active`: galion-nerve, gaia-cortex, console, console-rust-primary, hermes-rs, galion-one,
  openmind-queue active; hermes-gateway inactive.
- VPS `systemctl show`: cortex `gaia-cortex-73583ab-llm serve` (NRestarts 0, up since 2026-10-27 18:17 CET);
  nerve `openmind-nerve-9d1a4f3 serve --mode event` (up since 2026-10-27 18:15 CET).
- VPS `ss -ltnp`: cortex on 127.0.0.1:8674 (not 8672).
- VPS loopback GET codes: 8670 /api/v1/state 200, 8674 /health 200, 8080 / 302, 8080 /api/v1/admin 404,
  18080 /api/v1/admin 404, 8080 /api/arena 404, 8080 /api/v1/bank 404, 8002 /health 503, 8642 /health 200.
- Cortex /health: ok true, mode live, nerve true, incidents_open 2, escalations_open 1, llm_enabled true.
- Nerve /state: cycle 55, cycle_ms ~1.1 s; console status healthy (blind spot); alerts external "none configured";
  inc-865945dd mouth stuck loading 11.5 h; inc-06ffb32f edge 15 hostnames -> 6 closed ports, 234.2 failed req/min.
- `stat` metadata only: `/opt/galion/galion-light/publish/obsidian/link.env` 666 root:root.
- GitHub API (read-only): main 13a296ce; PR #12 open, head 38a07700, mergeable true, mergeable_state clean;
  commits 4828997d, 7fc44a00, 99cc25bd, 65fa0271, f68def1d "No commit found" (local only); 7091f76b found.
