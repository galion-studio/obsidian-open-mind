<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Swarm 9 core/rust-hygiene, 2026-11-15 +01:00

Lane: rust-hygiene. Decision: pin the compiler. Do not rewrite
`slice::as_chunks`, `is_multiple_of`, or `File::lock`.

## Choice

The workspace floor is rustc **1.89.0**.

`galion-common` uses `is_multiple_of` and `slice::as_chunks` (stable in
1.88). `hades-gate` uses `File::lock` (stable in 1.89,
<https://github.com/rust-lang/rust/issues/130994>). Locked crates.io
dependencies already declare at most rust-version 1.88.0 (`opus-pure`
0.2.2, `zip` 9.0.5). Removing `as_chunks` would not lower the floor.

## Measurements, 2026-11-15 +01:00

Commands used `--locked --offline --ignore-rust-version` and a private
`CARGO_TARGET_DIR`, so the new `rust-version` field did not hide the
compiler error. Host rustc for the green runs is
`rustc 1.89.0 (29483883e 2025-08-04)`.

| Toolchain | Command | Result |
|---|---|---|
| 1.83.0 (90b35a623 2024-11-26) | `cargo +1.83.0 check -p galion-common` | exit 101. Six E0658: `unsigned_is_multiple_of` in `crates/galion-common/src/base64.rs:73` and `src/hex.rs:21`; `slice_as_chunks` in `src/hex.rs:24`, `src/sha256.rs:63`, `src/sha256.rs:86` (`as_chunks_mut`), `src/sha256.rs:95` |
| 1.87.0 | `cargo +1.87.0 check -p galion-common` | exit 101. Four E0658, all `slice_as_chunks` (the same four sites). `is_multiple_of` compiled |
| 1.88.0 (6b00bc388 2025-06-23) | `cargo +1.88.0 check -p galion-common` | exit 0, finished in 0.90s |
| 1.88.0 | `cargo +1.88.0 check -p hades-gate` | exit 101. E0658 `file_lock` at `crates/hades-gate/src/ledger.rs:93` and `:138` (`file.lock()`) |
| 1.89.0 | `cargo test --workspace --locked` | exit 0 (see Local runs) |

## What is pinned

- `rust-toolchain.toml` channel `"1.89.0"`, components `clippy` and `rustfmt`.
  This is the file rustup and `dtolnay/rust-toolchain` read when a workflow
  step does not pass `toolchain:`.
- Root `Cargo.toml` `rust-version = "1.89.0"`.
- All 75 members under `crates/*/Cargo.toml` set
  `rust-version.workspace = true` and `[lints] workspace = true`.
- `edge/openmind-node` is its own empty workspace, so it cannot inherit.
  Its `Cargo.toml` sets `rust-version = "1.89.0"` and
  `undocumented_unsafe_blocks = "warn"` directly.
- `[workspace.lints.clippy] undocumented_unsafe_blocks = "warn"`.
  `cargo clippy -D warnings` turns that into an error. The comment clippy
  accepts is a `// SAFETY:` line immediately above the `unsafe` keyword.
- `[workspace.lints.rust] unexpected_cfgs` moved here. A member with
  `[lints] workspace = true` cannot also declare `[lints.rust]`.

## CI

`toolchain: stable` was removed from the `dtolnay/rust-toolchain` steps in:

- `.github/workflows/self-eval.yml`
- `.github/workflows/launch-gates.yml`
- `.github/workflows/openmind-desktop.yml`
- `.github/workflows/realtest.yml`
- `.github/workflows/release.yml`
- `.github/workflows/sbom.yml`

The action is still pinned to commit `b725eda525362e7cf34af9a7243cac1e3f01a7ec`.
The self-eval clippy job reads `channel` out of `rust-toolchain.toml` and
fails unless `rustc --version` contains that version. `RELEASE-CHECKLIST.md`
item 1.8 is marked done (2026-11-15T15:40:00+01:00).

`.github/workflows/miri.yml` still passes `toolchain: nightly-2026-10-20`.
Miri is not on the MSRV channel. The comment in `rust-toolchain.toml` says so.

## Clippy on 1.89

`cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
exited 0 on rustc 1.89.0. The same command for
`edge/openmind-node/Cargo.toml` exited 0. Fixes that 1.89 clippy required:

- `nonminimal_bool`: `Option::is_none_or` in `crates/galion-vending/src/sim.rs`,
  `crates/gaia-cortex/src/rules.rs`, `crates/hermes-bench/src/live.rs`, and
  `edge/openmind-node/src/doctor.rs`. `crates/openmind-queue/src/vram.rs`
  rewritten to `is_empty() || !provider_approved`.
- `clippy::const_is_empty`: allow on the const fallback assert in
  `crates/galion-common/src/build_info.rs`. `crates/console-ui/src/port2.rs`
  asserts `SPA_FIRST.len() == 0`.
- `clippy::unnecessary_cast`: allow on the `statvfs` widening in
  `crates/openmind-sandbox/src/lib.rs` (the count is `u64` on this host and
  `u32` on other targets).
- `crates/galion-one/src/main.rs` allows `unknown_lints` above
  `manual_is_multiple_of` and `manual_checked_ops`. Those names do not exist
  in 1.89 clippy, and `-D warnings` treats an unknown lint as an error.

`cargo fmt --all -- --check` and the edge fmt check both exited 0.

## Unsafe audit

`vendor/` is excluded from the workspace and was not edited. First-party
blocks that clippy rejected, or whose `// SAFETY:` comment was not on the
line immediately above `unsafe`, were updated:

- `crates/galion-backup/src/backup.rs`, `src/tarw.rs` (`statvfs`, `passwd`,
  `group` zeroed then filled by libc).
- `crates/galion-guard/tests/real_netns.rs` (`getppid`).
- `crates/galion-voice-cpu/src/kitten/mod.rs` (read-only mmap of weights).
- `crates/hermes-rs/tests/heap_growth.rs` (counting `GlobalAlloc` in front of
  the system allocator).
- `crates/openmind-nerve/src/evt/sys.rs` (`OwnedFd::from_raw_fd` moved so the
  existing comment sits on the `unsafe` keyword).
- `crates/openmind-sandbox/src/lib.rs` and `src/treecopy.rs`.
- `edge/openmind-node/src/sandbox.rs` and `src/seccomp.rs`. The seccomp
  comment had to be `// SAFETY:` exactly; `// SAFETY (all probes):` does not
  satisfy the lint.

Tests added next to those blocks:

- `avail_kb_of_tmp_is_positive` (`galion-backup`).
- `counting_allocator_round_trip` (`hermes-rs`).
- `getppid` probe at the start of `real_netns` (runs even when
  `GALION_REAL` is unset; the rest of that binary still skips).
- `kitten::tests::missing_weights_are_an_error`. The module is behind the
  `kitten` feature, so `cargo test --workspace` does not build it. Run with
  `cargo test -p galion-voice-cpu --features kitten --lib -- missing_weights`.
  It passed. Clippy with `--features kitten -D warnings` also passed.
  `Result::unwrap_err` is not used: `KittenTts` does not implement `Debug`.
- `rust_version_matches_toolchain_file` embeds `Cargo.toml` and
  `rust-toolchain.toml` with `include_str!`. A runtime `File::open` fails
  the galion-common Miri pass (isolation has no `open`).

## Local runs, rustc 1.89.0

- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  exit 0.
- `cargo clippy --manifest-path edge/openmind-node/Cargo.toml --all-targets --locked --offline -- -D warnings`:
  exit 0.
- `cargo test --workspace --locked`: exit 0. No test result line reported a
  failure. `tests/release_feature_unify.rs`: 5 passed. `tests/msrv_pin.rs`:
  passed. `avail_kb_of_tmp_is_positive` and `counting_allocator_round_trip`
  passed. `real_netns` printed `skipped (set GALION_REAL=1)` and the process
  exited 0, which means the `getppid` assert before that print held.
  Musl targets `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`
  were installed and both graphs were fetched, including
  `RUSTFLAGS= cargo fetch --locked --target x86_64-unknown-linux-musl`,
  matching the self-eval test job.
- `cargo test --manifest-path edge/openmind-node/Cargo.toml --locked --offline`:
  exit 0. Library 88 passed, `real_denylist_parity` passed,
  `real_first_boot_pairing_restart` passed.

## Miri, nightly-2026-10-20

`rustc 1.101.0-nightly (07e526db9 2026-10-19)`. Same dated nightly as
`.github/workflows/miri.yml`. Short skip list from that workflow
(`fips_180_2_appendix_b`, `nist_additional_vectors`, `old_copies_agree`,
`every_switch_1996_to_2100`, `coarse_sweep_and_labels`, `stamp_checks_agree`).

- `cargo +nightly-2026-10-20 miri setup`: exit 0.
- `cargo +nightly-2026-10-20 miri test --locked -p galion-common --lib`:
  exit 0. 21 passed, 2 ignored (TCP socket, realtime clock), 386.78s.
- `cargo +nightly-2026-10-20 miri test --locked -p galion-common --tests`:
  exit 0 after the `include_str!` fix. `rust_version_matches_toolchain_file`
  passed in 0.22s. `nist` and `time_vs_chrono` were filtered by the short
  skip (0 run, 3 filtered each). The other integration binaries passed or
  ignored themselves (real-life / socket). The first attempt, before
  `include_str!`, aborted in `msrv_pin` because Miri isolation has no `open`.
- `cargo +nightly-2026-10-20 miri test --locked -p hades-gate -p openmind-schema`:
  exit 0. hades-gate lib 23 passed in 158.31s. openmind-schema lib 4 passed,
  7 ignored (filesystem and the realtime clock). No failure.

## Left stale on purpose

This lane does not edit `LICENSE`, `README.md`, or `STATUS.md`, and it does
not edit the historical snapshots. They still say the workspace has no
toolchain pin. That was true on 2026-10-18 and is not true after this change.
For Maciej:

- `README.md` lines 128-130: "does not pin a version yet (no `rust-version`,
  no `rust-toolchain.toml`); it is developed on current stable."
- `release/FACTS.md` row "No Rust toolchain version is pinned in the repo".
- `release/brand/CLAIMS.md` claim 25, re-verified 2026-10-18T19:09:10+02:00
  as `NO_RUST_TOOLCHAIN_FILE` / `NO_RUST_VERSION_KEY`.
- `release/repo/README.md` and `release/repo/RELEASE-CHECKLIST.md` item 1.8
  (still "open" in that snapshot). The live checklist item 1.8 is done.

`vendor/` unsafe was not audited. It is not a workspace member.
