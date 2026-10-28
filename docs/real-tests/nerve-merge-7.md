<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-merge-7 real tests (slot 411), 2026-10-28 CET (+01:00)

Box ports 22810-22819 (`REAL_NERVE_PORT_BASE=22810`). HEAD a5979a96 (tests) + docs.

## F-396-1 blind console -> degrading (real_console_blindspot, 18:37:49)

Consoles: blind = `artifacts/t291u/galion-console-serve-2d846fb8` (byte-identical
to the console live 2026-10-27 19:28 -> 2026-10-28 17:35 CET), fixed =
`artifacts/galion-console-serve-5aa31927` (live now).

```
direct GET :22811 -> / 302, /api/v1/admin 404, /api/v1/services 404
F-396-1 rooms :22811 (Host console.galion.app, loopback) -> /console 503/200, /node 503/200, /hermes 503/200, /status 503/200
F-396-1 rooms :22813 (5aa31927 + store) -> all 200/200
console          degrading  reasons: deep route /api/v1/admin HTTP 404, /api/v1/services HTTP 404; consecutive_down 0
console-noadmin  degrading  deep route /api/v1/admin HTTP 503 -> store added -> healthy after 5.1 s
console-full     healthy
system degrading, HEPHAESTUS degrading, /health.system degrading, incident warn cause=console
blind killed -> failing (hard), no deep record; admin dirs untouched (GET only)
test result: ok. 1 passed (15.6 s)
```

Controls with `NERVE_BLINDSPOT_NERVE_BIN`:
- live `openmind-nerve-9baa450` (18:38:54): **passes** (same verdicts) -> the
  deep console check is already live since 17:40 CET; D-396-2's detection part is done.
- old `openmind-nerve-12956b4` (18:39:14): **fails** as designed
  (`F-313-2 REGRESSED: nerve reports "console" healthy`).

## real_console_deep (18:39:26): 1/1 ok (blind degrading, nodeep healthy, full healthy -> killed failing).

## real_edge_overcount (F-380-1)

- engine replay (18:39:26): failed_requests=900 rate=30.03/min window_s=1798, 1/1.
- VPS journal leg `GALION_REAL_VPS=1` (18:39:50, read-only journalctl, hosts scrubbed):
  lines=737 once_failed=737 eng_failed=737 rate=60.08/min, 1/1.
- Live 9baa450 evidence (read-only `/api/v1/state` 18:39:53): incident inc-94f3b84e
  title "16.4 failed requests/min over 29 min" while `logs.edge_failures`=311 in
  window 17:55:13-18:25:07 (= 10.7/min) and cloudflared journal 18:10-18:40 has 635
  "Request failed" lines (~10.6/min at 2 lines/request). Live is already ~1.5x high
  after 1 h uptime (it grows with uptime; F-380-1 measured ~37x before).

## real_build_stamp (18:43:34-18:45:16): 1/1 ok, 103 s

- A = B = C sha256 61e6ff81..., 1,483,552 B, `--version` "openmind-nerve 0.1.0 (commit a5979a96, built 2026-10-28T18:36:52+01:00, features none)"
- D (no git) b7f26ffb..., commit unknown
- Z base 9baa450c 1,479,456 B sha256 8c9d7a5f... = live artifact sha -> delta **+4,096 B**
  (the stamp page; F-380-1 adds 0 B). Budget 2,097,152 B (config/release/size-budget.tsv):
  headroom 613,600 B (F-376-1 ok).
- A/C/D served on 22810-22812: startup + `/health` commit/build as expected.

## Gates

fmt --all ok; clippy --workspace --all-targets -D warnings ok (excl. sandbox/cgroup-detect);
cargo test --workspace (excl. openmind-sandbox, cgroup-detect) 1368 pass / 0 fail / 6 ignored, 191 suites
(18:45:37-18:49:17); openmind-nerve alone all green; no-python ok; spdx 0 missing;
repo-check 11 err / 53 warn = same 11 errors as base eaa5d0ac (vendor/candle core::arch x10,
guardian-tick nerve-client x1; pre-existing), +1 warn (SystemTime in real_edge_overcount rate leg).
aarch64 musl release (rust-lld) 1,223,408 B, qemu-aarch64-static `--version` stamped a5979a96;
cargo check aarch64-linux-android + armv7-unknown-linux-musleabihf ok.

## VPS read-only (18:39-18:45)

galion-nerve MainPID 1251569, NRestarts 0, ExecStart `/opt/galion/rust/bin/openmind-nerve-9baa450 serve --mode event`
(start 17:40:13 CET); `/health` ok, system degraded (galion-one-mouth 503 loading, cloudflared
failing-soon, tuner-quick failing, voice-worker absent); live console + console-rust-primary
healthy with deep /api/v1/admin 200 + /api/v1/services 200; public https://console.galion.app
/console 200, /status 200. No writes, no service change.
