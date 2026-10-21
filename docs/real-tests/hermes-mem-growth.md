<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# hermes-mem-growth (slot 198): hermes-gateway RSS growth under load

Base: `swarm/nerve-t33-backpressure` dfa93a1. Times are Europe/Zagreb (CEST +02:00).

## Short answer

The ~9.5 MB/min growth that slot 187 measured was **`MockProvider.calls`**.
It is a test-spy `Vec` that kept a clone of **every** request the mock
provider answered (target + system prompt + messages + tool schemas, about
10 kB each) and never dropped any. It is only used by `hermes-gateway --mock`
(shadow/smoke/soak runs) and unit tests. **The production path (CurlProvider,
which is what live hermes-rs runs) never grew.** Live hermes-rs is 3.3 MB
RSS after 7 h. It now stays flat at 6.2 MB under 30 chats/s in a VPS sandbox.

Fix:
- The `MockProvider.calls` log is bounded (`MOCK_CALL_LOG_CAP` = 256; when it
  is full the oldest half is dropped, so the newest ≥128 are always kept).
  A new `calls_total` counter counts every call, and `call_log_cap = 0` turns
  the log off.
- `hermes-gateway --mock` uses `MockProvider::echo().without_call_log()`,
  because nothing in the gateway reads the log.

## Heap profile (Rust only, std counting allocator)

`HMG_HEAP=1 cargo test --release -p hermes-rs --test heap_growth -- --nocapture`
(2026-10-21 16:47-16:50 +02:00). In-process `Gateway` with the SQLite store.
Warm-up is 2,600 chats, which gets past every bounded cache (spend map 2,000
sessions, runs 200, mock log 256). Then the live heap is sampled every 50
chats over 3,000 chats, and the result is the least-squares slope:

| configuration | retained per chat | notes |
|---|---|---|
| pre-198 behaviour (`call_log_cap = usize::MAX`) | **10,208 B** | 30.2 MB over 3,000 chats; clearing the log frees 56.5 MB = all of it (187 % of the window, since it also held the warm-up) |
| default cap 256 | 8.9 B slope | swing 1.2 MB = log sawtooth; log 224 of 5,600 calls |
| `--mock` (no log) | 0.3 B | swing 37 kB |
| production-shaped provider (records nothing, like CurlProvider) | 1.0 B | swing 37 kB |
| JSONL store (no-SQLite builds) | 478 B per new session | in-memory index, by design; see D-198-2 |

Attribution: clearing that one `Vec` gives back ≥95 % of the growth (the test
asserts this). After the fix, every configuration keeps under 32 B per chat
(also asserted).

## Real binary soak (`crates/hermes-gateway/tests/real_mem_growth.rs`, GALION_REAL=1)

The test runs two gateways side by side at 30 ops/s each:
- `--mock` on 20680
- the production code path (CurlProvider) on 20681, pointed at a stub
  OpenAI-compatible SSE upstream inside the test on 20682.

There is no network beyond 127.0.0.1, `HERMES_RS_PROVIDER_LOOPBACK_ONLY=1` is
set, and the paid model is off.

The load is a mix:
- new-session chats
- `mock-fail` (502) on the mock
- `/v1/runs` runs
- 6-turn session continuations
- `/health` + `/v1/models`

RSS, threads and fds are sampled every 15 s. The run passes when, after a
2 min warm-up, the slope is under 256 kB/min and the last quarter minus the
first quarter is under 4 MiB.

### Box, glibc release build

| run | time | binary | mock RSS warm → end, slope | stub-path RSS warm → end, slope | result |
|---|---|---|---|---|---|
| positive control, 4 min (`HMG_EXPECT_GROWTH=1`) | 16:39:51-16:43:51 | dfa93a1 | 25.8 → 83.0 MB, **18,883 kB/min** | 8.6 → 10.9 MB (still warming) | growth reproduced |
| fix, 20 min | 16:51:43-17:11:44 | b85c26e | 10.3 → 11.8 MB, 52 kB/min, +781 kB last-first quarter | 10.4 → 12.0 MB, 58 kB/min, +838 kB | **pass** |

Fix run counts:
- mock: 35,951 requests (26,963 ok, 8,988 deliberate 502, 0 other)
- stub path: 35,914 requests (35,903 ok, 11 × 429 from max_concurrent_runs)
- stub upstream hits: 46,006
- threads 2-3 and fds 7-10 throughout

RSS every 2 min (mock/stub, MB): 10.3/10.4, 11.3/10.8, 11.6/12.1, 11.9/11.9,
12.3/11.7, 11.8/11.8, 11.8/12.0, 11.9/11.9, 12.0/12.0, 11.8/12.0. This is a
plateau from about minute 6. The small rise before it shows on both gateways,
so it comes from the shared parts: SQLite's page cache (default about 2 MB)
filling as state.db grows, the 200 kept runs and the 2,000-entry spend map.
None of that is in the Rust heap profile.

### VPS sandbox, static x86_64 musl (the build type live runs)

Folder `/var/tmp/sandbox-test-20261021-165406` (0700, owner `nobody`). Both
runs were under `runuser -u nobody -- env -i ... TMPDIR=<folder> nice -n 10
timeout ...`, on ports 20680-20682 (fix) and 20685/20687 (control).

sha256:
- hermes-gateway fix 3d65d212…7876
- dfa93a1 ab84b03a…936d
- real_mem_growth 8ffd267e…387a

