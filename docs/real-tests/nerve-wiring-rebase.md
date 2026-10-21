<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: swarm/nerve-wiring-rebase (wave-4 slot 155)

Branch `swarm/nerve-wiring-rebase` = integration-4 `42de672` + nerve-wiring-2
`d97dab9` (merge commit, no rebase of published history). Box ports
20250-20259. All times Europe/Zagreb (CEST, +02:00).

## The 7 conflicting files and how they were resolved

| file | resolution |
|------|------------|
| `Cargo.lock` | both path deps kept: `galion-common` (integration-4) + `nerve-client` (nerve-wiring) on `hermes-rs` and `openmind-memory` |
| `crates/hermes-gateway/src/main.rs` | integration-4 base + nerve-wiring's NERVE doc, `--mock` test knobs (`mock-slow`, `mock-fail`, `HERMES_RS_MOCK_DELAY_MS`) |
| `crates/hermes-rs/src/http.rs` | integration-4's galion-common accept loop kept (strict reads, env limits); nerve-wiring's hand-rolled loop dropped; its conn-cap push re-added through a new `galion_common::http::BusyHook` (`serve_connections_with`) |
| `crates/openmind-memory/src/http.rs` | same as hermes-rs |
| `crates/openmind-nerve/src/cortex.rs`, `engine.rs`, `lib.rs` | integration-4's `Sensor` trait split kept; nerve-wiring's owner map / Gaia owners threaded into `cortex::Input` (`owners::EMPTY`, `owners::NO_PROCS` defaults) and into the `listeners` sensor (`sensors/listeners.rs`) |

`galion-common` change: `serve_connections` now calls
`serve_connections_with(.., None)`; the hook runs on the accept thread before
the 503 is written. `nerve_client::cap_hit` is rate-limited and only queues
(bounded channel), so it never blocks the accept loop. Unit test
`busy_hook_runs_on_each_cap_refusal` (cap 1, three refusals -> hook count
1, 2, 3, busy bytes written).

`cortex_replay` (pre-trait golden gate) now strips the documented, additive
nerve-wiring changes before comparing (Gaia owners on findings/incidents,
`exposure.ownership`, cortex incident list, owner evidence lines, no empty
`(last: )` suffix); everything else must still match byte for byte. The new
test `nerve_wiring_additions_are_present` checks those additions positively.

## Workspace (box, 2026-10-21 07:27-07:32)

