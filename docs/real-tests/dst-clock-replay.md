<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 402 dst-clock-replay: F-383-8 diagnosis and fix (2026-10-28, Europe/Zagreb)

Base `swarm/real-ports-batch-3` 26a526c1, branch `swarm/dst-clock-replay`.
Ports 22720-22729. VPS read-only (GET `/state` over ssh), nothing deployed.

## Diagnosis

`real_dst_clock` VPS replay compared the cortex finding ids of the fold run
(2026-10-25 02:25+02:00 .. 02:35+01:00) with the control night
(2026-10-21 02:25 .. 03:35+02:00). The live nerve now has 2 open incidents
(`edge`, `galion-one`), and the cortex lists them under `cortex.incidents`
with their incident ids. An incident id is
`inc-<FNV-1a(cause, opened instant)>` (`incident::short_id`), so the same
cause opened on another night gets another id **by design**. When slot 157
wrote the test (2026-10-21) the live nerve had no open incident, so the
comparison held by accident.

Box replay on a read-only capture 16:40-16:41 +01:00 (3 x `/state`,
cycle 101, ts 16:30:01+01:00, 14 components, degraded, 2 open incidents),
base test:

```
fold:    score 90 findings {inc-55399539, inc-9a32fa93} causes {edge, galion-one}
control: score 90 causes {edge, galion-one}
panicked: findings differ at the fold
  left: {"inc-55399539", "inc-9a32fa93"}
 right: {"inc-219384bc", "inc-70209b86"}
```

Score, causes and timestamps were identical; only the episode ids differed.
So F-383-8 is a **test harness bug, not a DST bug** in the nerve.

## Fix

- `tests/real_dst_clock.rs`: cortex ids that are incident ids are compared
  as `incident:<cause>`; raw ids are checked instead by a stricter DST
  check per run: every incident opened exactly once on the first replayed
  cycle (no re-open in the repeated 02:00-03:00 hour), `cycles == 141`,
  `duration_s == 4200` (real elapsed seconds across the fold), and
  `id == short_id(cause, opened)`. The fold capture is replayed twice and
  must give identical raw ids/incidents (determinism).
- `src/incident.rs`: `short_id` made `pub` with a doc comment (no behaviour
  change) + unit test `short_id_names_one_episode_across_the_fold`
  (02:30+02:00 and 02:30+01:00 give different ids; same instant same id).

## Real tests

| when (+01:00) | run | result |
|---|---|---|
| 16:52 | base test, capture dir | FAIL (ids differ, above) |
| 16:53 | fixed test, capture dir | PASS 1/1 |
| 16:53:47-16:55:08 | `GALION_REAL=1 NERVE_DST_REAL_PORT_BASE=22720 NERVE_DST_SSH=root@<vps>` release, both tests | **PASS 2/2** (80.98 s) |
| 16:56 | negative control: fold stamps written naive (+01:00 -> +02:00) | FAIL as expected (ts 02:59:30+02:00, opened 02:00+02:00, duration 3570) |

Live run evidence (3 live snapshots ts 16:45:01+01:00, 14 components,
degraded, 2 open incidents):

```
serve on 127.0.0.1:22720: new self-evals self-eval-20261028T1653{47,52,57}+0100.json, 96 kept, score 100
fold:    ts 2026-10-25T02:35:00+01:00 score 90 findings {incident:edge, incident:galion-one}
         edge inc-9a32fa93 opened 02:25:00+02:00 cycles 141 duration_s 4200
         galion-one inc-55399539 opened 02:25:00+02:00 cycles 141 duration_s 4200
control: ts 2026-10-21T03:35:00+02:00 score 90 findings {incident:edge, incident:galion-one}
         edge inc-219384bc / galion-one inc-70209b86 opened 02:25:00+02:00 cycles 141 duration_s 4200
```

## Gates

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok.
- `env -u GALION_REAL cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`:
  **2034 passed / 0 failed / 18 ignored** (base 2033 + 1 new unit test).
- openmind-nerve release x86_64 musl 1,930,048 B and aarch64 musl 1,591,776 B build;
  aarch64-android and armv7-musleabihf `cargo check` ok.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 752 scanned, 0 missing.
- No new dependencies.

## Findings

- F-402-1: the shared box shell env carries an `OPENROUTER_API_KEY`
  variable (value not read); the nerve warns and ignores it. Real tests
  here ran with it unset (`env -u`).
- F-402-2: the live nerve self-reports nothing about DST; the fold behaviour
  is only proven by replay. The real fold night 2026-10-25 already passed on
  the live nerve (no live data was re-checked for it in this slot).

## Owner decisions

- D-402-1: accept the test-only fix (ids compared per cause, raw ids checked
  against `short_id(cause, opened)`), or (b) make incident ids cause-only
  (would merge separate episodes of one cause: not recommended).
