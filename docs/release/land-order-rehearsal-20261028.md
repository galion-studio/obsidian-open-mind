<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Land-order rehearsal (wave-4 slot 359), 2026-10-28 05:09-05:31 CET (+01:00)

Throwaway branch `swarm/land-order-rehearsal` off the swarm/main-staging tip. GitHub main-staging
is 38a07700 (PR #13); its box snapshot is 3f015986 (tree 4864589f, byte-identical, slot 312).
Every candidate commit is built on 3f015986. **swarm/main-staging and main were NOT updated.
Nothing deployed.**

## Order rehearsed (D-337-1 / D-339-1)

| step | merged | result | rehearsal commit |
|---|---|---|---|
| 1 | 186cd61c cortex-staging-land-check | clean | f4813b7b |
| 2 | 6ffb2570 release-repro-unify (325) | **2 conflicts**: `crates/gaia-core/src/ownership.rs` and `crates/gaia/tests/commands.rs` (crate counts: HEAD 71/59, theirs 70/58) -> resolved to **73 total / 61 system** with both comment lines | b162a7f5 |
| 3 | 7091f76b release-artifacts-refresh (312) | clean | 74717121 |
| 4 | c7cebb28 console-v2-admin-fold | **2 conflicts**: `crates/console-ui/src/port2.rs` (SPA_FIRST/LIVE_SRC_ROUTES vs /arena,/bank Rust-first) and `crates/console-static/tests/ui_port2.rs` (fallback test) -> resolved to the release-bins-add fc481390 versions | d6b4d66e |
| 5 | fc481390 release-bins-add | clean | 15b08dbf |
| 6 | 5aa31927 console-deploy-candidate code (console-dist-arena-bank 7888d8fb) | clean | 83c7584e (tree 223544ea) |
| fix | F-359-1 (below) | 1 test file | e158400a |

## Findings

- **L-359-1 (trap in step 2).** The first attempt copied slot 337's resolution 83c6e265 for
  ownership.rs. That file has a **damaged SPDX header** (comment + `assert_eq!` text pasted into
  line 1); slot 337 fixed it later in d360ef28. When fc481390 is then merged (step 5), git keeps
  the damaged line silently (criss-cross merge base: the damage looks like "our" change, fc481390's
  side looks unchanged). The build catches it (`cargo check -p gaia-core` on 75fb6d44: "unclosed
  delimiter", rc 101), but only after a full merge chain; resolve it cleanly up front.
  Attempt 1 was discarded (local tag `rehearsal-attempt1-badheader` 75fb6d44); attempt 2 resolves ownership.rs to the
  clean fc481390 content. **Never reuse 83c6e265's ownership.rs.**
- **L-359-2 (simpler land).** fc481390 already contains 186cd61c, 6ffb2570 and c7cebb28 with
  the conflicts resolved. Landing `fc481390` (fast-forward from 3f015986) then merging `7091f76b`
  and `5aa31927` gives **0 conflicts and the identical tree 223544ea**. Recommended order for the
  real land: **fc481390 -> 7091f76b -> 5aa31927** (+ F-359-1). Same result, no manual resolution.
- **F-359-1 (repo-check error, fixed here).** `crates/console-static/tests/real_admin_flags_live.rs`
  (from 49b31b3f via c7cebb28) hard-coded the VPS public IPv4 as the ssh default -> repo-check
  `hosts` 2 errors (also on fc481390 alone). Default is now the ssh alias `slavik-vps` (same as
  galion-realtest `GALION_REAL_VPS_HOST`), `CONSOLE_UNIT_SSH` still overrides. repo-check 0 errors
  after the fix; the live read-only test still passes.
- **P-359-1 (ports).** real_release_smoke (22070-22079), real_dist_arena_bank (22060-22069) and
  real_release_artifacts (21820-21829) hard-code their slot ports. For this rehearsal they were
  remapped in the working copy only (22290-22299) and reverted; 7427f96e (release-pair-check) adds
  `GALION_RELEASE_PORT_BASE` for the artifacts test only. Suggest the same env for the other two.
- **S-359-1 (size).** hermes-gateway 2,202,384 B is over 2 MiB but under its 2.5 MiB cap
  (F-353-1 is about the live 2.73 MB build, not this one). galion-console-static/-serve grew to
  1,037,184 B (was 867,192 on fc481390 alone: +arena/bank reader from 5aa31927), still under 2 MiB.

## Gates on 83c7584e + e158400a (box, CARGO_TARGET_DIR target/land-order-rehearsal)

| gate | time CET | result |
|---|---|---|
| `cargo fmt --all --check` | 05:10 | ok |
| clippy `--workspace --all-targets` excl. openmind-sandbox, cgroup-detect, `-D warnings` | 05:11 | rc 0 |
| clippy gaia-cortex `--all-targets --features llm-advisor -D warnings` | 05:12 | rc 0 |
| ws test excl. openmind-sandbox, cgroup-detect, hermes-tools, `--no-fail-fast` | 05:12-05:16 | **420 suites, 2213 passed, 0 failed, 18 ignored** |
| `scripts/no-python.sh` | 05:11 | ok |
| `scripts/spdx-scan.sh` | 05:11 | 811 scanned, 0 missing |
| repo-check `--root .` on 83c7584e | 05:16 | FAIL 2 errors (F-359-1), 86 warnings |
| repo-check after F-359-1 | 05:16 | **PASS 0 errors**, 86 warnings |
| real_release_repro (GALION_REAL, x86_64 musl, --locked --offline) | 05:18-05:24 | **19 compared, 0 differ**, 14 bins 0 over budget/unstripped, 396 s |
| `scripts/size-budget.sh --check-dir` full set | 05:24 | ok (14 binaries) |
| real_release_smoke on release bytes (ports 22290/22291/22298/22299) | 05:26 | 14 checks pass |
| real_release_artifacts on tagged release bytes (ports 22297/22290/22299) | 05:27 | 21 checks pass |
| real_dist_arena_bank local legs (22292-22296) | 05:27 | 3/3 (live leg skipped in that run) |
| real_dist_arena_bank live leg, VPS read-only (ssh -L to :18080) | 05:27 | 1/1: live board 12 ids -> box /api/arena 200 12/12; live :18080 /api/arena and /api/v1/bank 404 (gap still open until deploy) |
| real_admin_flags_live, VPS read-only | 05:17 | 1/1: both console units mirror == live, active, NRestarts 0, no drop-ins |

Sandbox tests (sandbox_vps_only) not run: nothing in this land touches the sandbox.
aarch64 byte proof not run (needs the cross C compiler, D-318-3 / D-339-3).

## Live VPS (read-only, 05:27 CET)

Nerve 127.0.0.1:8670 /state 200; open alert inc-865945dd galion-one mouth stuck loading 11.0 h
(slot 358 owns it). console and console-rust-primary active. Nothing changed on the VPS.

## Owner decisions

- **D-359-1** land via fc481390 -> 7091f76b -> 5aa31927 (+ e158400a F-359-1) instead of the
  six-step order; same tree, no manual conflicts.
- **D-359-2** carry F-359-1 into the land (repo-check otherwise fails with 2 errors).
