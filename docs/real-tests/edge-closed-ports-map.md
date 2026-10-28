<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests for slot 380 edge-closed-ports-map (2026-10-28, CET +01:00)

All VPS access read-only (`ssh slavik-vps`: `systemctl show`, `ss -ltnp`,
`journalctl` reads, loopback GETs). No secrets read or printed; the cloudflared
config was read with `tunnel:` / `credentials-file:` / `origin-cert:` lines
filtered out on the VPS. No VPS writes, no DNS or cloudflared change. Ports
22500-22509 on the box only. VPS clock is UTC; times below converted to CET.

## VPS captures (10:06-10:15 CET)
- cloudflared active, NRestarts 0, up since 2026-10-22 22:46:47 CET,
  `cloudflared tunnel --no-autoupdate --config .../cloudflared/config.yml run`.
- Config 3420 B, mode 644 root, mtime 2026-10-21 16:08:54 CET: 39 rules.
- `ss -ltnp`: 19 listeners; of the ingress ports only 8671, 8091, 8080, 8650 listen.
- Unit grep for all 20 closed ports over /etc/systemd/system and /lib/systemd/system: 0 hits; no pm2/docker/podman; 0 failed units.
- Journal 24 h: 43,966 refused lines = 21,983 failed requests; last 30 min 328.
- Metrics: total 241,793, errors 122,227, 404 117,981, 200 1,353; 60 s sample 10:09:44-10:10:44 CET: +11 requests, +10 errors, +1 404.
- Nerve /state 10:00 CET: `edge-dead-origins` "19 hostnames -> 9 closed ports :3004 :3010 :3011 :3333 :4001 :4020 :4060 :5530 :8000: 318.0 failed requests/min over 29 min", window 09:30:01-09:59:47, edge_failures 387; journal same window 772 refused lines (F-380-1).
- Cortex /health 10:14:14 CET: ok, mode live, nerve true, incidents_open 2, escalations_open 11, hermes_handoff true.
- Public (box, 10:08 CET): DoH status 3 (NXDOMAIN) for jupyter.adriaticpadel.com, villapenava.com, www.villapenava.com, www.tactical-belts.galion.app, tuner.galion.app; all other 33 names resolve to Cloudflare. HTTPS GET: one/galion.app/www/obsidian 200, console 302, code.galion.app 404, the 27 resolving closed names 502.

## Rust tests (`crates/tunnel-lint`)
Built release `tunnel-lint` + `tuner` (box, CARGO_TARGET_DIR target/edge-closed-ports-map).

- `cargo test --release -p tunnel-lint`: closed_ports_map 5/5, lint 13/13,
  real_* gated skip; unit 0/0.
- `GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p tunnel-lint --test real_edge_closed_ports_map -- --nocapture --test-threads 1` (10:13:36-10:13:42 CET): 2/2 pass, 5.14 s.
  - `real_20261028_map_on_box`: 4 real tuner origins on 22500-22503 (GET 200 each); live lint exit 1,
    `summary: 39 rules, 31 closed routes on 6 ports [22504, 22505, 22506, 22507, 22508, 22509], 31 findings`,
    7 OPEN / 31 CLOSED / 1 SKIP; staged re-lint exit 0 `8 rules, 0 closed routes`; forge origin started on 22509 ->
    `39 rules, 30 closed routes on 5 ports [22504..22508]`, forge OPEN; teardown leaves 22500-22509 free.
  - `real_vps_live_map_matches_fixture`: live VPS capture linted with `--ss` ->
    `39 rules, 31 closed routes on 20 ports [3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444, 5530, 8000, 8889, 18800], 31 findings`, rule table identical to the fixture.
- Negative control (10:14 CET): appended a fake `ssh` listener on 127.0.0.1:18800 to the ss fixture ->
  closed_ports_map 4/5 FAIL (map, open owners, drift, staged) and the VPS test FAILED with
  `DRIFT live 38 CLOSED forge.galion.studio ... vs fixture 38 OPEN ...`; fixture restored, 5/5 pass again.
- CLI on the fixture: exit 1 with the same 31 findings; `--stage` output committed as
  `docs/ops/edge-closed-ports-map.config.staged.yml` (pinned by test).