- `cargo fmt --check`: ok
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings`: clean
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: **1237 passed, 0 failed, 6 ignored**
  (first run before the replay fix: 1 failure, `cortex_replay::replay_matches_the_pre_trait_build`, fixed as above)
- `scripts/no-python.sh`: ok
- Cross targets: `cargo check` of galion-common, openmind-nerve, nerve-client for aarch64-musl, armv7-musleabihf, aarch64-android: ok; x86_64-musl check of all 6 touched crates: ok. hermes-rs / hermes-gateway / openmind-memory on the 3 cross targets fail in `libsqlite3-sys` (bundled C build, no cross C compiler on this box) - pre-existing in both parents (rusqlite), not caused by this merge.
- Sandbox crates not run here (sandbox_vps_only); this branch does not touch them.

## GALION_REAL=1 real_wiring (release binaries, N_REQ=1000), 3 runs

`GALION_REAL=1 N_REQ=1000 cargo test -p openmind-nerve --test real_wiring -- --nocapture`
moved to slot-155 ports 20250-20257 and extended with a **conn-cap phase**
(memory :20256 / hermes :20257 with `*_HTTP_MAX_CONNECTIONS=1`; one idle
connection holds the slot -> 3 x 503 -> `conn_cap` event at the nerve ->
200 after release). All 3 runs passed (07:34:05, 07:35:30, 07:36:46).

Run 1 evidence (excerpt):

```
-- ownership (listen scope 20250-20259)
{"documented_unowned":false,"gaia":"ARTEMIS","owner":"hermes-rs","port":20251}
{"documented_unowned":false,"gaia":"APOLLO","owner":"openmind-memory","port":20252}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":53907,"port":20255,"process":"openmind-memory","unit":null}
{"cause":"hermes-rs","gaia":["ARTEMIS"],"severity":"error","title":"hermes-rs: pushed 59 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502"}
{"cause":"openmind-memory","gaia":["APOLLO"],"severity":"error","title":"openmind-memory: pushed 3 error(s) in the last 2 min; last: POST /seed -> 501"}
"hermes-rs" heartbeat: queued=293 sent=112 dropped=18 failed=181   (after hung + down phases)
{"gaia":["ARTEMIS"],"severity":"critical","title":"hermes-rs: failing: health probe down (connect: Connection refused (os error 111)); ..."}
{"kind":"event","msg":"daily provider call cap reached (3590 of 5)","name":"spend_cap","source":"hermes-rs",...}
openmind-memory :20256 while held: [503, 503, 503]
{"kind":"event","msg":"1 connections busy: 503","name":"conn_cap","source":"openmind-memory","ts":"2026-10-21T07:35:15+02:00","value":1.0}
openmind-memory :20256 after release: 200
hermes-rs :20257 while held: [503, 503, 503]
{"kind":"event","msg":"1 connections busy: 503","name":"conn_cap","source":"hermes-rs","ts":"2026-10-21T07:35:18+02:00","value":1.0}
hermes-rs :20257 after release: 200
-- RSS kB: nerve 5148 hermes 4472 memory 4472 stray 3944
```

Memory answered 1000/1000 with 200 in every phase of every run.

## Hermes latency: nerve down vs baseline (honest)

Baseline = same binary, `NERVE_URL` unset, no nerve. 1000 chat requests per
phase (10% `mock-fail` -> 502 by design), sequential, shared 8-core box
(load ~1.4-3).

| run | base p95 | base p99 | up p99 | hung p99 | down p95 | down p99 | down vs base p99 | up2 p99 |
|-----|---------:|---------:|-------:|---------:|---------:|---------:|-----------------:|--------:|
| 1 | 8.42 | 18.19 | 12.56 | 16.18 | 12.53 | 24.04 | +32.2% | 15.52 |
| 2 | 9.70 | 19.87 | 14.81 | 15.45 | 11.62 | 17.12 | -13.9% | 13.77 |
| 3 | 7.29 | 11.49 | 11.38 | 16.21 | 13.02 | 19.27 | +67.7% | 16.40 |
| median | 8.42 | 18.19 | 12.56 | 16.18 | 12.53 | 19.27 | **+5.9%** | 15.52 |

(ms). Memory p99: base 0.50-0.58, down 0.46-0.56.

Reading: the p99 delta swings from -14% to +68% between runs, so p99 on
1000 samples is noise-dominated on this box; the median is +5.9%, which is
**not** inside the 5% target. p95 is steadier and is ~+40-50% higher in the
hung/down phases (11.6-13.0 vs 7.3-9.7 ms) - but `up2` (nerve running again)
stays at 10.3-12.6 ms p95 too, so part of the rise is phase order (later
phases run after the slow/fail traffic, with a growing spend file and a few
429s), not only the nerve being unreachable. With the nerve down the client
keeps trying to connect (`failed=181` in run 1), on its own worker thread;
requests are never blocked (no unanswered requests in any phase). A clean
attribution needs an interleaved A/B (base and down alternating in one
process); not done here.

## Live VPS (read-only, 2026-10-21 07:35:49 +02:00)

- nerve `127.0.0.1:8670/state`: version 0.1.0, cycle 6842, mode live;
  components healthy 11 / degrading 2 (galion-one-mouth, cloudflared) /
  absent 2 (voice-worker, gaia); `signals.sources` = **0** (no live service
  pushes to the nerve yet: this wiring is not deployed). `/api/v1/signals`
  and `/api/v1/cortex` -> 404 on the live nerve (cortex not deployed;
  observed through the nerve per no_skip_extensive_testing).
- 50 x `GET /health`: hermes :8642 p50 0.38 ms / p99 0.57 ms; memory :8420
  p50 0.41 ms / p99 2.50 ms (all 200).
- Nothing on the VPS was changed, built or restarted.

## Re-verification and interleaved re-measure (resumed executor, 2026-10-21 07:54-08:05 +02:00)

Fresh executor after the previous one stopped at ~07:47 waiting on an
approval; the merge commit `ce71ce4` was already there and nothing had been
pushed. Re-run on the box:

- `cargo fmt --check`: ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools -- -D warnings`: clean
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`: **1225 passed, 0 failed, 6 ignored**
  (the 1237 above also ran hermes-tools' 12 tests; its `sandboxed.rs` runs the sandbox, so it is now excluded locally per sandbox_vps_only)
- `GALION_REAL=1 N_REQ=1000 real_wiring`: **5/5 passed** (07:56:41, 07:58:11, 08:00:58, 08:02:16, 08:03:33), conn-cap phase `[503, 503, 503]` -> `conn_cap` event -> 200 for memory and hermes every run.

### Why the sequential p99 delta is not the nerve

Sequential "down vs base" p99 over all 8 runs: +32.2, -13.9, +67.7, +75.1,
+50.1, +24.1, +51.2, +19.1 % (median +41%). But `base` is a separate fresh
process measured first, while `down` is the long-lived nerve-wired gateway
measured after 4000 earlier requests; `up2` (nerve running again) is just as
high (p99 14.9-26.1 ms). So the test now has an **interleaved A/B phase**
while the nerve is down: two fresh gateways, A `NERVE_URL` unset (:20253), B
`NERVE_URL` pointing at the dead nerve (:20258), same traffic (10%
`mock-fail`), 10 alternating blocks of 100 per side, order flipped each round.

| run (start) | A p50 | A p95 | A p99 | B p50 | B p95 | B p99 | B vs A p95 | B vs A p99 |
|-------------|------:|------:|------:|------:|------:|------:|-----------:|-----------:|
| 3 (08:01:31) | 3.69 | 7.88 | 11.90 | 3.38 | 8.55 | 11.84 | +8.5% | -0.5% |
| 4 (08:02:48) | 3.39 | 8.67 | 13.26 | 3.38 | 8.06 | 12.17 | -7.0% | -8.2% |
| 5 (08:04:06) | 3.41 | 7.76 | 11.70 | 3.30 | 7.34 | 14.39 | -5.4% | +23.0% |
| median | | | | | | | **-5.4%** | **-0.5%** |

(ms, n=1000 per side, codes per side 882-886 x 200, 14-18 x 429, 100 x 502 -
the 429s hit both sides equally.) Reading: with the nerve down, hermes p99 is
**within noise of no nerve at all (median -0.5%, range -8% .. +23%)**, inside
the 5% target on the median; a single run can still swing +23% on 10 tail
samples. Not measured interleaved: the **hung** nerve (SIGSTOP); its
sequential p95 stays ~11-13 ms vs base ~8 ms in every run, which is the one
open question (the client's connect/write timeout under a stopped peer).

### Live VPS, read-only (2026-10-21 08:05:05 +02:00)

nerve `127.0.0.1:8670/state`: version 0.1.0, cycle 6901, mode live, healthy
11 / degrading 2 (galion-one-mouth, cloudflared) / absent 2 (voice-worker,
gaia); `signals.sources` = [] (no live pusher yet - this wiring is not
deployed); `/api/v1/signals` and `/api/v1/cortex` -> 404 (cortex not live,
observed through the nerve). `GET /health`: hermes :8642 200 in 0.62 ms,
memory :8420 200 in 0.34 ms. Nothing on the VPS changed.
