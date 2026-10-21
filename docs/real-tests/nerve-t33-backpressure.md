<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Nerve T3.3(b) back-pressure: real-life box test (wave-4 slot 187)

Branch `swarm/nerve-t33-backpressure`, base 334beaa (the nerve-client in the
aa77864 builds live on the VPS since 2026-10-21 09:47 CEST +02:00). Box only;
the VPS was only read (GETs, `systemctl show`). Ports 20570-20579.

Question: with `NERVE_URL` live in hermes-rs and openmind-memory, can a dead,
slow, 429-returning or misbehaving nerve slow down or block their requests,
and is every signal that does not arrive counted?

## Found and fixed: a trickling nerve held the push worker 16 s per signal

nerve-client had 300 ms timeouts per socket call only. A nerve that writes
its reply one byte at a time (stuck or overloaded peer, half-dead proxy) reset
the read timeout on every byte: one POST held the single worker for up to 64
reads. Measured with the 334beaa binaries: **16 010 ms** for one signal, the
next 200 signals of each service sat in the queue meanwhile (`unresolved 200`
in the heartbeat; with more traffic they would have overflowed into drops),
and the POST ended as `rejected` (status 0), not `failed`, so no back-off.
Requests themselves were never slowed (the queue is non-blocking).

Fix (`crates/nerve-client/src/lib.rs`): `POST_DEADLINE` = 1 s for the whole
POST (connect + write + status line); each socket timeout is
`min(300 ms, time left)`. Same binaries rebuilt: the fake nerve saw the
client hang up after **1 500 ms** (one 250 ms beat after the 1 s deadline),
the POST counts as `failed` and starts the normal 2 s back-off.
Unit tests: `trickling_nerve_is_cut_at_the_post_deadline` (fails in 15.76 s
with the old behaviour, simulated by a 60 s deadline; passes now) and
`post_within_honours_a_short_deadline`.

## Box test `nerve_backpressure_box` (2026-10-21 12:49-12:52 CEST +02:00)

`GALION_REAL=1 T33_BIN_DIR=<release> cargo test -p openmind-nerve --test real_backpressure nerve_backpressure_box -- --nocapture --test-threads 1`
-> `test result: ok. 1 passed` (151.5 s). Release binaries of hermes-gateway
(`--mock`, offline, no spend), openmind-memory and openmind-nerve. A = same
binary with `NERVE_URL` unset, B = `NERVE_URL` to the in-test fake nerve,
defaults otherwise (queue 256, like the live drop-ins). 400 requests per
stream per phase, A/B interleaved in 8 blocks; half the B requests push one
error signal (hermes `mock-fail` 502, memory `POST /seed` 501).

Per-phase client counter deltas (from the first heartbeat after the phase;
`queued + dropped` = pushed errors + 2 heartbeat signals; `unresolved` =
queued - sent - failed - rejected was 0 after every phase):

| phase | source | pushed errors | queued | sent | dropped | failed | rejected |
|---|---|---|---|---|---|---|---|
| ok | hermes-rs | 197 | 199 | 199 | 0 | 0 | 0 |
| ok | openmind-memory | 200 | 202 | 202 | 0 | 0 | 0 |
| dead (port closed) | hermes-rs | 198 | 200 | 2 | 0 | 198 | 0 |
| dead | openmind-memory | 200 | 202 | 2 | 0 | 200 | 0 |
| slow (202 after 5 s) | hermes-rs | 200 | 202 | 2 | 0 | 200 | 0 |
| slow | openmind-memory | 200 | 202 | 2 | 0 | 200 | 0 |
| 429 | hermes-rs | 199 | 201 | 2 | 0 | 0 | 199 |
| 429 | openmind-memory | 200 | 202 | 2 | 0 | 0 | 200 |
| trickle (1 B / 250 ms) | hermes-rs | 200 | 202 | 2 | 0 | 200 | 0 |
| trickle | openmind-memory | 200 | 202 | 2 | 0 | 200 | 0 |
| blackhole (accept, never read) | hermes-rs | 198 | 200 | 2 | 0 | 198 | 0 |
| blackhole | openmind-memory | 200 | 202 | 2 | 0 | 200 | 0 |
| ok again | hermes-rs | 200 | 202 | 202 | 0 | 0 | 0 |
| ok again | openmind-memory | 200 | 202 | 202 | 0 | 0 | 0 |
| burst (blackhole, 4 000 x /seed) | openmind-memory | 4000 | 259 | 2 | **3743** | 257 | 0 |

