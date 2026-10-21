<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: cortex-t31-calibrate (slot 207), 2026-10-21 +02:00

Box only. Release binary `openmind-nerve` built from this branch. No socket opened (CLI only); ports 20770-20779
reserved and unused. VPS reads were read-only.

## 1. Fresh VPS capture (19:51 +02:00, read-only)

`ssh slavik-vps`: 3x `curl -s 127.0.0.1:8670/state` 30 s apart, `curl -s 127.0.0.1:8660/status.json`, `ss -ltn`,
`journalctl -o json --output-fields=UNIT,_SYSTEMD_UNIT,MESSAGE,PRIORITY,__REALTIME_TIMESTAMP --since -30min`
into `/workspace/swarm/nerve-capture/replay207` (box only, never committed: it holds real journal lines).

- The live nerve is `openmind-nerve-bb287cc serve --mode event` since 19:16:57 +02:00 (cycle 4 at 19:46:57). The 3
  GETs returned the identical snapshot (same `ts`, same md5), so one state file is kept.
- The live cortex is `gaia-cortex-c7f56b8` on 127.0.0.1:8674 (unit `gaia-cortex.service`, dry-run).
  `GET /health` 19:53:07: `ok true, mode dry-run, nerve true, incidents_open 1, events_in 8, llm_enabled true`.
  `GET /incidents`: one open, `nerve-incident:ddos:edge-cloudflared` warn, opened 19:46:59 +02:00.
- One helper GET wrote a 189-byte body to `/tmp/.c207` on the VPS and removed it in the same command (no other write).

## 2. `openmind-nerve calibrate` on all 8 live captures (`GALION_REAL=1 cargo test --release --test real_calibrate`)

```
replay1: 210 steps, invalid_user max 0, preauth_other max 0, edge rps max 0.43, floor10/8 episodes 0/0, dead-origin failed/min min 8.2
replay158: 182 steps, invalid_user max 2, preauth_other max 2, edge rps max 5.97, floor10/8 episodes 1/2, dead-origin failed/min min 8.0
replay179: 181 steps, invalid_user max 8, preauth_other max 94, edge rps max 3.75, floor10/8 episodes 0/0, dead-origin failed/min min 8.0
replay186: 212 steps, invalid_user max 2, preauth_other max 5, edge rps max 0.4, floor10/8 episodes 0/0, dead-origin failed/min min 7.6
replay192: 181 steps, invalid_user max 3, preauth_other max 7, edge rps max 2.3, floor10/8 episodes 0/0, dead-origin failed/min min 7.8
replay193: 182 steps, invalid_user max 1, preauth_other max 2, edge rps max 0.33, floor10/8 episodes 0/0, dead-origin failed/min min 7.6
replay200: 210 steps, invalid_user max 0, preauth_other max 0, edge rps max 2.27, floor10/8 episodes 0/0, dead-origin failed/min min 7.6
replay207: 181 steps, invalid_user max 1, preauth_other max 1, edge rps max 5.08, floor10/8 episodes 1/2, dead-origin failed/min min 7.6
calibrate: 8 captures in 1164 ms
```

Binary output = the committed `calibration.json`, no IPv4 address or hostname in it.

## 3. Verdicts through the real binary

```
band (NERVE_SSHD=1): final 96 A, timeline 96 A x5 then 100 A x18 (sshd-invalid-user warn + memory-heavy-galion-desk info)
replay1 sshd=false/true: final 56 D held 62 D, edge error
replay158 / 179 / 186 / 192 / 193 / 207 sshd=false/true: final 66 D held 66 D, edge error
replay200 sshd=false/true: final 76 C held 66 D, edge error
```

All exit 0 (no flap over 20 hold cycles).

## 4. Fresh VPS window inside the test (`GALION_REAL_VPS=1`, 19:57 +02:00)

```
fresh VPS window: 182 steps, ssh lines 126, invalid_user max 1, preauth_other max 2, edge rps max 5.08,
floors 10: 1 episode (3 hot steps) / 8: 2 episodes / 6: 2 / 5: 2 (3 flood steps) / 4: 2 / 3: 2
live cortex open incidents: ["nerve-incident:ddos:edge-cloudflared warn"]
```

Same picture as the corpus: the tightened sshd and memory lines have their headroom on brand-new windows, and a
floor below 10 req/s would flap on the dead-origin bursts. The temp capture was deleted.

## 5. Gates

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 218 suites, 1056 passed, 0 failed,
  3 ignored. openmind-nerve lib 155 (incl. calibrate 3, sshd/components boundary tests), `calibration` 5/5,
  `replay_corpus` 4/4 (17 entries), `real_calibrate` 3/3 and `real_replay_corpus` 3/3 with `GALION_REAL=1`.
- `cargo xtask replay-corpus --require-live` ok. `scripts/no-python.sh` ok.
- repo-check: 3 errors, the same 3 as base 55f07ff (`sensors/surface.rs` 198.18.0.0 lines, not touched here).
- x86_64 musl release 1 602 336 B, aarch64 musl release (rust-lld) 1 333 848 B; aarch64-android and
  armv7-musleabihf `cargo check` ok. No new dependency; Cargo.lock unchanged.
