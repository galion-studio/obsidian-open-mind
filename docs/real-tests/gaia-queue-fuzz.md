<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-test record: gaia-queue-fuzz (wave 4, slot 221)

Branch `swarm/gaia-queue-fuzz`, base `swarm/poseidon-crash-soak` b71a909.
Box only, ports 127.0.0.1:20910-20919 (used: 20911 real test, 20912
negative-control probe, 20913 /stats shape check). All times
Europe/Zagreb (CEST, +02:00), 2026-10-21.

## What was fuzzed

| Target (`tests/fuzz_queue.rs`) | Input | Property |
| --- | --- | --- |
| `fuzz_log_line` | mutated `queue.log` lines, raw and with the CRC recomputed | no panic; a decoded record re-encodes to itself |
| `fuzz_store_open` | whole mutated log files | `Store::open` equals an independent oracle (records, cut bytes, file length, refuse-on-mid-file-damage); the store still appends and reopens; replay does not panic |
| `fuzz_replay_hostile` | CRC-valid hostile records (duplicate ids, u64::MAX fields, unknown ids) | `apply` and every server read (`depth`, VRAM summary, `lease`, `expire_due`, enqueue, Gaia load) never panic; compaction of the result is a fixed point |
| `fuzz_job_body` | mutated `POST /jobs` JSON, plus lease/ack/nack/extend/expire/redrive with caps | hard stop (spent + reserved <= cap) holds, depth sums to the job count, agent reservations sum to global, compaction rebuilds the same budgets and agents |
| `fuzz_sidecar` | real and crafted `refusals.dat`, mutated | open never fails or panics; the next save is what the next open restores |
| `fuzz_http_request` | mutated requests | `read_request` never panics; Content-Length is digits only, duplicates agree, body length matches; `parse_response`, `split_url`, `param` never panic |

The auth header parser of slot 205 was skipped: `swarm/poseidon-auth`
d10cb09 does not merge cleanly onto b71a909 (`git merge-tree`: content
conflict in `docs/poseidon.md`; code auto-merges), and the row says
"if merged cleanly, else skip".

## Finds (7), all fixed, regression tests in `tests/fuzz_finds.rs`

First fixed-seed run 22:59 found 5 targets failing; timed runs 23:02 and
23:05 found 2 more (budget saturation at cap u64::MAX, reservation sums
with costs near u64::MAX). See `docs/poseidon.md` "Fuzzing" for the
detail. Summary:

1. duplicate job id in the log -> `POST /lease` panic (release: SIGABRT)
   and `/stats` `dead` = 18446744073709551615;
2. id/token/counter `+ 1` overflow in replay;
3. `cost` near u64::MAX overflowed reservation sums (now `MAX_COST` 2^40);
4. `Budget::fits` saturated sum passed at `cap = u64::MAX`;
5. VRAM summary `cents_per_hour` sum overflow (`GET /stats`);
6. `refusals.dat` slot at seq u64::MAX stopped persistence (now rebased);
7. conflicting / signed `Content-Length` accepted.

Negative control 23:07: `tests/fuzz_finds.rs` against b71a909 source:
9 of 10 fail (the 10th is a door sanity check that already held).

## Fuzz runs (debug build, box)

| When | Mode | Result |
| --- | --- | --- |
| 23:17:51-23:25:51 | `GALION_FUZZ_SECS=240`, 6 targets, 3 threads | 6/6 pass, no finds |
| 23:33:35-23:37:36 | `GALION_FUZZ_SECS=120 --nocapture` | 6/6 pass: http_request 21,948,005 cases; log_line 1,855,859; job_body 1,351,113; replay_hostile 741,182; sidecar 705,582; store_open 166,102; no finds |

(A 23:10-23:14 timed run reported the old finds again: the negative-
control build of b71a909 had shared this target directory and replaced
the library artefacts. Rebuilt from this branch and re-run at 23:17;
that run is the one recorded above.)

## Real binary (`GALION_REAL=1 tests/real_queue_fuzz.rs`, 127.0.0.1:20911)

Release profile (`panic = "abort"`, so any reachable panic kills the
process).

- 23:09:35, glibc release, 3,000 cases: codes 200 863, 201 161, 400
  1686, 404 51, 405 28, 409 195, 429 16; 0 closed without answer; the
  finds over the wire: conflicting Content-Length 400, `+2` 400, cost
  u64::MAX 400, two devices at cents_per_hour u64::MAX ->
  `planned_cents_per_hour` 18446744073709551615 (saturated); SIGKILL +
  restart: jobs/enqueued/acked equal (161/161/0), 476 records replayed;
  hostile log: ready 1 (duplicate ignored), lease + ack + lease ok,
  sidecar restored 7 at seq u64::MAX, rebased, a refusal after a SIGKILL
  restored as 8 (seq 3). PASS.
