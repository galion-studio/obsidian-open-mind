<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: POSEIDON crash / soak suite (slot 206, Gaia S4.4)

Branch `swarm/poseidon-crash-soak`, base integration-5 0d68f46, plus a merge
of `swarm/poseidon-refusal-persist` eec2d99 (slot 161's `refusals.dat`
sidecar, the build live on the VPS since 2026-10-21 08:29 +02:00). Box only;
loopback ports 20760-20769 (slot band). Nothing was run, copied or changed
on the VPS; only read-only GETs (below).

## What runs

`crates/openmind-queue/tests/real_crash_soak.rs`, gated `GALION_REAL=1`, runs
the real release binary (`POSEIDON_BIN`, default
`$CARGO_TARGET_DIR/release/openmind-queue`). Each test starts, probes, kills
and cleans up its own servers (a `Drop` SIGKILLs any server a failed
assertion leaves; a busy port fails the start instead of reusing a stray).

| Test | Port(s) | What it does |
|---|---|---|
| `real_kill9_loop_during_ack` | 20761 | 4 producers (agents `spend` cost 1, `tight` cost 5 under cap 400, `qq` quota max_pending 5), 4 workers (lease `wait_ms 0`, 1 in 10 a deliberate stale ack, then the real ack), `--max-depth 60` (backpressure), `--compact-min 300` (compaction under load). SIGKILL every 150-650 ms (random), restart on the same data dir, clients parked, then verify. |
| `real_kill9_loop_compaction_hammer` | 20762 | Same with `--compact-min 100` and a thread POSTing `/admin/compact` every 25 ms, so kills land inside compactions. |
| `real_torn_tail` | 20763, 20764 | A real 242-line log (caps, quotas, 120 enqueues, 45 acks, 15 nacks) cut at 30 random offsets, inside each of the last 5 lines, without its last newline, plus garbage, a 4 KiB zero block, half JSON and a complete line with a bad CRC. Each copy must start, replay exactly the complete valid lines (checked against the library replay), take a new job and replay cleanly again. A flipped CRC digit in a middle line must refuse the start and leave the log byte-identical. |
| `real_flock_two_servers` | 20765-20767 | 8 rival servers on the same data dir at once. |
| `real_soak_10k` | 20768 | Rounds of 10,000 enqueues (4 producers, 64-byte payloads, 8 agents) then drain (8 workers, lease + ack), RSS sampled every 250 ms (`/proc/<pid>/status`), one SIGKILL at 50 % of round 2's drain with a full no-loss check. `SOAK_ROUNDS` (default 3). |

Checks after every restart in the kill loops (clients parked, unanswered
requests classified: connection refused = never sent, anything else =
ambiguous and resolved after the restart by `GET /jobs/{id}`):

1. No lost job: every enqueue answered 201 and not known acked is live.
2. No resurrected ack: every job acked since the last check is 404.
3. `stats.acked` == acks known applied (exact); `enqueued - acked` == live.
4. Spend: per agent == sum of the charges of applied acks (exact); global ==
   sum of agents; `tight` cap still 400 and `spent + reserved <= 400`;
   `qq` quota still `[5, 2]`.
5. Refusal counters (budget, quota, backpressure, stale) never go backwards
   across a restart and lie between what clients saw and that plus the
   unanswered requests.
6. The restarted binary's stats == an independent library replay of the
   `queue.log` + `refusals.dat` the killed process left (per-field max, as
   the server restores).
7. Token fencing: a fresh lease token is above every token handed out
   before the kill.

## Results (box, 2026-10-21, Zagreb +02:00)

Final code (HEAD of this branch), glibc release build, full suite
20:03:53-20:05:49, `SOAK_KILLS=100`, `--test-threads 1`: **5/5 pass**
(log `logs/poseidon-crash-soak/FINAL-glibc-suite.log`).

| Run | Kills | Ops | Enq 201 | Acked | Unanswered acks (applied) | Compactions (errors) | Restart max / avg | Lost | Resurrected |
|---|---|---|---|---|---|---|---|---|---|
| kill9, glibc | 100 in 50.1 s | 162,103 | 29,784 | 29,737 | 40 (7) | 352 (0) | 26 / 12 ms | 0 | 0 |
| compaction hammer, glibc | 100 in 46.9 s | 137,152 | 20,025 | 19,971 | 40 (12) | 1,527 (0) | 29 / 12 ms | 0 | 0 |
| kill9, static musl | 40 in 20.4 s | 59,706 | 11,082 | 11,059 | 18 (5) | 124 (0) | 26 / 13 ms | 0 | 0 |
| compaction hammer, static musl | 40 in 20.0 s | 69,604 | 14,218 | 14,184 | 26 (6) | 672 (0) | 26 / 14 ms | 0 | 0 |

Refusal counters at the end of the glibc kill9 run (server vs clients saw):
budget 38,739 vs 38,710, quota 25,465 vs 25,465, backpressure 5,534 vs
5,534, stale 2,959 vs 2,959 (budget includes the fence-probe leases).
truncated_bytes 0 in every restart (SIGKILL never tears a line), server
stderr empty, token fencing checked on 96/100 and 77/100 restarts (the rest
had nothing leasable and a full queue).

