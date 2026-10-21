<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# swarm/integration-1: all 20 wave-1 branches on one branch

Wave 2, slot 22. Built 2026-10-20 20:15-20:35 (Europe/Zagreb, +02:00) from
main `13a296c` with normal `git merge --no-ff` commits, one per branch, in
the order below. No rebase, no squash, no force-push. Each wave-1 tip was
checked against the SHA in WAVE2.md before merging (all 20 matched).
Not merged: `gaia-land` (slot 01, still running, not pushed).

## Merge table

| # | branch | tip | result | resolution |
|---|---|---|---|---|
| 1 | docs-sync | 80e28f3 | clean | - |
| 2 | ucpu | 3e70ffb | clean | - |
| 3 | one-probe | b44cd4f | clean | - |
| 4 | tuner-cutover | 5fd1611 | clean | - |
| 5 | room | 5bb27e1 | conflict | `crates/room/src/main.rs`: took room's rewrite (R1) |
| 6 | sandbox | d194c19 | clean | - |
| 7 | hermes-tools | 8ccdfcc | clean | (contains sandbox d194c19, already merged) |
| 8 | hades-approval | ccaab42 | conflict | `.cargo/config.toml` add/add, `crates/hades-gate/tests/journal.rs` (R2) |
| 9 | survival-heal | 45a18aa | conflict | `docs/STATUS.md` (R3) |
| 10 | gaia-live | 4fbbb7f | clean | - |
| 11 | nerve | 5802d0e | conflict | `crates/openmind-nerve/{Cargo.toml,src/lib.rs,src/main.rs}`: took nerve's (R4) |
| 12 | memory-real | 723b46c | clean | - |
| 13 | desk-linkwatch | a54c792 | conflict | `crates/galion-desk/src/main.rs`: took desk-linkwatch's rewrite (R5) |
| 14 | voice-mouth | 5d776c1 | conflict | `Cargo.lock` only (R6) |
| 15 | lan-discovery | fe5f0bd | conflict | `Cargo.lock` only (R6) |
| 16 | poseidon | 7d0c478 | clean | - |
| 17 | edge-boot | 753676e | clean | (after ucpu, as required for armv7 seccomp) |
| 18 | common | 26d98b0 | conflict | `desk-hermes/src/jsonout.rs`, `link-watch/src/jsonout.rs`, `galion-one/src/main.rs`, `tuner/src/main.rs` (R7) |
| 19 | real-harness | 5b3c662 | clean | - |
| 20 | launch-gates | 79ee761 | clean | - |

Then two fixup commits on top (F1, F2). `Cargo.lock` auto-merged cleanly for
the other branches; `cargo metadata --locked --offline` accepts the final lock.

## Resolutions

**R1 room / docs-sync.** docs-sync only added a `// repo-check: allow`
comment to a `SystemTime::now()` in the old single-file `main.rs` test
module. room replaced that file (code moved to `lib.rs`, `http.rs`,
`index.rs`, ...), so room's `main.rs` was taken whole. The allow comment
was not re-applied to room's new call sites (repo-check `utc` is a warning).