| run | time | mock RSS warm → end, slope | stub-path RSS warm → end, slope | result |
|---|---|---|---|---|
| positive control dfa93a1, 6 min, mock only | 16:54:20-17:00:21 | 22.3 → 111.2 MB, **17,603 kB/min** | — | growth reproduced |
| fix, 20 min | 16:54:20-17:14:21 | 6.2 → 6.5 MB, 9.2 kB/min, +127 kB | 6.2 → 6.2 MB, 0.6 kB/min, +15 kB | **pass** |

Fix run counts:
- mock: 36,003 requests (27,002 ok, 9,001 deliberate 502)
- stub path: 36,003 requests, all ok
- stub upstream hits: 46,134

The control's RSS by minute was 22.3, 41.3, 58.5, 76.2, 93.4 and 111.2 MB:
linear, about 10 kB per answered chat. That matches slot 187's 9.5 MB/min at
its rate and the heap profile.

Cleanup at 17:15:
- the folder was removed (logs copied to the box first)
- 0 `nobody` processes
- ports 20680-20689 free
- no user created (see below)
- nothing on the VPS installed, restarted or reconfigured

A temp file `/tmp/.hmg-state.json` (the nerve `/state` body, for grep) was
written and removed twice.

Disclosure: the planned temp system user `sbx198` was blocked by Auto-review
("creates a system user ... changes remote VPS configuration"). Per standing
instruction I did not request approval. The runs used the existing
unprivileged `nobody` account instead (uid 65534, nologin), with a 0700 folder
owned by it.

### Live VPS through the nerve (read-only; cortex not deployed)

- 16:53 (before): nerve `/state` cycle 860, live, score 61/D; component
  states healthy 18, degrading 2, failing-soon 4, failing 1, absent 4.
  hermes-rs PID 89484, RSS 3,292 kB after 25,590 s, NRestarts 0.
- 17:15 (after): hermes-rs same PID 89484, NRestarts 0. Nerve cycle 15,
  score 71. galion-nerve was restarted at 17:08:27 +02:00 by someone else; it
  now has a `70-nerve-dryrun.conf` drop-in. This slot never touched the nerve
  or its ports. Component counts were unchanged (18/2/4/1/4).

## Other structures checked (no growth on the production path)

| structure | bound |
|---|---|
| `runs::Runs` map | 200 kept, the oldest 50 done runs evicted; each run's events capped at 20,000 |
| `SpendTracker.sessions` | pruned at 2,000 to 1,500 (cheapest first). Every `record()` rewrites the spend JSON with all of them (CPU, not memory) |
| SQLite store | on disk; page cache bounded by SQLite's default |
| `nerve-client` | slot 187: no RSS, 2 threads |
| `hermes-tools` `Shared.calls` (coding tools, off unless `HERMES_RS_CODING_WORKTREE`) | one counter per session that used a coding tool, never evicted: ~100 B/session. Evicting it would reset the per-session call cap. Owner decision D-198-3 |
| `JsonlStore` index (no-SQLite builds: phones/Pi/cross builds) | one `SessionRow` per session in memory (~480 B), and `index.json` is rewritten whole on every upsert and append (2x per chat, O(sessions)). Owner decision D-198-2 |

## Gates (box)

- `cargo fmt --check`: ok
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings`: ok
- workspace tests (same excludes, debug): 272 suites, 1261 passed / 0 failed / 6 ignored (base 1257). New tests:
  - provider unit tests `mock_call_log_is_bounded_and_keeps_newest` and `mock_call_log_small_caps_and_off` (run every time)
  - `heap_growth` and `real_mem_growth`, gated
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 512 scanned, 0 missing
- repo-check: 0 errors, 49 warnings, PASS (same 49 as base)
- x86_64 musl static release build ok
- aarch64 musl release build (`--no-default-features`, rust-lld) ok
- aarch64-android and armv7-musleabihf `cargo check` ok (`--no-default-features`; bundled SQLite needs a C cross compiler, pre-existing)

## Owner decisions (Slavik)

- **D-198-1: merge the fix.** It is test/shadow-only code (MockProvider and
  `--mock`); the live binary's behaviour does not change, so there is nothing
  to deploy. Options:
  - (a) merge with the next hermes branch (recommended)
  - (b) leave it, and keep `--mock` soaks under ~20 min
  
  No money, no CPU.
- **D-198-2: JSONL session store (pure-Rust builds for phones/Pi).** Its
  in-memory index grows ~480 B per session, and it rewrites `index.json` whole
  twice per chat. At 10k sessions that is ~5 MB of RAM and ~2 × 3 MB written
  per chat. Options:
  - (a) append-only index log, compacted at start (recommended if the phone
    build is going to serve real traffic)
  - (b) cap plus archive old sessions
  - (c) leave it: only SQLite builds run on the VPS
  
  No money.
- **D-198-3: hermes-tools per-session call counters** (only when coding
  tools are on). They grow ~100 B per coding session. Options:
  - (a) evict sessions idle for more than N hours (resets their cap; a
    resumed session gets a fresh cap)
  - (b) leave it (recommended: coding is off on live, and the growth is tiny)
- **D-198-4: VPS sandbox user.** Creating a temp system user (`useradd`) is
  now blocked by Auto-review, so this slot used `nobody`. Options:
  - (a) accept `nobody` + a 0700 folder for sandbox-only soaks
  - (b) pre-create one permanent unprivileged `sbxtest` account for all
    slots (one-time, owner action)