Burst: 4 000 signals in ~0.6 s against a nerve that never answers: 256 fit
the queue, 3 743 were dropped, and every one is counted
(259 + 3 743 = 4 000 + 2 heartbeat signals). Fake-side cross-check: in the ok
phases the fake accepted exactly `sent` (199/202/202/202).

Request latency, ms p50 / p95 / p99 / max (A = no nerve, B = nerve in that mode):

| phase | hermes A | hermes B | memory A | memory B |
|---|---|---|---|---|
| ok | 3.48/6.55/47.82/56.68 | 3.51/5.86/115.10/127.82 | 0.14/0.37/0.63/0.96 | 0.14/0.37/0.60/0.79 |
| dead | 3.71/13.54/21.49/22.86 | 3.44/6.14/9.72/11.24 | 0.14/0.33/0.49/0.58 | 0.13/0.36/0.60/0.80 |
| slow | 3.29/7.91/14.19/55.30 | 3.83/8.96/12.89/17.75 | 0.17/0.39/0.74/0.83 | 0.20/0.85/2.78/3.83 |
| 429 | 3.44/9.30/69.08/77.55 | 3.24/6.00/8.95/13.27 | 0.15/0.35/0.49/0.57 | 0.17/0.38/0.54/2.29 |
| trickle | 3.92/13.69/26.86/37.85 | 3.79/7.69/11.25/14.54 | 0.20/0.64/1.47/141.47 | 0.16/0.43/0.89/3.35 |
| blackhole | 3.52/7.59/21.59/28.43 | 3.49/7.46/37.96/57.11 | 0.13/0.31/0.45/0.54 | 0.14/0.37/0.56/0.71 |
| ok again | 3.23/9.20/69.49/72.10 | 3.68/10.82/19.80/25.31 | 0.14/0.34/0.52/0.63 | 0.15/0.37/0.52/0.64 |
| burst | | | 0.08/0.17/0.43/2.45 | 0.07/0.13/0.40/1.61 |

No B request waited for the nerve (max B 127.8 ms against a nerve that
answers after 5 s or never); p99 outliers go both ways (A worse in 5 of 7
phases), i.e. shared-box noise (~20 agents), not the client. Every request
was answered (hermes codes 200/502 plus a few 429 from its own
`max_concurrent_runs` under 4-way load, memory 200/501).

### Real nerve 429 (per-source limit 120/min)

Same load against R services pointing at a real `openmind-nerve serve`:

- hermes-rs: pushed 199, client queued 212, **sent 120, rejected 92**; nerve
  reports `dropped` (rate-limited) **92**: both sides agree exactly.
- openmind-memory: sent 120, rejected 93; nerve rate-limited 93.
- The first heartbeat got through **61 s** after the flood started: the
  heartbeat shares the 120/min budget, so during a signal flood the nerve
  is blind to the client counters for up to a minute (owner decision D-T33-2).
- Real nerve after the phase: cycle 13, RSS 6 640 kB; R hermes RSS 6 500 kB,
  4 threads; R memory RSS 4 508 kB, 3 threads (A: 2 / 1 threads; the client
  adds exactly 2: `nerve-push`, `nerve-heartbeat`).

### Same test on the 334beaa binaries (`T33_EXPECT_DEADLINE=0`, 12:51-12:54)

All phases pass the same way except trickle: client hold **16 010 ms**,
heartbeat after the phase showed `unresolved 200` per service (worker stuck,
backlog queued). Log line: `-- longest client hold on a trickling nerve: 16010 ms`.

## Soak `nerve_backpressure_soak`, 20 min (2026-10-21 12:57-13:17 CEST +02:00)

