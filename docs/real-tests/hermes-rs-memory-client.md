<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real test: hermes-rs memory client (slot 403)

Written 2026-10-28 17:41 (+01:00) Europe/Zagreb. Branch `swarm/hermes-rs-memory-client` off
1d7164b4. Box only (sandbox_vps_only): throwaway `openmind-memory` (release build of this
branch, temp data dir, test key) on 127.0.0.1:22730, real `hermes-gateway` processes on
22731 (memory on), 22732 (off control), 22733 (wrong key), 22734 (remote URL, must refuse).
Upstream LLM is the in-test stub as in the other real gateway tests; the memory side is the
real server, real HTTP, real SQLite store. Nothing on the VPS was changed.

Run: `GALION_REAL=1 cargo test --release -p hermes-gateway --test real_memory_client -- --nocapture`
(`crates/hermes-gateway/tests/real_memory_client.rs`; skips without `GALION_REAL=1`).

## Steps and results (3 runs 17:29:09-17:29:16 +01:00, all PASS, 2.3-2.4 s each)

| # | Check | Result |
|---|---|---|
| 1 | throwaway memory server healthy, gateway `/health/detailed` shows memory on, `key_set` true | ok |
| 2 | turn 1 (fact) -> capture lands in the store | 2 L0 lines after turn 1 |
| 3 | turn 2 asks for the fact -> recall hit, stub upstream sees the `<memory-context>` fence + system note + `## memory-tencentdb Memory` in the **last user message**; unrelated query recalls nothing. Recall also crosses sessions (F-403-1, below) | ok |
| 4 | streaming (`stream: true`) path recalls and captures too | ok |
| 5 | stored transcript has plain user text, no fence; captures have no fence (sanitized) | ok |
| 6 | off control gateway (no switch): no `memory` in health, store line count unchanged after its chats | ok |
| 7 | wrong key gateway: chats 200, `recall_err`/`capture_err` counted, nothing stored | ok |
| 8 | remote URL `http://192.0.2.10:8420` with switch on: exit 2, `memory url must be loopback` | ok |
| 9 | memory server killed: chat 200 in 4-6 ms, `recall_err` 1, `capture_err` 1; key string absent from every gateway log | ok |

Final on-gateway stats per run: `recall_ok 5, recall_hit 3, recall_err 0, capture_ok 5,
capture_err 0, capture_dropped 0`, 10 L0 lines in the store.

**Negative control** (17:30:11 +01:00): same test with `HMC_GATEWAY_BIN` = base 1d7164b4
`hermes-gateway` -> FAIL at step 1 (`memory` missing from `/health/detailed`), so the test
really needs the new code.

## Finding F-403-1 (owner decision D-403-2)

`openmind-memory` L0 fallback recall returns every stored text containing the raw query and is
**not scoped by session_key** (same as the Node gateway it replaced). So a fact said in one chat
can come back in another chat of the same `user_id`. That is the "long-term memory" intent, but
it is cross-session by design; the test asserts this behaviour so a change is noticed.

## Gates (17:31-17:37 +01:00)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` (no GALION_REAL) | 2027 passed, 0 failed, 18 ignored (381 suites) |
| hermes-rs unit tests (memory 10 + config 1 new) | ok |
| `scripts/no-python.sh` | ok |
| x86_64 musl release `hermes-gateway` | ok, 2,218,768 B (base 2,198,288, +20,480; base already over 2 MiB) |
| aarch64 musl release `hermes-gateway` (rust-lld) | ok, 2,062,392 B (base 2,045,824, +16,568; under 2 MiB) |
| `cargo check --all-targets` armv7-unknown-linux-musleabihf (hermes-rs, hermes-gateway) | ok |
| `cargo check --all-targets --no-default-features` aarch64-linux-android (same) | ok (default features need a C cross compiler for bundled SQLite, pre-existing on base) |

## Live read-only probe (2026-10-28 17:29 +01:00)

`hermes-rs` /health 200 and nerve `healthy`; `openmind-memory` :8420 /health ok and nerve
`healthy`; 0 established connections to :8420 (nobody writes memory today); live
`vectors.db-wal` mtime still 2026-10-19 14:57 +02:00; live config has no `hermes_rs:` block, so
this binary would stay off on deploy until the drop-in is added.
