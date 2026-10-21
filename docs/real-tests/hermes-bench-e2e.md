<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real test: the bench sector drives hermes-gateway end to end (slot 162)

Branch `swarm/hermes-bench-e2e` = hermes-core-bench-sector 12be83b + hermes-gateway-bench
20d9a2f (merge 79c9a2c) + this commit. Recorded 2026-10-21 08:16-08:30 CEST (+02:00) on the box.
Slot 162 ports 20320-20329: the Hermes status API test used 20320, `real_hermes` used
20322/20323 (`HERMES_REAL_PORT=20322`). The gateway, the mock LLM, the mock bank and the two
loopback backend fronts bind kernel-assigned 127.0.0.1 ports (the whitelist allows no port flag,
same rule hermes-bench always followed). No paid model, no key read or printed, no money, no
sandbox run, nothing written to or restarted on the VPS (one disclosure in section 5).

## 1. What runs

```
hermes bench run --bin target/release/hermes-bench            (hermes-core coordinator, governor cap 3)
  └─ hermes-bench run --bench B --agent gateway                (child, cleared env: PATH HOME TMPDIR TZ)
       ├─ mock bank (bench-banking), paper broker, vending sim  (the run's own backends)
       ├─ loopback HTTP fronts for broker + sim                 (so the gateway and the checks share state)
       ├─ mock LLM 127.0.0.1:<ephemeral>                        (scripted brain, no key, records transcripts)
       └─ hermes-gateway (bench build next to hermes-bench)     (env cleared; HERMES_RS_BENCH=1,
            HERMES_RS_HOST=127.0.0.1, MCP off, *_URL = the loopback backends, model = the mock LLM)
            ◀── POST /v1/chat/completions per episode / shift (Bearer per-run test key)
```

Each episode prompt goes to the gateway; the gateway's hermes-rs agent loop asks the mock LLM,
which answers with the reference script's tool calls; the gateway executes them with its own
bench tools against the run's backends. Tool calls are read back from the message list the
gateway sent to the mock LLM. Banking drafts and the audit chain live in the gateway's own
`BankingTools`, so they are read back through the gateway (`drafts_list`, `audit_trail` probe
turns) and the banking card gets one extra axis: every draft is `awaiting_owner` and unsubmitted.

## 2. Commands

```
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/hermes-bench-e2e
cargo-slot build --release -j 2 -p hermes-gateway --features bench -p hermes-bench
hermes-bench run --bench all --agent gateway --out /tmp/x162/out --sha test   (env -i, manual, 08:16)
GALION_REAL=1 cargo-slot test --release -j 2 -p hermes-core --test real_bench -- --nocapture
GALION_REAL=1 HERMES_REAL_PORT=20322 cargo-slot test --release -j 2 -p hermes-core --test real_hermes -- --nocapture
```

## 3. real_bench, run 2026-10-21 08:22:25-08:22:48 +02:00: 1 passed, 0 failed (9.2 s)

| Step | Result |
|---|---|
| RB1 | bench sector: 4 sessions / 12 tasks (planner 4/12), 0 gated; agents allowed `scripted, mock-llm, gateway` |
| RB2 | `hermes bench run`: 12 done / 0 failed / 0 blocked / 0 refused / 0 retries, peak 3/3, 7.8 s wall. S4.1 banking 400 ms, S4.2 wallstreet 200 ms, S4.3 vending 3604 ms (30 shifts, 443 tool calls). Scoreboards: scripted 2909 B, mock-llm 2778 B, gateway 2835 B, each 7 x 100% |
| RB3 | 6 scripted/mock-llm scores = committed record (457ef74). Gateway: banking axes = mock-llm record + `payments stay drafts awaiting_owner (gateway tools)` 100; wallstreet axes = record; vending axes = record and final_state_hash c4b11d2642a0b6be = record |
| RB4 | ledger 24 lines, chain sound; all 12 plan rows `done`; no `--agent live` in the ledger; no marker env reached a child |
| RB5 | readiness 12 of 16 pass; G1 G3 G4 G5 G16 pass. G16: `built-in plan 12 tasks (9 runs, 3 scoreboards), gateway e2e runs 3/3, whitelist refuses live agent and backend URLs; ... 12 done`. Fails unchanged owner items: G9 HADES config, G11 model credit, G14 open decisions; G12 unknown |
| RB6 | `hermes serve --port 20320`: GET /v1/bench 200 (10741 B, done 12), /v1/tasks?sector=bench&state=done count 12, /v1/readiness G16 pass, POST /v1/bench 405 |
| RB7 | intake `--agent live` refused (exit 3, 0 attempts, no process started); last.json kept |
| RB8 | no-op without --rerun, re-run with --rerun 100%, partial run does not replace last.json |
| RB9 | see below |

RB9, from the gateway evidence in each score JSON:

| Run | gateway | chat turns | LLM requests (with auth header) | tools offered | ws_kill_reset | money-move tools | backend front requests |
|---|---|---|---|---|---|---|---|
| banking | pid 604385, 127.0.0.1:39283 | 17 (5 episode turns; the 6th is refused before the model; + 12 drafts/audit probe turns) | 36 (0) | 38 | not offered | 0 | - (bank is HTTP already) |
| wallstreet | pid 604299, 127.0.0.1:35179 | 2 | 10 (0) | 38 | not offered | 0 | wallstreet 8 |
| vending | pid 605174, 127.0.0.1:33783 | 30 | 473 (0) | 38 | not offered | 0 | vending 443 = hermes_tool_calls |

Gateway start-up line (vending run):
`hermes-gateway bench on: 36 tools (banking@http://127.0.0.1:33741, wallstreet@http://127.0.0.1:45625, vending@http://127.0.0.1:35113), local mock/paper/sim only, ws_kill_reset operator-only`.
Banking: `drafts_awaiting_owner` true, mock `forbidden_total` 0, `money_moves` 0. Every gateway
pid gone after its run, 0 leftover gateway homes in TMPDIR. A hermes-bench copied to a folder with
no gateway next to it exits 1: `--agent gateway needs .../hermes-gateway (cargo build --release -p
hermes-gateway --features bench)`.

## 4. Other runs

- Manual `hermes-bench run --bench all --agent gateway` 08:16:54 +02:00 under `env -i`: banking 100%
  (episodes, forbidden 0), wallstreet 100%, vending 100% (30 shifts, 443 tool calls, total_bp 10000
  = reference, hash c4b11d2642a0b6be, replay_matches_live true), 4.0 s, exit 0.
- Fail closed with a default-feature gateway (08:23:41 +02:00): built `hermes-gateway` without
  `bench` (1,846,312 B), put it next to hermes-bench: `... is not a bench build (no "bench on:" line)`,
  gateway log `HERMES_RS_BENCH ignored: this build has no bench feature`; exit 1, child killed, home removed.
- `real_hermes` (GALION_REAL=1, 20322/20323) 08:23:11-08:23:20 +02:00: passed after updating its totals
  (sessions 47+4, tasks 200+12, target/useful +12); G4 `212 tasks parsed = 212 reported`.
- Static x86_64 musl pair (hermes-bench 1,610,528 B, hermes-gateway `--no-default-features --features
  bench` 1,557,280 B) under `env -i`, 08:27:23 +02:00: banking 100%, wallstreet 100%, vending 100%
  (443 tool calls, hash c4b11d2642a0b6be), exit 0. aarch64 musl (rust-lld) builds static;
  `cargo check` passes for aarch64-linux-android and armv7-unknown-linux-musleabihf (hermes-bench,
  hermes-core).
- Checks: `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` 0
  warnings (+ hermes-gateway `--features bench`); `cargo test --workspace --exclude openmind-sandbox
  --exclude cgroup-detect --exclude hermes-tools` 184 suites, 758 passed, 0 failed; hermes-bench lib
  18 (3 new e2e), hermes-core lib 34, tests/bench.rs 10 (1 new); no-python ok; spdx-scan 305 files,
  0 missing.
- Not run: `real_gateway_bench` (slot 128 hard-codes ports 19980-19989, outside this slot's range).
  Its gateway path is the same code RB9 exercises.

## 5. VPS read-only probes (2026-10-21 08:24:00-08:24:07 +02:00, ssh slavik-vps, GET/reads only)

- Nerve `GET 127.0.0.1:8670/state`: 200, 13091 B, mode `live`, cycle 6939, 15 components: 11 healthy,
  2 degrading, 2 absent. `hermes-rs`: healthy since 2026-10-21 02:57:05 +02:00, unit active/running,
  pid 1361728, port 8642, RSS 4504 kB, 0 restarts in 1 h, Gaia ARTEMIS / hermes-agents (the system the
  bench sector reports to).
- Cortex: not deployed (`127.0.0.1:8680` no answer); observed through the nerve only.
- `GET 127.0.0.1:8642/health`: 200 `{"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}`.
- Live binary `/opt/galion/rust/bin/hermes-gateway`: 2,173,696 B, mtime 2026-10-18 22:00:49 +02:00,
  contains neither `bench on:` nor the `HERMES_RS_BENCH ignored` stub, so it predates the bench
  wiring; nothing on the VPS can serve bench tools. NRestarts 0, unchanged.
- No listener on the VPS in 20320-20329 or 19980-19989.
- Disclosure: the first probe command wrote the nerve JSON to `/tmp/.n162` on the VPS and removed it in
  the same command (no secret in it). Later probes piped instead.

The benchmarks cannot be compared with a VPS service, because none runs there. The real-data
checks are RB3 (scores and state hash = the committed record) and the live nerve/hermes-rs reads.