`T33_SOAK_MIN=20`; the fake nerve cycles ok / dead / slow / 429 / trickle /
blackhole every 60 s (each mode ~3 times); 2 threads x 4 ports at ~20 req/s
each (half the B requests push a signal); a real nerve probes the B services
every 5 s. `test result: ok. 1 passed` (1 210 s). 183 410 requests answered,
codes only 200/502 (hermes) and 200/501 (memory) on A and B.

| mode | hermes A p50/p95/p99/max | hermes B | memory A | memory B |
|---|---|---|---|---|
| ok | 2.89/6.79/29.55/296.58 | 2.87/6.91/30.33/296.71 | 0.38/0.76/0.92/68.23 | 0.39/0.75/0.91/22.48 |
| dead | 3.00/7.05/29.73/77.39 | 3.00/7.06/28.71/76.98 | 0.41/0.77/0.92/67.31 | 0.41/0.77/0.93/126.56 |
| slow | 2.93/7.99/42.77/108.95 | 2.94/7.81/41.56/109.87 | 0.37/0.76/0.94/259.51 | 0.37/0.76/0.96/9.66 |
| 429 | 2.98/6.47/16.96/359.71 | 2.96/6.50/18.15/360.11 | 0.40/0.77/0.93/275.50 | 0.41/0.76/0.93/80.03 |
| trickle | 2.99/6.61/21.61/114.02 | 3.00/6.56/22.37/114.11 | 0.41/0.76/0.89/15.22 | 0.41/0.75/0.89/16.45 |
| blackhole | 2.92/6.90/30.02/125.44 | 2.92/6.96/32.97/125.88 | 0.40/0.75/0.90/8.69 | 0.40/0.76/0.93/53.81 |

Client counters at the end (heartbeat after switching back to ok):

- hermes-rs: queued 22 517, sent 4 437, dropped 0, failed 14 777, rejected 3 303, unresolved 0.
- openmind-memory: queued 23 972, sent 4 717, dropped 0, failed 15 734, rejected 3 521, unresolved 0.

Fake nerve connections per mode: ok 9 159, 429 6 824, slow 156, blackhole
156, trickle 120. A dead/slow/blackhole/trickle nerve cost about one
connection per 2-3 s per service (back-off works); a 429 nerve gets one
connection per signal (no back-off on non-2xx; D-T33-3).

Resources, warm-up (2 min) -> end:

| process | RSS kB | threads | fds |
|---|---|---|---|
| hermes A (no nerve) | 30 400 -> 196 084 (+165 684) | 2 -> 2 | 7 -> 7 |
| hermes B (nerve) | 30 564 -> 196 196 (+165 632) | 4 -> 4 | 7 -> 7 |
| memory A | 4 196 -> 4 200 (+4) | 1 -> 1 | 9 -> 9 |
| memory B | 4 236 -> 4 240 (+4) | 3 -> 3 | 9 -> 9 |
| real nerve (cycles 7 -> 240) | 4 528 -> 6 220 | | |

nerve-client adds no RSS growth (B minus A: hermes -52 kB, memory 0 kB),
exactly 2 threads and no fds. **Separate finding:** hermes-gateway itself
grows ~8.3 MB/min under this load (~7.5 kB per chat, linear, identical with
and without `NERVE_URL`; ~22 000 chats per process). Not caused by the
nerve; not investigated further in this slot (see open issues).

## VPS (read-only, 2026-10-21 12:57 CEST +02:00)

`GET 127.0.0.1:8670/health`: status ok, cycle 388, system degraded, score 61.
`GET /api/v1/signals` (heartbeat messages only):

- hermes-rs: `queued=954 sent=954 dropped=0 failed=0 rejected=0`, RSS 3 216 kB;
  pushes a `spend_cap` event about every 20 s ("session spend cap reached
  ($1.0014 of $1.00)", the keep-warm 402s of slot 181), ~3 signals/min,
  far under the 120/min limit.
- openmind-memory: `queued=385 sent=385 dropped=0 failed=0 rejected=0`, RSS 3 412 kB.
- `systemctl show`: galion-nerve, hermes-rs, openmind-memory active since
  09:43-09:46 CEST, NRestarts=0, MemoryCurrent 6.0 / 3.6 / 2.7 MB.

Live has lost nothing so far. Cortex: not live as its own service; observed
through the nerve as above.
