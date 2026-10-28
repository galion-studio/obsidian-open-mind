<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 412 cortex-live-merge-5: next cortex deploy candidate (merge prep)

Branch `swarm/cortex-live-merge-5`, base `cortex-live-merge-4` 58a12e61 (= the
live cortex `gaia-cortex-58a12e6-llm`, deployed 2026-10-28 17:37:56 CET; main
has PR #12 merged as 7ba85b60 at 18:09 CET, which does not touch this cortex
line). All times Europe/Zagreb (CET, +01:00), 2026-10-28. Box loopback only,
slot 412 ports 22820-22829. **NO deploy.** The cortex never calls OpenRouter
(endpoint guard in `llm.rs`, unchanged).

## What is merged

| Step | Commit | Content |
|---|---|---|
| base | `58a12e61` | cortex-live-merge-4 (live) |
| 1 | `6f513c04` (merges `7fc44a00`) | cortex-health-nerve-live (slot 347, F-337-1): `/health` `nerve` = real reachability + `nerve_contact` block (clean) |
| 2 | `cf8e89ac` (merges `b36dc358`) | cortex-version-commit (slot 377, D-377-2): std-only build stamp in `--version` and `/health` `commit`/`build`; also brings cortex-nerve-pair-runbook `4bd62a6b` (runbook, staged pair, `pair_preflight` test). Conflicts: `engine.rs::health_json` (kept the F-337-1 `contact.overlay` and added `commit` + `build`), `tests/binary_e2e.rs` (kept both test sets) |
| 3 | `6dcab61e` (merges `6e96ee54`) | cortex-handoff-exclude (slot 387): `hermes.exclude_units`, **off by default** (clean) |
| 4 | `5026d9cf` | `REAL_CORTEX_PORT_BASE` compile-time port block for `real_health_nerve` and `real_handoff_exclude` (defaults unchanged 22170 / 22570), so merge re-runs stay in the slot's range |

`5026d9cf` is the code candidate; the commit after it is docs/staging only.

## Gates (box, 18:36-18:50 CET)

