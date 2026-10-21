<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# hades-gate SIMD guard (Gaia S5.4, slot 209, 2026-10-21 +02:00)

## Problem

`hades-gate` signs and verifies approvals with `ed25519-dalek` 2.2 →
`curve25519-dalek` 4.1.3. On x86_64 that crate picks its AVX2/AVX-512 SIMD
backend unless the **target rustflags** carry
`--cfg curve25519_dalek_backend="serial"`. The workspace sets that in
`.cargo/config.toml` `build.rustflags`, but each of these silently replaces the list:

* `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS`
* `CARGO_TARGET_<TRIPLE>_RUSTFLAGS`, `target.<triple>.rustflags` / `target.'cfg(..)'.rustflags` config
* building from outside the repo with `--manifest-path` (cargo looks for
  `.cargo/config.toml` from the **cwd**, not from the manifest)

Before this change only `tests/simd_free.rs` noticed, and only when someone ran
`cargo test`. A `cargo build --release` went through. Measured on base 0d68f46
(x86_64 musl release, 2026-10-21 20:24 +02:00): `RUSTFLAGS=""` → exit 0, dalek
build script `curve25519_dalek_backend="simd"`, binary 647,888 B with 1,704 `ymm`
instructions (2 of them `vpermd`/`vpmuludq ymm`); normal build → `serial`,
619,216 B, 42 `ymm` (std/memchr) and 0 `vpermd`/`vpmuludq ymm`.
The live VPS CPU has `avx2` (read 20:30 +02:00), so such a binary would run the SIMD path.

`curve25519-dalek` 4.1.3 has **no cargo feature** for the backend (features:
alloc, default, group, group-bits, legacy_compatibility, precomputed-tables), so
the "dalek feature selection" option in plans/gaia.md S5.4 does not exist; the cfg is the only switch.

## Fix (this branch)

1. `crates/hades-gate/build.rs` (std only) reads the target cfg cargo reports
   (`CARGO_CFG_CURVE25519_DALEK_BACKEND`). Anything other than `serial` (unset,
   `simd`, `fiat`, ...) fails the build with:

   ```
   error: hades-gate: SIMD guard (project rule 1, no SIMD): the target rustflags do not carry
   --cfg curve25519_dalek_backend="serial" (backend cfg not set, target x86_64-unknown-linux-musl).
   Without it curve25519-dalek builds its AVX2/AVX-512 backend on x86_64. ...
   Fix: unset them, or append --cfg curve25519_dalek_backend="serial" to them.
   ```

   It names the overriding env var when cargo lets the build script see it
   (`CARGO_TARGET_<TRIPLE>_RUSTFLAGS`: yes; `RUSTFLAGS`: cargo hides it from
   build scripts, so the message then lists every possible source). Names only,
   never values.
2. `src/lib.rs` backstop: `#[cfg(not(any(doc, doctest, curve25519_dalek_backend = "serial")))] compile_error!(..)`
   for builds that skip build.rs (plain rustc, other build systems). rustdoc is
   exempt because it reads `build.rustdocflags`, not `build.rustflags`; the deps
   it links are still built by rustc with the cfg.
3. The check logic lives in `build_guard.rs`, `include!`d by build.rs and by
   `tests/rustflags_guard.rs` (6 unit tests), so the exact build check is tested.

Normal builds are unchanged (x86_64 musl release binary 619,216 B, same as base).
Overrides that keep the cfg (e.g. `RUSTFLAGS='-C debuginfo=0 --cfg curve25519_dalek_backend="serial"'`) still pass.

The guard is **strict on every target**: on aarch64/armv7 dalek would pick
`serial` anyway without the cfg (measured), but the build still fails, so config
drift is caught on all arches. See decision D-209-1.

## Slavik SIMD ruling: options (owner decision, not decided here)

| Id | Option | For | Against |
|---|---|---|---|
| D-209-1a | **Keep serial, strict guard on all targets (this branch)** | One rule everywhere; drift caught on ARM builds too; zero code-size cost | An ARM build with a custom RUSTFLAGS must append the cfg even though dalek is serial there anyway |
| D-209-1b | Keep serial, guard x86_64 only (`target_arch = "x86_64"` in both checks) | Only fails where SIMD would really come back | ARM drift goes unnoticed until someone builds x86_64 |
| D-209-2 | Allow dalek SIMD (exception to rule 1 for crypto): drop the cfg + guard | Faster Ed25519 verify on AVX2 hosts (VPS has avx2); dalek detects AVX2 at runtime and falls back to serial | Breaks rule 1 ("no SIMD"); two code paths per CPU; adds curve25519-dalek-derive proc-macro to the build; hades-gate verifies a handful of tokens, so speed does not matter |
| D-209-3 | Replace ed25519-dalek with an Ed25519 crate that has no SIMD backend at all (e.g. ed25519-compact, MIT, pure Rust) | No cfg, no guard, no fragility; build from anywhere | New crypto dep to review (strict verification / malleability parity with `verify_strict`, zeroize), lock churn; needs a slot + licence/third-party doc |
| D-209-4 | Escape hatch `--cfg hades_allow_simd` to bypass the guard (benchmarks) | Lets someone measure SIMD on purpose | Makes the rule optional; not recommended |
| D-209-5 | Same guard for `edge/openmind-node` (standalone workspace, ed25519-dalek 2 default features, no guard): measured 20:26 +02:00 `cargo check --manifest-path` from /tmp → `simd`; from inside the repo → `serial`. `install/linux.sh` / `termux.sh` build from `$OMNODE_SRC` when set, which can be outside the repo | Closes the same hole on the edge node | Separate task (out of row 209 scope; repo file only change there) |

Recommendation: D-209-1a now (cheap, no runtime change), D-209-5 next, D-209-3
only if the cfg keeps causing trouble.
