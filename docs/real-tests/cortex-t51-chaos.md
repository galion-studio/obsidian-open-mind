<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex-t51-chaos (Cortex T5.1, slot 227)

All times Europe/Zagreb (+02:00). Box only, loopback only, ports 20970-20979.
No VPS write, no systemd unit touched, no model call, no network beyond 127.0.0.1.

## What runs

`crates/gaia-cortex/tests/real_t51_chaos.rs`, gated `GALION_REAL=1`:

```text
GALION_REAL=1 GALION_NERVE_BIN=<openmind-nerve release, swarm/nerve-event-driven bb287cc> \
  cargo test --release -p gaia-cortex --test real_t51_chaos -- --nocapture --test-threads 1
```

- cortex: the release `gaia-cortex` of this branch (base `swarm/cortex-event-driven` c7f56b8),
  `127.0.0.1:20971`, dry-run, `CORTEX_COALESCE_MS=500`, LLM default (off, feature not compiled).
- nerve: the unmodified release `openmind-nerve serve --mode event` from
  `swarm/nerve-event-driven` bb287cc (built in `target/cortex-t51-chaos/nerve-target`),
  `127.0.0.1:20970`, `NERVE_DRY_RUN=1`, `NERVE_REFLEX=0`, journal from a FIFO
  (`NERVE_EVT_JOURNALCTL=replay:<fifo>`, the nerve's own parser), dead component on :20979.
- latency proxy `:20972` (std-only, in the test), second cortex `:20973`, refused-start probe `:20974`.

## Scenarios and results (final runs 23:21:56, 23:23:32, 23:24:59; 3/3 green, 86.7-86.9 s each)

| # | chaos | asserted | measured (3 runs) |
|---|---|---|---|
| S1 | unit kill: `tuner.service` SIGKILL exit + failed via the journal | ONE decision for the 2 events, R3-known-crash, `POST /actions` dry-run answered by the nerve (`would_run systemctl restart --no-block tuner.service`) | 503 / 508 / 506 ms journal line -> decision |
| S2 | restart loop on the same unit, nerve default dedupe 600 s | nerve folds the repeats (deduped +5); cortex keeps 1 decision | **F1** (below) |
| S3 | ssh, cloudflared, postfix, galion-backup, hades-gate killed; event naming `/opt/galion/openmind` | R1-protected, alert only, 0 action requests (6/6) | 0 requests to the nerve |
| S4 | 4 stack units killed in one burst (8 events) | ONE R0-stack-wide decision, escalated, 0 restarts | 1 decision |
| S5 | 504 garbage journal lines, 1 MiB line, 20k-deep JSON into the nerve; into the cortex 1000 garbage NDJSON lines, 50k-deep nest, 2 MiB body, invalid UTF-8, 1500-item array, 5 injection unit names, raw bytes, 40-connection flood | both alive; garbage 400, nest 400, 2 MiB 413, UTF-8 400; array 202 accepted 1000 **invalid 500**; injection units R1/R4 only, never an action; only exact allowlist units ever reached the nerve | events_invalid +1502, actions +0; flood: /health 503, back to 200 after 5132 / 5141 / 5147 ms (head timeout 5 s) |
| S6 | second cortex through a latency proxy | 1 s each way: answered by the nerve; 4 s (> 3 s client timeout): `by = unknown` | 6465 / 6465 / 6467 ms; 4 s case `unknown` 3/3 |
| S7 | `SIGSTOP` the nerve, push a crash for `galion-desk.service`, `SIGCONT` | decision `by = unknown`, escalated, counts toward the cap; `/health` answers during the hang (`busy: true`); the stream survives | decision after 9722 / 9720 / 9725 ms (3 x 3 s timeouts); /health 0 ms; /stats 8821 / 8817 / 8820 ms; nerve logged the request after SIGCONT (1 late dry-run) |
| S8 | `SIGKILL` the nerve 150 ms after a kill line (incident in flight), new nerve on the same port (dedupe 1 s), 4 more failures 1.3 s apart | re-subscribe, `nerve_restarts = 1`, new events decided: R3 then R2-circuit-open (3 failures / 900 s), escalated, <= 3 restarts | re-subscribed 905 / 904 / 914 ms; new nerve seq 16-18 vs old 72-74; decisions `[R3 simulated, R2-circuit-open]`; in-flight crash decided `local` (nerve dead = never delivered) |
| S9 | error/critical ambiguous incidents (disk.crossed, kernel.error) | advisor skipped (disabled), llm_calls 0, spend $0, ledger untouched; nerve paid slot off, nerve llm_calls 0; no non-loopback socket on either process (`/proc/<pid>/fd` x `/proc/<pid>/net/{tcp,tcp6,udp,udp6}`) | skipped 2, calls 0, spend 0.0, sockets 0 / 0 |
| L | `CORTEX_LLM_ENABLED=1`; config `llm.enabled` + endpoint openrouter / `:8642` / hermes | exit 2 at start ("needs a build with the llm-advisor cargo feature") | 4/4 refused |

Final cortex counters (run 3): events_in 1087, batches 34, decisions 39, suppressed 4,
actions requested 6 (simulated 6, executed 0, unknown 1), alerts 24, escalations 10,
llm_calls 0, spend 0.0. Teardown: processes killed, ports 20970-20973 free, work dir removed
(checked after the runs: 0 leftover processes, 0 listeners in 20970-20979).

## Findings

| id | finding | before (23:09-23:12 runs) | status |
|---|---|---|---|
| F1 | The nerve folds repeats of one key for `dedupe_s` (default **600 s**, also live on the VPS) and does not re-emit them, so the cortex sees a crash loop as ONE failure per 10 min: its flap rule (3 in 900 s) can never fire through the real nerve; only the cortex cap (3/unit/h) bounds restarts. In dry-run the nerve's own caps (2/unit/h, 6/h) are not applied either (dry-run returns before the cap). | S2: 5 more failures -> deduped +5, cortex 1 decision | **open, owner decision D1** |
| F2 | After a nerve restart its `seq` starts at 1 again; the cortex resumed with `since=<old seq>` and the old seq fingerprints, so it was blind to the new nerve until the new seq passed the old cursor (here 70 events; on the VPS the live nerve is at seq 26 now). | S8: new seq 12 vs old 70, 0 of 4 failures decided | **fixed**: `nerve_instance` (GET /stats `started`+`seq`) + `resume_since` on every reconnect, `Cortex::nerve_restarted` clears fingerprints; `nerve_restarts` in /stats |
| F3 | Incident lifetime is counted in batches, not time: the reused correlator resolves an incident after 2 quiet *batches*, so two unrelated bursts close an open incident a second later. | S1 incident resolved after the S3 bursts | open, D3 |
| F4 | 32 idle connections fill the cortex API (`max_connections` 32) for the 5 s head timeout. The stream subscriber is outbound, so decisions continue; push ingest and reads wait. Loopback only. | /health 503 for ~5.1 s | accepted (documented) |
| F5 | The decider holds the engine lock across nerve HTTP calls (POST /actions, alert, escalation: up to 3 x 3 s per subject), so every API read waited for it. | /health 5741 ms during the hang | **partly fixed**: /health never blocks (`busy: true` snapshot); /stats etc. still wait (8.8 s). Moving nerve I/O out of the lock = follow-up D4 |
| F6 | A request written to the nerve whose reply timed out was reported `by = local` ("nerve unreachable": not run). The hung nerve processed it after SIGCONT. In live mode that is an uncounted restart. | S7: `local`, nerve logged 1 late action | **fixed**: only a refused connect is `local`; written-but-no-reply is `by = unknown` (dry-run `simulated`, live `failed`), escalated, counts toward the cap, `actions_unknown` in /stats |
| F7 | Push ingest silently dropped items past 1000 (`accepted 1000, invalid 0, ok true`). | S5 array 1500 | **fixed**: extra items are `invalid` (`ok false`) |
| F8 | `tests/pipeline.rs flapping_opens_circuit_then_escalates_once` was a time bomb (checked the circuit at the wall clock against fixed T0 = 18:26:40; red since ~19:29 (T0 + 120 s + 3600 s)). | failed on c7f56b8 at 23:13 | **fixed** (asserts at T0 + 240); `swarm/cortex-hermes-boss` fixed it the same way independently |

## Gates (23:19 - 23:28)

- `cargo fmt --all --check` ok; clippy `-D warnings --all-targets` for gaia-cortex + nerve-cortex-contract,
  default and `--features llm-advisor`: 0 warnings.
- `cargo test --release -p gaia-cortex -p nerve-cortex-contract`: lib 9, binary_e2e 1, pipeline 22,
  contract 8 (new: 3 nerve-link unit tests incl. a hung listener, 1 batch-limit test), all pass.
- Workspace (excl. openmind-sandbox, cgroup-detect, hermes-tools; `--no-fail-fast`): 228 suites,
  **1127 passed / 1 failed / 4 ignored**. The 1 failure is `openmind-nerve replay_corpus
  every_saved_capture_is_in_the_corpus`: box state, not this branch (it lists
  `/workspace/swarm/nerve-capture/`, where other slots saved `replay200` / `replay207` today).
- `scripts/no-python.sh` ok, `scripts/spdx-scan.sh` 417 scanned / 0 missing; repo-check: no finding in
  changed files (the 4 errors are pre-existing hard-coded test IPs in openmind-nerve).
- x86_64-unknown-linux-musl release 906,080 bytes (sha256 `9f2fe7c461324abb…`, static-pie);
  aarch64-unknown-linux-musl 777,536 bytes (`81fcac3d80b7c932…`, rust-lld);
  `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok.

## VPS read-only observation (2026-10-21 23:18:29 - 23:18:43)

GETs on 127.0.0.1 over ssh and `systemctl show` only. Nothing changed.

- Nerve `:8670` (event mode): `/health` 200 (system degraded, score 51), `/stats` seq 26, streams 1,
  started 22:56:55, deduped 11, **dedupe_s 600** (F1 applies live), `actions_live true`,
  escalate delegate, paid_enabled false, nerve llm_calls 0. galion-nerve MainPID 938448, NRestarts 0.
- **The cortex IS deployed**: `gaia-cortex.service` active since 23:08:11, build
  `/opt/galion/rust/bin/gaia-cortex-c5cae88` (= c7f56b8 + `swarm/cortex-hermes-boss` handoff),
  env `CORTEX_ACTIONS=live`, `CORTEX_LLM_ENABLED=1`, `CORTEX_HERMES_HANDOFF=1`.
  `/health`: mode live, llm_enabled true; `/stats`: events_in 8, decisions 5, actions 0,
  llm_calls 0 this process, ledger spend $0.000031. So on the VPS the advisor is NOT off; this run
  cannot assert "LLM off" for the live service, only for c7f56b8 + this branch. Reported, not touched.
- F2 and F6 matter more on that live service: a nerve restart blinds it (F2), and in live mode a
  hung nerve gives uncounted restarts (F6). Both fixes are on this branch, not deployed.

## Merging into the live line

Trial merge of this branch with `swarm/cortex-hermes-boss` 1c5dec4 (not committed): 2 conflicts.
`server.rs` /health: keep the non-blocking version and add `hermes_handoff` / `handoffs` to
`health_json`. `tests/pipeline.rs`: both sides fixed F8; keep either.