- `cargo fmt --all --check` ok; `scripts/no-python.sh` ok.
- `cargo clippy -p gaia-cortex --all-targets -D warnings`: ok default, `--features llm-advisor`, and with `REAL_CORTEX_PORT_BASE=22820`.
- `cargo test -p gaia-cortex --all-targets`: lib 58, binary_e2e 4, flap_dedupe 7, handoff_exclude_corpus 4, incident_lifecycle 11, io_unlock 6, pair_preflight 5, pipeline 24, r3_week_replay 1, real suites gated skip-pass 11: **0 failed**. With `--features llm-advisor`: **132 passed / 0 failed**.
- `cargo test -p openmind-nerve --test replay_corpus` **4/4**, `--test cortex_replay` **5/5**; `cargo test -p nerve-cortex-contract` **9/9**.
- Release `--features llm-advisor`: x86_64 musl static, **1 159 968 B, sha256 `d5b71ef94a2632bbc782cb4e33affa7e80d8ad3f25111cc4d085d4a06162de7e`**, `--version` = `gaia-cortex 0.1.0 (commit 5026d9cf, built 2026-10-28T18:38:33+01:00, features llm-advisor)`. aarch64 musl build ok (984 632 B, sha256 `d6f83dcc…24c3`). `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok. Box artifacts `/workspace/swarm/artifacts/gaia-cortex-5026d9c-llm-{x86_64,aarch64}-musl` (+ `.sha256`), not installed anywhere.

## Real tests (`GALION_REAL=1`, release, `--features llm-advisor`, `REAL_CORTEX_PORT_BASE=22820`)

Inputs (sha256 checked OK): nerve-merge-6 `openmind-nerve-9baa450-x86_64-musl`
(= the live nerve), live cortex `gaia-cortex-58a12e6-llm-x86_64-musl`, old
`openmind-nerve-9d1a4f3` + `gaia-cortex-73583ab-llm` for the R3 "before" case.
Journal = the real VPS console.service lines replayed through the nerve FIFO.

| Test | Time (CET) | Result |
|---|---|---|
| `real_planned_stop_pair` (nerve 9baa450 + this cortex; live cortex 58a12e6 as base) | 18:39-18:41 | **1/1**. PAIR: R5b x3 + R5-recovered, 0 nerve restart requests; same with live cortex 58a12e6; SPLIT R99 + R5b, 0 requests; CONTROL crash / kill -9 / restart-then-crash -> R3 + 1 restart request each |
| `real_r3_planned_stop` nerve 9d1a4f3, base 73583ab | 18:46-18:47 | **1/1**. BEFORE R3 x2 + R2, 2 restart requests (reproduces dec-874); AFTER R5b x3, 0 requests; crash/kill9 -> R3 |
| `real_r3_planned_stop` nerve 9baa450, no base | 18:47-18:48 | **1/1**. AFTER R5b x3 + R5-recovered, 0 requests; crash/kill9/mixed -> R3 |
| `real_health_nerve` (nerve 9baa450, base 58a12e6) | 18:42 | **1/1**. P0 nerve down: new `nerve=false unreachable`, base (live) still says `true` (F-337-1 bug); P1 up after 825 ms; P2 SIGKILL -> false at once; P3 restart -> true after 457 ms; P4 SIGSTOP -> `stale` after 5 s with stale=5 |
| `real_build_stamp` | 18:42-18:44 | **1/1**. 3 builds from different paths byte-identical to the artifact sha `d5b71ef9…`; `git archive` build without env = `commit unknown`; `/health` commit ok on 22820-22822 |
| `real_handoff_exclude` (nerve 9baa450, `GALION_CORTEX_BIN` = the musl artifact) | 18:44-18:46 | **1/1**. off: 5 test tickets, control rate-limited (live 06:24 incident); on: 0 test tickets, 10/10 excluded, control handed; `hermes*`, `*`, `galion-*`, `*test*` refused (exit 2) |

First `real_r3_planned_stop` run (18:41) used base = 58a12e6 and failed its
"before must reproduce R3" assertion: expected, because 58a12e6 already has
R5b. Re-run with the test's documented base 73583ab passed (above).
Not run: `real_t51_chaos`, `real_t237_backfill`, `real_api_load`,
`real_flap_dedupe`, `real_nerve_io_unlock` - they hard-code ports of other
slots (20974, 21270-21275, 21076/21078); their code is untouched by this merge
and they passed on merge-4.
All started processes ended with the tests; ports 22820-22829 free after.

## Live VPS (read-only, 18:49 CET)

`gaia-cortex` active, `ExecStart=/opt/galion/rust/bin/gaia-cortex-58a12e6-llm`
via `91-pair-58a12e6.conf`, NRestarts 0, started 17:37:56 CET. `/health`:
`ok:true mode:live llm_enabled:true hermes_handoff:true busy:false nerve:true`,
no `commit` and no `nerve_contact` (both arrive with this candidate). Last 200
decisions: R11 8, R99 6, R1 1. `galion-nerve` active; `/state` 200 (open
galion-one mouth + edge incidents, out of scope). Nothing changed on the VPS.

## Deploy checklist (parent/owner only; slot 412 does NOT deploy)

1. Artifact `gaia-cortex-5026d9c-llm-x86_64-musl` sha256 `d5b71ef9…62de7e`, 1 159 968 B (or rebuild from `5026d9cf` with `--features llm-advisor --target x86_64-unknown-linux-musl`; `real_build_stamp` shows the rebuild is byte-identical; from `git archive` pass `GALION_BUILD_COMMIT` + `SOURCE_DATE_EPOCH`, D-377-2). Install as `/opt/galion/rust/bin/gaia-cortex-5026d9c-llm`; keep `gaia-cortex-58a12e6-llm`.
2. Unit verify in a throwaway folder (runbook step 3) with the staged `deploy/staged/cortex-live-merge-5/gaia-cortex.service.d/93-merge5-5026d9c.conf` (sorts after 91, so its `ExecStart` wins).
3. Install 93, daemon-reload, restart gaia-cortex. Do **not** install `92-hermes-exclude.conf` in the same step (D-387-1 is separate).
4. Gates (60 s): active, ExecStart names `gaia-cortex-5026d9c-llm`, NRestarts 0; `/health` `ok:true mode:live llm_enabled:true hermes_handoff:true busy:false nerve:true`, **new** `commit:"5026d9cf"`, `nerve_contact.state:"ok"`, `stream:"connected"`; no panic in the journal.
5. Rollback: `rm 93-merge5-5026d9c.conf`, daemon-reload, restart -> `gaia-cortex-58a12e6-llm` via 91.
6. Nerve unchanged (stays 9baa450); no nerve step needed.

## Owner decisions

- D-412-1: deploy `5026d9cf` as the next live cortex (adds F-337-1 real nerve reachability, D-377 build stamp, exclude_units off). Recommended: yes; low risk, the decision rules are unchanged versus the live 58a12e6 (same R5b/R3 behaviour in the pair tests).
- D-387-1 (unchanged): later, install `92-hermes-exclude.conf` to stop test-unit Hermes tickets; needs this binary first.
- D-412-2: `/health` gains `nerve_contact`, `nerve_configured`, `commit`, `build`, and `nerve` can now be `false` when the nerve is stale. Any watcher that alarms on `nerve:false` (live-fix watch) will now see real outages; recommended: yes, that is the point of F-337-1.