Torn tail: 44/44 cases replayed exactly and kept working (start 25 ms);
mid-log corruption refused (exit 2, "refusing to start"), log untouched.

Flock: 8/8 rivals refused ("in use by another process", exit 2), none bound
20766/20767, log bytes unchanged, first server unharmed; after its SIGKILL
a new server started in 7-25 ms with 21/21 jobs.

Soak, glibc, 3 rounds (20:05:31-20:05:47): every round 10,000 enqueued in
1.0-1.1 s, drained in 2.4-5.3 s. SIGKILL at 5,190 acks of round 2: log
2,919,558 B, 9,094 records replayed in 72 ms, 4,809 live, 0 lost, 0
resurrected, 3 acks unanswered (1 applied), RSS 18,208 KiB before the kill,
9,788 KiB after replay. RSS with 10k live: r1 9,392 / r3 16,880 KiB;
drained r1 12,556 / r3 19,720 KiB (passes the 1.5x + 4 MiB bound).

Soak, 10 rounds = 100,000 jobs:

| Build | RSS 10k live r1 / r10 | RSS drained r1 / r10 | Peak | Verdict |
|---|---|---|---|---|
| static musl x86_64 (deploy target) | 7,024 / 7,144 KiB | 5,156 / 5,512 KiB | 12,876 KiB | flat, PASS |
| glibc (box default) | 9,392 / 52,996 KiB | 12,076 / 54,532 KiB | 54,532 KiB | grows ~5 MB/round, FAIL |
| glibc, `MALLOC_ARENA_MAX=2` | 9,372 / 21,968 KiB | 11,272 / 21,976 KiB | 22,728 KiB | plateau from round 4 (~22 MB), fails the 1.5x ratio only |

Is the glibc growth a leak? `tests/heap_rounds.rs` (`POSEIDON_HEAP=1`,
release, counting global allocator, same server in-process, 6 rounds of
10,000): live heap drained 4,362,914 B in round 1 and 4,364,398 ->
4,369,950 B in rounds 2-6 = 1,388 B per round (0.14 B per job) while the
process RSS went 12.9 -> 44.1 MB. Live data is flat; the RSS is memory glibc
keeps in per-thread arenas (one short-lived thread per connection). The
musl build does not show it.

## Fixes found by the suite

1. HTTP client accepted a truncated body: a server SIGKILLed between head
   and body left "201 Created" with an empty body, parsed as success with
   no id (first kill-loop run, 19:49). `http::parse_response` now returns
   `UnexpectedEof` when the body is shorter than `Content-Length`;
   `write_response` sends head and body in one write. Unit test
   `truncated_body_is_an_error_not_an_answer`.
2. A failed compaction turned a synced change into a 500. On base 0d68f46
   the new `crash_counters::failed_compaction_does_not_fail_a_durable_change`
   gets 500 for an ack that is in the log; now 200/201, logged, counted in
   `store.compaction_errors`, `/admin/compact` still answers 500.
3. `Store::compact` reopened the log path after the rename; a failure there
   left the old handle writing to the unlinked file (silent loss of every
   later append). The snapshot's own read/write handle is now the log
   handle (no step after the rename can fail).
4. Refusal counters reset on restart on base 0d68f46 (control run 19:54:59:
   "budget refusals: server 104, clients saw 142" after the first kill;
   `crash_counters` on base: all five counters back to 0). Fixed by merging
   slot 161's sidecar eec2d99 (already live). An own `Record::Tally` log
   record was written first and then dropped for the sidecar, which is
   rollback-safe (an old binary ignores `refusals.dat`; it would refuse a
   log with unknown records).

## Control runs on base 0d68f46 (static source copy, same tests)

- `real_kill9_loop_during_ack`: FAIL after the first kill (refusal counter
  went from 142 seen to 104 on the server).
- `real_torn_tail`, `real_flock_two_servers`: PASS (those properties held).
- `crash_counters::refusal_counters_...`: FAIL (all refusal counters 0
  after restart); `failed_compaction_...`: FAIL (500 for a synced ack).

## Live VPS (read-only GETs, 2026-10-21 20:00:59 +02:00)

`ssh slavik-vps`: `GET 127.0.0.1:19120/healthz` 200 (`openmind-queue`
0.1.0, POSEIDON); `GET /stats`: `refusals_sidecar` restored true, seq 4,
`store` lines 7, compactions 0, truncated_bytes 0; 2 agents
(`deploy-check`, `deploy-check-b`), depth all 0, quota_refusals 3,
budget_refusals 1; unit `openmind-queue.service` active since 08:27:54 +02:00,
listener 127.0.0.1:19120 only. The live build is eec2d99, so it has the
sidecar but not fixes 1-3 (its `/stats` has no `store.compaction_errors`).
Nerve `127.0.0.1:8670/state` answered (alerts: cloudflared request-flood
warn incident, galion-one mouth 503); the cortex is deployed in dry-run;
nothing in this slot touched either.

## Gates

See the slot 206 DONE line in /workspace/swarm/CALL-RECORD.md for fmt,
clippy, workspace tests, no-python, SPDX, repo-check and cross builds.