**R2 hades-approval / ucpu, docs-sync.**
- `.cargo/config.toml`: both branches created it. Kept both sections:
  ucpu's `[env] LIBSQLITE3_FLAGS` (SQLite trim, size budget) and
  hades-approval's `[build] rustflags = ["--cfg",
  "curve25519_dalek_backend=\"serial\""]` (no SIMD backend in
  curve25519-dalek). ucpu's header said "no rustflags"; reworded to say the
  one rustflag is a backend-selection cfg, not a CPU flag (F1 also removes the
  literal words `target-cpu`/`target-feature` from the comment, which
  repo-check's `portable` check flagged).
- `crates/hades-gate/tests/journal.rs`: took hades' new expectation
  (`"DryRun"` instead of `"WouldApply"`) and kept docs-sync's allow comment.

**R3 survival-heal / docs-sync: `docs/STATUS.md`.** survival-heal set WS-SURV
D -> P "becomes D on merge"; on this branch it is merged, so the row is D
with survival-heal's corrected evidence text ("galion-guardian/healer were
dropped by PR #8; the heal path is re-ported onto openmind-survival +
guardian-tick", now "merged on swarm/integration-1, not yet on main"). The
baseline note records both edits. `scripts/status-percent.sh` re-run: D 54,
P 12, N 2, O 21; 60.7 % / 59.0 % weighted (same as docs-sync's figures), and
the computed block was set to that output.

**R4 nerve / gaia-live: openmind-nerve.** As WAVE2.md instructs, nerve's
`main.rs` was taken, and with it nerve's `Cargo.toml` (full NERVE + CORTEX
description, explicit `[lib]`/`[[bin]]`) and `lib.rs` (writer moved to
`writer.rs`, re-exported at the root, so `openmind_nerve::Writer` etc. still
resolve). gaia-live's `serve --listen --log` contract (GET /health with
`"status":"ok"`, POST /event, GET /events) is implemented by nerve's
`server.rs`; proven end to end in scripts/real/integration-1.sh part C.

**R5 desk-linkwatch / docs-sync: `galion-desk/src/main.rs`.** Same shape as
R1: desk-linkwatch moved the code into `lib.rs` and friends; took its
`main.rs`. docs-sync's allow comment for `now()` is not re-applied
(`galion-desk/src/lib.rs:360`, warning only).

**R6 `Cargo.lock` (voice-mouth, lan-discovery).** Took ours, let
`cargo metadata --offline` add the other side's packages, then checked that
the set of (name, version) pairs is exactly the union of both sides (no extra,
no missing, no version drift). Helper: `target/integration-1/lockmerge.sh`
(box only, not committed).

**R7 common / desk-linkwatch, one-probe, voice-mouth, tuner-cutover.**
- `link-watch/src/jsonout.rs`: common's body (re-export of
  `galion_common::jsonout::{json_str, to_json_compat, truthy}`) plus
  desk-linkwatch's new test module (CPython default layout + ensure_ascii).
  desk-linkwatch's non-panicking `to_json_compat` change is already in
  `galion_common::jsonout`.
- `desk-hermes/src/jsonout.rs`: common's file (re-export + the original
  "no python child, no socket" test) plus desk-linkwatch's three unit tests
  as a second module `value_tests` (it imports `serde_json::Value` itself).
- `galion-one/src/main.rs`: kept one-probe's `mod probe; mod services;`,
  dropped `mod sha256;` (common deleted `sha256.rs`; `fn sha256` now calls
  `galion_common::sha256::hex`), and kept both test modules:
  `pairing_tests` (one-probe's fail-closed test) and `common_tests`
  (common's SHA-256 / base64url vectors).
- `tuner/src/main.rs`: common's galion-common HTTP server plus
  tuner-cutover's CLI (`--version`, `--help`, `--check` before binding);
  both doc paragraphs kept. tuner-cutover's live golden fixtures
  (`tests/golden_live.rs`, 22 captured requests) still pass byte for byte on
  the common server.

## Fixups after the merges

**F1 (`c4fcae7`).**
- `openmind-sandbox/examples/escape.rs` (sandbox): `libc::PTRACE_SEIZE` does
  not exist in libc's Android bindings, so `ucpu-gate.sh check` failed for
  `aarch64-linux-android` (sandbox was never checked against ucpu's gate).
  Local `const PTRACE_SEIZE: libc::c_int = 0x4206` on Android only (same
  kernel request number on every Linux ABI); other targets still use libc's.
- ucpu grep gate: two comments said "no target-cpu" / "no target-feature
  flags" (`galion-common/src/lib.rs`, `galion-voice-cpu/Cargo.toml`) in a
  form the gate does not exempt; reworded to "CPU-specific codegen".
- repo-check `hosts` errors (7): public IPv4 literals in `#[cfg(test)]`
  fixtures of openmind-nerve (cortex, logscan, replay) and openmind-discovery
  (admit, cmd, probe). Added `// repo-check: allow (test fixture address,
  never dialled)`; no code change.
- Workflows: ucpu's new `ucpu-tier2`/`ucpu-qemu` jobs and real-harness's
  `realtest.yml` still used `@v4`/`@stable` tags; launch-gates had pinned
  every other `uses:` to a commit SHA. Pinned these to the same SHAs
  (checkout v4.4.0, rust-toolchain stable 2026-10-20, upload-artifact v4.6.2).
- `tests/real/known-failures.txt`: removed the 10 galion-one fail-open
  entries (one-probe is merged); the two live-VPS entries stay.

**F2 (`177dd0c`).** public-export gate 7 (REUSE / comply) failed:
- voice-mouth vendors candle whisper code as `MIT OR Apache-2.0`; added the
  licence text `LICENSES/Apache-2.0.txt` (verbatim Apache-2.0 text, taken from
  the serde 1.0.229 crate's LICENSE-APACHE).
- `gaia-minerva/src/lib.rs` (gaia-live) has a test string that contains an
  SPDX header line, which comply parses as the file's licence; added the file
  to REUSE.toml annotation 2 (override to MIT, same as the other files that
  quote SPDX tags). Gate 8 (cargo deny) needed `cargo fetch --locked` once on
  the box (new crates for other platforms were not cached); no repo change.

## Results on the final head

See docs/real-tests/integration-1.md for the commands and outputs.

## Needs Slavik before this goes to main

1. Merge this branch (owner merges; no PR opened). It is the 20 wave-1
   branches plus the fixups above; the wave-1 owners' own open items still
   apply (each branch's report).
2. `.cargo/config.toml` now has a `[build] rustflags` entry (cfg only,
   selects curve25519-dalek's portable `serial` backend). Any `RUSTFLAGS`
   env var replaces it and `hades-gate/tests/simd_free.rs` then fails.
3. WS-SURV marked D in docs/STATUS.md on this branch (not live: no
   guardian/healer/guardian-tick unit is installed).
4. Public-export gate 10 (warn): 20 public IPv4 literals now (7 at
   launch-gates' run), mostly test fixtures added by nerve and lan-discovery;
   owner decides before any public export.
5. repo-check: 0 errors, 28 `utc` warnings (docs-sync had reached 0; the new
   ones come from room, desk-linkwatch, gaia-live, hades-approval, nerve,
   common, poseidon, lan-discovery, memory-real, survival-heal). Warning
   level only; not changed here.
6. `realtest.yml` CI still runs the stack without `--with-nerve`; locally the
   with-nerve run passes 81/81. Turning it on in CI is a one-line change.
7. gaia-land (slot 01) is not in this branch. gaia-live already merged ws/gaia,
   so expect overlap when gaia-land lands; GAIA-1 stays P.
8. Nothing here is deployed; the VPS stays at 2f5a730.
