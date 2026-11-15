<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real test: hermes-memory-session-scope (slot 426)

Written 2026-10-28 22:00 (+01:00) Europe/Zagreb. Branch `swarm/hermes-memory-session-scope`
off `swarm/hermes-rs-memory-client` 8887a0cb. D-403-2 / D-403-4 / F-403-1 prep. Box only, ports
22960-22969, throwaway data dirs, `--mock` provider (no network, no spend). Nothing deployed,
VPS read-only.

## What changed

- openmind-memory `POST /recall` takes optional `scope`: absent = legacy (unchanged reply),
  `"all"` = legacy + echo, `"session"` = L1 keyword lines and L0 text fallback limited to the
  request's `session_key` in SQL before the limit (L1: filter then take, scan window 100x
  `recall_max_results`), echoed; other values 400. `Store::l0_text_fallback_scoped` (FTS and
  LIKE paths).
- hermes-rs memory client: `scope: "session"` by default; `HERMES_RS_MEMORY_CROSS_SESSION=1`
  / `hermes_rs.memory.cross_session: true` = `"all"` (explicit opt-in). A session-scoped reply
  without the `scope: "session"` echo is dropped (`recall_unscoped`, nerve `memory_unscoped`).
  Health `memory.scope`, `memory.recall_unscoped`; start log `scope=`.
- Memory key unchanged (D-403-4): session_key, else session id.

## New real test `crates/hermes-gateway/tests/real_memory_session_scope.rs`

`GALION_REAL=1 HMS_REQUIRE_OLD=1 HMS_OLD_MEMORY_BIN=<8887a0cb openmind-memory> cargo test --release -p hermes-gateway --test real_memory_session_scope -- --nocapture`

| port | process |
|---|---|
| 22965 | openmind-memory (this branch, release) |
| 22966 | hermes-gateway, memory on, default scope |
| 22967 | hermes-gateway, `HERMES_RS_MEMORY_CROSS_SESSION=1` |
| 22968 | openmind-memory 8887a0cb (older, ignores `scope`) |
| 22969 | hermes-gateway default scope -> 22968 |

| # | Check | Result |
|---|---|---|
| 1 | session `s426-a` states fact A, `s426-b` states fact B; both captured | ok |
| 2 | A recalls A (fence + fact), B recalls B; neither sees the other's word | ok |
| 3 | B asks A's word, A asks B's word, new key `s426-c` asks A's word | **0 leaks** (no fence) |
| 4 | no session key: memory key = session id `api-...`; a new unkeyed session does not recall it; same `X-Hermes-Session-Id` does; keyed A does not see it | ok |
| - | store keys `s426-a`, `s426-b`, `s426-c`, 2x `api-...`, none empty; `recall_err` 0, `recall_unscoped` 0, health `scope: session` | ok |
| 5 | opt-in gateway (`scope: all`): new key `s426-x` recalls fact A | ok |
| 6 | old server 8887a0cb: raw `/recall` as B with `scope: "session"` returns A's fact and no echo (the leak); the default gateway drops it: no fence, `recall_unscoped` 2, `recall_hit` 0 | ok (fail closed) |
| 7 | memory key / API key absent from gateway logs; start log `scope=session` / `scope=all` | ok |

Runs: 21:47:16, 21:48:29, 21:48:30, 21:48:31 (+01:00) all PASS, about 1.0 s each.
Scoped gateway stats: `recall_ok 11, recall_hit 3, recall_err 0, recall_unscoped 0,
capture_ok 10`. Old-server leg: `recall_ok 2, recall_hit 0, recall_unscoped 2`.

**Negative controls (21:48 +01:00), test unchanged:**

| Gateway / server | Result |
|---|---|
| 8887a0cb gateway + 8887a0cb openmind-memory (`HMS_GATEWAY_BIN`, `HMS_MEMORY_BIN`) | FAIL at step 3: `LEAK A->B: echo: heliotrope-22965 ...` |
| 8887a0cb gateway + this branch's openmind-memory | FAIL at step 3: same leak (old client sends no `scope`, server stays legacy) |

So the test needs both halves: the new client asks for the scope and the new server applies it;
a new client against an old server fails closed (step 6).

## Other real tests on this branch (21:49-21:50 +01:00)

| Test | Ports | Result |
|---|---|---|
| `real_memory_client` (slot 403, step 3 now asserts **no** cross-session recall; health `scope: session`, `recall_unscoped` 0) | `HMC_PORT_BASE=22960` (22960-22964) | 1/1 PASS; stats `recall_ok 5, recall_hit 2, capture_ok 5`; memory down: chat 200 in 2 ms |
| openmind-memory `real_memory` (legacy routes, auth, crash, perf 10k, read-only) | `OPENMIND_REAL_PORT_BASE=22960` | 7/7 PASS (37.9 s) |
| openmind-memory `real_freshness` | 22967 | 2/2 PASS |
| openmind-memory `real_local_time` | 22968 | 1/1 PASS |

`real_schema` (fixed ports 19366-19369, outside this slot's range) was not run; the legacy
no-scope recall path it covers is unchanged and pinned by the goldens below.

## Unit / golden tests (new)

- `openmind-memory` golden `recall_scope_session_and_all`: legacy reply byte-identical with no
  `scope`; `"all"` echoes; `"session"` hides other sessions' L0 and L1 (`memory_count` 0), own
  session still recalls both; `null` = absent; `"SESSION"`, `"global"`, `1`, `true` -> 400.
- `openmind-memory` store `l0_text_fallback_scoped_keeps_other_sessions_out`: FTS and LIKE paths,
  10 newer rows of another session cannot starve a limit-1 scoped list; `%` stays literal.
- `hermes-rs` memory `session_scope_default_fails_closed_on_unscoped_reply`,
  `cross_session_is_an_explicit_opt_in`; `recall_wire_and_block` now pins `scope: "session"`.

## Gates (21:51-22:00 +01:00)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` (no GALION_REAL) | 382 suites, 2032 passed, 0 failed, 18 ignored (403: 2027) |
| `scripts/no-python.sh` | ok |
| armv7-unknown-linux-musleabihf `cargo check --all-targets` (hermes-rs, hermes-gateway, openmind-memory) | ok |
| aarch64-linux-android `--no-default-features` check (hermes-rs, hermes-gateway) | ok; openmind-memory android fails in `libsqlite3-sys` build script (no C cross compiler), same on base 8887a0cb (pre-existing) |

## Size delta (release, musl, vs 8887a0cb)

| Binary | x86_64 | aarch64 (rust-lld) |
|---|---|---|
| hermes-gateway | 2,218,768 B (+0) | 2,064,104 B (+1,328) |
| openmind-memory | 1,768,176 B (+4,096) | 1,722,624 B (+1,256) |

## Live read-only probe (2026-10-28 21:50 +01:00)

`hermes-rs` and `openmind-memory` units active; hermes-rs `/health` 200; openmind-memory
`/health` ok (`fts: true`, `read_only: false`); 0 established connections to :8420 (still
nobody recalls or captures on live); nerve `/state` cycle 18 (21:40:13 +01:00), self-eval 68 D,
`hermes-rs` and `openmind-memory` healthy, restarts_1h 0. Cortex not queried. The live
openmind-memory is pre-426, so a 426 gateway with memory on would recall nothing there (fail
closed) until openmind-memory from this branch is deployed too. No POST, no key read, nothing
changed on the VPS.
