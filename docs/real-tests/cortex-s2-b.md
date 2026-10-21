<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: cortex-s2-b (T2.5 `drift`, T2.7 `conn-rate`)

- Branch `swarm/cortex-s2-b`, base `swarm/cortex-sensor` e536446. Slot 56, ports 19560-19569.
- Code: `crates/openmind-nerve/src/sensors/drift.rs`, `sensors/connrate.rs`, two registry lines in `sensors/mod.rs`.
- Test: `crates/openmind-nerve/tests/real_cortex_s2b.rs` (Rust builds, starts, probes and tears down; no script).

## Command

```
GALION_REAL=1 CONNRATE_VPS_DIR=<dir with two VPS reads> CONNRATE_VPS_DT=32 \
  nice -n 10 cargo test -j 2 --release -p openmind-nerve --test real_cortex_s2b -- --nocapture
```

Run 2026-10-20T23:19:25+02:00 to 2026-10-20T23:20:20+02:00 on the box: `test result: ok. 1 passed` (30.08 s).

## Box: release `openmind-nerve serve` on 127.0.0.1:19560, interval 5 s

Env: `NERVE_DRIFT_MANIFEST=<tmp>/SHA256SUMS` (lists the real nerve binary by absolute path + `s2b-svc`, a deployed copy
under `NERVE_DRIFT_ROOT`), `NERVE_CONNRATE_PORTS=19561`, live service on :19561 (component `s2b-svc`).

```
serve :19560 cycle 3 clean (binary 1.0 MiB hashed at 2 MiB/cycle): listeners=3us scanners=1us edge=0us components=1us ddos=15us drift=78us conn-rate=1661us; cycle_ms 24; rss 3300 KiB
cycle 4: patched -> event {"ts":"2026-10-20T23:20:05+02:00","source":"drift","kind":"vulnerability","severity":"high","data":{"findings":1,"ids":["drift-s2b-svc"],"top":{"cause":"drift","component":"s2b-svc","evidence_n":3,"fix":"redeploy the released build (owner-approved cutover), or regenerate the manifest if this build is the intended release","id":"drift-s2b-svc","title":"s2b-svc: deployed file differs from the release manifest"}}}
cycle 5: restored -> drift cleared
cycle 6: 484 connections made in 5.0 s -> "port 19561: 95.8 new connections/s over 5 s (479 new)" (crit); event {"ts":"2026-10-20T23:20:15+02:00","source":"s2b-svc","kind":"bleeding","severity":"crit","data":{"findings":1,"ids":["conn-rate-19561"],"top":{"cause":"conn-rate:19561","component":"s2b-svc","evidence_n":3,...,"id":"conn-rate-19561","title":"port 19561: 95.8 new connections/s over 5 s (479 new)"}}}; ... drift=37us conn-rate=1974us
cycle 7: load stopped -> conn-rate cleared; cycle_ms 26; rss 3544 KiB
```

- The sensors appear (and clear) as designed; `cortex_sensors` order is the 5 built-ins then `drift`, `conn-rate`.
- Events carry ids/counts only; the test asserts no temp path is in the drift event.
- Budget: drift 37-78 us per cycle once hashed (one `stat` per entry); conn-rate 1.7-2.0 ms (the box's socket table
  is large: ~21 agents). Cycle 24-26 ms, RSS 3.3-3.5 MiB (< 50 ms / 8 MiB budget).
- Measured: 484 connections made, 479 counted as new (the other 5 were most likely made after the last table read of that cycle).

## VPS: read-only comparison (two `/proc/net/tcp*` reads)

`ssh root@VPS 'cat /proc/net/tcp /proc/net/tcp6'` at 2026-10-20T23:18:43+02:00 and ~32 s later (read-only, stdout only,
kept on the box under the slot target dir and deleted after the run; addresses never committed). Fed through
`connrate::parse_tables`:

```
VPS :8420: estab 0->0 time_wait 6->6; new 3 = 0.09/s; B41 growth 0.00/s
VPS :8642: estab 0->0 time_wait 85->86; new 46 = 1.44/s; B41 growth 0.03/s
VPS :8646: estab 0->0 time_wait 2->3; new 2 = 0.06/s; B41 growth 0.03/s
VPS :8650: estab 0->0 time_wait 2->3; new 2 = 0.06/s; B41 growth 0.03/s
VPS :8091: estab 0->0 time_wait 63->64; new 34 = 1.06/s; B41 growth 0.03/s
VPS :8670: estab 0->0 time_wait 0->2; new 2 = 0.06/s; B41 growth 0.06/s
```

Findings:
1. B41's "ESTAB + TIME-WAIT growth" says ~0/s on every port; the identity count shows :8642 (hermes-rs) at 1.44/s and
   :8091 (galion-one) at 1.06/s. Under a steady load the TIME-WAIT pool stays flat, so the B41 formula reads 0 for any
   constant rate. The sensor counts new connection identities and keeps the B41 growth only as evidence.
2. memory-gateway :8420 is quiet (0.09/s), consistent with B41 section 5 (the old ~67/s is gone).
3. With the B41 memory-gateway thresholds (warn >= 1/s) applied to all ports, :8642 and :8091 would warn. That is why
   the sensor only watches ports listed in `NERVE_CONNRATE_PORTS` (owner decision, see the branch report).

No VPS service, unit or file was changed; nothing was built or written on the VPS; `/opt/galion/openmind` was not read.
