<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: cortex-connrate-fix (slot 178, corrected B41 conn-rate formula)

- Branch `swarm/cortex-connrate-fix`, base `swarm/cortex-merge-all` 6bcb882. Ports 20480-20489.
- Code: `crates/openmind-nerve/src/sensors/connrate.rs` (`b41_flow_rate`, `Formula`, `NERVE_CONNRATE_FORMULA`,
  off by default). Design and owner decisions: docs/cortex/conn-rate-formula.md.
- 6bcb882 did not fix it: connrate.rs is unchanged since cortex-s2-b 51100e1.

## 1. VPS read-only sample -> replay (unit test, runs in every `cargo test`)

Taken 2026-10-21T09:24:42+02:00 to 09:25:42+02:00: `ssh slavik-vps 'cat /proc/net/tcp /proc/net/tcp6'` three
times, 30 s apart (read-only; nothing written or changed on the VPS). Redacted to the watched ports (all rows
loopback, uid/inode zeroed) in `crates/openmind-nerve/tests/fixtures/conn-rate-vps/`. Nerve observed at
09:26:00+02:00: GET 127.0.0.1:8670/state 200, cycle 7062; cortex not live (`/api/v1/state` 404), so the nerve is
the live view.

`cargo test -p openmind-nerve --lib connrate -- --nocapture` (`replay_vps_sample_b41_vs_identity_vs_flow`):

```
VPS :8642 w0: estab+tw 87->84; identity 1.37/s; b41 old 0.00/s; b41flow 1.35/s
VPS :8642 w1: estab+tw 84->85; identity 1.37/s; b41 old 0.03/s; b41flow 1.43/s
VPS :8091 w0: estab+tw 70->63; identity 1.07/s; b41 old 0.00/s; b41flow 0.93/s
VPS :8091 w1: estab+tw 63->64; identity 1.03/s; b41 old 0.03/s; b41flow 1.08/s
VPS :8420 w0: estab+tw 11->7; identity 0.13/s; b41 old 0.00/s; b41flow 0.05/s
VPS :8420 w1: estab+tw 7->7; identity 0.10/s; b41 old 0.00/s; b41flow 0.12/s
VPS :8646 w0: estab+tw 9->4; identity 0.07/s; b41 old 0.00/s; b41flow 0.00/s
VPS :8646 w1: estab+tw 4->4; identity 0.07/s; b41 old 0.00/s; b41flow 0.07/s
VPS :8650 w0: estab+tw 4->4; identity 0.07/s; b41 old 0.00/s; b41flow 0.07/s
VPS :8650 w1: estab+tw 4->4; identity 0.07/s; b41 old 0.00/s; b41flow 0.07/s
```

Asserted: for :8642 and :8091 old B41 <= 0.2/s, `|b41flow - identity| / identity <= 25 %` (worst: :8091 w0,
13 %). The sensor over the same three tables gives the same verdicts in all three modes:
cycle 0 baseline, cycles 1-2 `conn-rate-8642` warn (B41 4.3 thresholds), :8420 quiet.
Matches yesterday's cortex-s2-b sample (:8642 1.44/s identity vs 0.03/s B41).

## 2. Box: two release nerves, steady ~20/s load (`tests/real_connrate_fix.rs`)

```
GALION_REAL=1 cargo-slot test -j 2 --release -p openmind-nerve --test real_connrate_fix -- --nocapture
```

Run 2026-10-21 ~09:31-09:33 +02:00, `test result: ok. 1 passed` (120.17 s). :20480 default formula, :20482
`NERVE_CONNRATE_FORMULA=b41flow`, both `NERVE_CONNRATE_PORTS=20481`, interval 10 s; live service :20481; client
paced at one connection per 50 ms, held 75 s past the 60 s TIME-WAIT window before measuring:

```
t+80s  client 19.9/s | identity 20.2/s (202 new) error, 3 evidence lines | b41flow 19.2/s (192 new) error, old-b41 0.00/s
t+90s  client 19.9/s | identity 18.9/s (189 new) error                   | b41flow 19.4/s (195 new) error, old-b41 0.00/s
t+100s client 20.0/s | identity 18.0/s (180 new) error                   | b41flow 21.0/s (210 new) error, old-b41 1.40/s
load stopped after 2006 connections (120 s); cleared identity cycle 12, b41flow cycle 13; rss 4556 / 4724 KiB
```

- Steady load: the old B41 delta reads 0.00-1.40/s for a real 20/s; `b41flow` reads 19.2-21.0/s (within 6 % of
  the client's own count), identity 18.0-20.2/s. Both high (>= 10/s), both cleared after the load stopped.
- Default mode output unchanged (3 evidence lines); `b41flow` adds line 4
  `formula=b41flow identity_rate=20.20/s b41flow_rate=19.23/s (...)`.
- conn-rate sensor cost 4.0-5.6 ms per cycle on the box (large shared socket table), RSS < 5 MiB.

All test processes were killed and the temp dir removed by the test.
