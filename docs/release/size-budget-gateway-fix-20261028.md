<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# One release size budget (wave-4 slot 353) — 2026-10-28 (Europe/Zagreb CET +01:00)

Branch `swarm/size-budget-gateway-fix` = release-pair-check 7427f96e + merge of hermes-gateway-2mib a0d099c2
(slot 329). No main-staging update, no deploy, VPS read-only.

## Problem

Three copies of `scripts/size-budget.sh` disagreed on hermes-gateway:

| script at | gateway rule | slot 339 set (`hermes-gateway-124a212` 2,198,288 B) | slot 353 set (1,944,432 B) |
|---|---|---|---|
| 7427f96e release-pair-check | 2.5 MiB, untagged + tagged names | ok | ok |
| 6ffb2570 release-repro-unify | 2.5 MiB, untagged name only | FAIL | ok |
| a0d099c2 hermes-gateway-2mib | 2 MiB, no exceptions | FAIL | ok |
| **this branch** | `config/release/size-budget.tsv` (2 MiB) | **FAIL** (correct) | **ok** |

The 2,198,288 B gateway was the slot 312/339 build without slot 329's size work, so it passed only through the
old exception.

## Fix

- `config/release/size-budget.tsv`: the one table. `default 2097152` plus one row per release binary (14), all
  2097152 today. Tighter per-binary rows are allowed; a row above 2 MiB needs an owner decision (D-353-1).
- `scripts/size-budget.sh`: reads the table (or `--budget-file` / `SIZE_BUDGET_FILE`), rejects bad tables
  (no default, duplicates, non-numeric, wrong shape, glob names), looks release names up without the commit
  tag (`<bin>-<7..40 hex>[-suffix]`, e.g. `gaia-cortex-77df80f-llm`), prints `budget` and `key` per row.
  ucpu-gate.sh, self-check.sh and CI call it unchanged.
- Rust tests read the same table: `hermes-gateway/tests/real_size_2mib.rs` (gateway row; new
  `HG2_PORT_BASE` so other slots can use their own ports), `openmind-nerve/tests/real_cross_target.rs`
  (nerve row; `<=` like the script). New `galion-realtest/tests/size_budget_gate.rs`: table policy + script
  on synthetic ELF fixtures incl. negative controls, and a `GALION_REAL=1` run on real release dirs.
- Slot 329's size work is merged as is: SQLite OMIT flags, C no-unwind-tables CFLAGS for musl targets,
  rust-lld for x86_64 musl, RELR for hermes-gateway (x86_64 + lld only), sha2 `force-soft-compact`.

## Result (x86_64 musl, 14-bin set, tag 942c3d9)

hermes-gateway 1,944,432 B (headroom 152,720 B; slot 339: 2,198,288 B, slot 329: 2,080,128 B). Largest
other: openmind-nerve 1,923,464 B (headroom 173,688 B). aarch64 musl: all 12 tree bins built (SQLite bins
too, via the slot 338 zig cc wrappers), size-budget ok (12), gateway 1,883,296 B. Full numbers:
docs/real-tests/size-budget-gateway-fix.md.

## Findings

- **F-353-1 (live, not changed):** the live `/opt/galion/rust/bin/hermes-gateway-b7fbbc78` (hermes-rs unit,
  mtime 2026-10-27 19:07 CET) is 2,732,248 B, stripped, no `.relr.dyn`: over 2 MiB and over the old 2.5 MiB
  exception. Replacing it is a deploy decision (D-353-2).
- **F-353-2:** openmind-nerve is now the binary closest to the budget (173,688 B headroom x86_64).
- **F-353-3:** the slot 329 C flags change cc-rs CFLAGS for all three musl targets. The slot 325/339
  byte-identity proof still holds on this tree (14 compared, 0 differ; solo = pair = full set).

## Owner decisions

- **D-353-1:** keep "no row above 2 MiB" as policy (enforced by size_budget_gate.rs), or allow a recorded
  per-binary exception. Recommendation: keep; nothing needs one today.
- **D-353-2:** at the next gateway deploy, ship a <= 2 MiB build from this tree (or its successor) to
  replace hermes-gateway-b7fbbc78. Not done here (no deploys).
- **D-353-3:** which branch carries the size budget into main-staging (this one supersedes the size-budget.sh
  in 6ffb2570, 7427f96e and a0d099c2). Not done here (no main-staging update).