- 23:09:49, glibc release, 50,000 cases in 15,950 ms: 200 14150, 201
  1661, 400 28308, 404 831, 405 449, 409 3564, 429 1037; 0 without
  answer; SIGKILL replay 1661/1661/0, 7,290 records; hostile part as
  above. PASS.
- 23:32:02, x86_64 musl static-pie (889,632 B, sha256 17715308...), 10,000
  cases in 1,943 ms; all parts PASS.
- 23:32:16, aarch64 musl static (758,880 B, sha256 cc6296c7..., linker
  rust-lld) under `POSEIDON_RUNNER=qemu-aarch64`, 10,000 cases in
  16,743 ms; all parts PASS.

Negative control, base b71a909 release binary (separate target dir,
removed after):

- 23:10:24 `real_queue_fuzz`: wire fuzz survives, then "conflicting
  Content-Length" answered **201** (expected 400): `Content-Length: 15`
  then `Content-Length: 2` took the last value.
- 23:10:24 `REAL_FUZZ_PARTS=hostile`: `/stats` depth
  `{"dead":18446744073709551615,"ready":2}` for the duplicate-id log.
- 23:10:40 manual probe on 20912 (same hostile log): lease 200, ack 200,
  second lease -> curl exit 52 (empty reply), server **exit 134
  (SIGABRT)**, stderr `panicked at crates/openmind-queue/src/engine.rs:555:31:
  no entry found for key`.

## Regression (existing suites, this branch)

- `cargo test -p openmind-queue`: 70 passed, 0 failed.
- Workspace (`--exclude openmind-sandbox --exclude cgroup-detect
  --exclude hermes-tools`, sandbox_vps_only) 23:27:51-23:30:40: 1276
  passed, 0 failed, 6 ignored.
- `GALION_REAL=1` real_binary 1/1, real_refusal_persist 2/2,
  real_crash_soak 5/5 (23:38:53-23:39:53, `--test-threads 1`). A first
  23:37:55 run with the five soak tests in parallel failed once on
  `real_kill9_loop_compaction_hammer`'s fence-check ratio
  (`fence_checks * 4 >= kills`, timing under load); it passed alone at
  23:38:30 and in the serial run. Not caused by this branch (no fencing
  code changed); noted for slot 206's owner.
- `cargo fmt --check` ok; `cargo clippy --workspace --all-targets -- -D
  warnings` ok (only the known vendor candle-core deprecation warnings);
  `scripts/no-python.sh` ok; aarch64-linux-android and
  armv7-unknown-linux-musleabihf `cargo check -p openmind-queue` ok.
- repo-check: 10 errors, all pre-existing `vendor/candle-core` portable
  findings, same count with and without this branch's changes (slot 211
  handles them); no new error.

## VPS (read-only, 23:11 and 23:11:27)

`ssh slavik-vps`, GET only, nothing written:

- `openmind-queue.service` is live: `/usr/local/bin/openmind-queue serve
  --bind 127.0.0.1:19120 --data /var/lib/openmind-queue`, user
  `openmind-queue`, active since 08:27:54, binary 889,632 B, sha256
  8a7f7faa... `/healthz` ok, version 0.1.0. `/stats`: depth all 0,
  enqueued 1, acked 1, quota_refusals 3, budget_refusals 1, sidecar
  restored seq 4, bad_slots 0, 7 log lines.
- That binary predates these fixes, so finds 1-7 apply to it. It binds
  loopback only, so only local clients reach it; finds 1, 2 and 6 need a
  tampered data directory. Not changed (no live changes in this row).
- `/stats` key set of this branch's binary vs the live one: no key added
  or removed by this branch (differences are per-agent keys that appear
  only with agents, and `store.compaction_errors` from slot 206).
- Nerve 127.0.0.1:8670 `/state`: score 56, grade D, cycle 5; recent
  alerts from hermes-rs (slow request) and gaia-cortex. Cortex is live
  (`gaia-cortex.service`, "event-driven, dry-run actions").

## Owner decisions (Slavik)

- D-221-1: `MAX_COST` = 2^40 spend units per attempt (400 above it). A
  behaviour change for clients that send larger costs.
- D-221-2: a second log record for a live job id is ignored (first wins,
  start-up continues). Alternative: refuse to start, like a corrupt line
  in the middle of the log.
- D-221-3: a log job owned by agent `*` (the door refuses it) aliases the
  global scope in compaction; left as is, the fuzzer does not generate it.
  Alternative: drop or refuse such records at replay.
- D-221-4: `next_token` saturates at u64::MAX (only via a tampered log),
  after which tokens repeat and fencing weakens; left as is.
- D-221-5: redeploy the live VPS queue with these fixes (needs a
  cutover; not done here).
- D-221-6: fuzz the slot 205 auth header parser once `swarm/poseidon-auth`
  merges cleanly with this line.
