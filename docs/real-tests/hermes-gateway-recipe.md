<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: hermes-gateway release recipe (slot 434)

Branch `swarm/hermes-gateway-recipe` off `swarm/hermes-gateway-candidate` 20936d52. All times Europe/Zagreb,
CET (+01:00), 2026-10-28. Box only (ports 23040-23049), VPS read-only. Nothing deployed. Hermes not used.
Evidence page: `docs/release/hermes-gateway-recipe-20261028.md`.

## Builds with `cargo xtask release-recipe` (commit 989958be)

| time | command | result |
|---|---|---|
| 23:12:43 | `release-recipe build --commit 989958be --cc gcc --out artifacts/t434/a-gcc` | 1,952,592 B sha256 0f7bb471…8ac8, within budget (144,560 B headroom) |
| 23:12:52 | `release-recipe build --commit 989958be --cc zig --out artifacts/t434/a-zig` | 2,088,128 B sha256 47d3897a…bc49, within budget (9,024 B headroom) |
| 23:13–23:15 | `release-recipe verify a-gcc/hermes-gateway-989958be --work target/hermes-gateway-recipe-pathB-gcc` | rebuilt in another path: same sha, `verify OK` |
| 23:15–23:17 | `release-recipe verify a-zig/hermes-gateway-989958be-zig --work …pathB-zig` | same sha, `verify OK` |

Both shas equal the slot 416 numbers (gcc = the `# sha256` of the staged `95-gateway-candidate.conf`).

## Box parity vs live b7fbbc78 bytes (`real_candidate_parity`, P1–P4)

`GALION_REAL=1 HERMES_GW_BIN=<artefact> HERMES_GW_LIVE_BIN=artifacts/t291u/hermes-gateway-b7fbbc78
HERMES_GW_LIVE_SHA=c5432a88… HERMES_PARITY_PORT_BASE=23040` (new env, ports 23040-23043; default stays 22860).

| time | candidate | result |
|---|---|---|
| 23:17:28 | gcc 0f7bb471 (+ `HERMES_GW_CHECK_DROPIN_SHA=1`) | **pass** P1–P4: sha == staged drop-in; `--check-config` same line, `/var/lib` worktree live 0 / cand 2; 9 routes same status + body; chat "echo: parity ping 416" both; tools `clock`, `delegate_task` both |
| 23:17:40 | zig 47d3897a | **pass** P1–P4 (same results) |

P5 (sandboxed preflight) is VPS-only and not re-run here (slot 416 passed it for the gcc bytes).

## `crates/xtask/tests/real_release_recipe.rs` (GALION_REAL=1, 23:19:42–23:26:25, 252 s, pass)

| step | result |
|---|---|
| R1 build `--commit HEAD` (5a5eb14b) gcc | 1,952,592 B sha 0f7bb471 (same bytes as 989958be: gateway source unchanged, other commit stamp/epoch) |
| R2 real binary | `--check-config` exit 0; `--mock` on 127.0.0.1:23044, `/health` 200 in 101 ms `{"status":"ok",…,"implementation":"hermes-rs"}`; port closed after kill |
| R3 `check` | OK |
| R4 `verify` other path | rebuilt same sha, `verify OK` |
| R5 recipe sha zeroed | `check` exit 1 "artefact sha256 … != recipe sha256 000…" |
| R6 recorded `rustc=rustc 0.0.0` | `verify` exit 1 "toolchain differs" before any build |
| R8 `cargo_argv` + `--features coding` | `check` exit 1 "cargo_argv was edited" |
| R7 recipe commit -> 83e573e2 | rebuilt 1,948,240 B sha 8f8ac925… ≠ recorded → `verify` exit 1 "recipe does NOT reproduce the artefact" (1,948,240 B = slot 373's number for 83e573e2) |

First run 23:17:46 failed at R2: test key 15 chars, gateway refuses < 16 (test bug, fixed).
Unit tests: `cargo test -p xtask` 11/0 (recipe roundtrip, bad recipes, mismatch, drift, argv pin, budget,
triples, committed gcc recipe == staged drop-in sha + name, zig recipe same source other bytes).

## VPS read-only (23:26:39)

hermes-rs active, MainPID 255539, NRestarts 0, ExecStart `/opt/galion/rust/bin/hermes-gateway-b7fbbc78`
(sha256 c5432a88…, 2,732,248 B), drop-ins 50/60/70/90 only (no 95), no 989958be file on the VPS;
`:8642/health` 200, nerve `:8670/state` 200 (cycle 25), cortex `:8674/health` 200. Nothing written.
