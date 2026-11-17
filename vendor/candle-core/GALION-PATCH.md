<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Galion patch notice for this copy of candle-core 0.11.0

This directory is `candle-core` 0.11.0 as published on crates.io
(https://crates.io/crates/candle-core/0.11.0, upstream https://github.com/huggingface/candle,
path `candle-core`), licence MIT OR Apache-2.0.
The original copyright and licence (`LICENSE`, Apache-2.0 text as shipped in the crate) are kept.

Rebased 2026-11-15 from the 0.9.2 vendored copy so `candle-nn` 0.11.0 and this crate are one
`candle-core`. The same two patches are reapplied. candle-core 0.11.0 still requires `zip` 8.6.0,
and zip 8 is still inside RUSTSEC-2026-0367, -0368 and -0369 (fixed in zip >= 9.0.0):

1. `Cargo.toml`, `Cargo.toml.orig`: `zip` requirement 8.6.0 -> 9.0.5 (still `default-features = false`).
2. `src/npy.rs` (2 places): `ZipFile::name()` returns `ZipResult<Cow<str>>` in zip 9; add `?`.
3. `src/pickle.rs` (1 place): `ZipArchive::file_names()` yields `ZipResult<Cow<str>>` in zip 9; collect into a
   `Result` and add `?`.
4. Removed the upstream Python test-data generators `tests/npy.py`, `tests/pth.py` (repo rule: no Python files;
   the generated fixtures shipped in the crate are kept). The `*.license` sidecars from the 0.9.2 copy are kept
   beside the fixtures that still have the same names.

5. SIMD kernels removed, so the copy obeys the project's no-SIMD rule (UNIVERSAL-CPU.md) and does not contain
   `core::arch` (repo-check `portable`, `scripts/ucpu-gate.sh grep`):
   - deleted `src/cpu/{avx,neon,simd128}.rs` and `src/quantized/{avx,neon,simd128}.rs`;
   - `src/cpu/mod.rs`: dropped the `mod`/`use` lines and the SIMD variants of `vec_dot_f32`, `vec_sum`,
     `vec_dot_f16`, `vec_dot_bf16`, `vec_add_f16`, `vec_add_bf16`, `vec_scalar_add_f16`, `vec_scalar_add_bf16`;
     the upstream portable variants are kept with their bodies unchanged;
   - `src/quantized/k_quants.rs`: dropped the 26 `#[cfg(target_feature = ...)] return super::<simd>::...;`
     early returns, so each `vec_dot` always calls its upstream `vec_dot_unopt` (or the portable `vec_dot_4`
     / `from_float` tail), and dropped the AArch64 `dotprod` `BlockQ4Kx8` pack/matmul (`target_feature`
     is itself a grep-gate hit);
   - `src/quantized/mod.rs`: dropped the three `mod` lines and the `dotprod` Q4K fast path;
   - `src/utils.rs`: `with_avx`, `with_neon`, `with_simd128`, `with_f16c` return `false`.
   The GGUF tokenizer (`quantized/tokenizer.rs`) is behind the `tokenizers` feature, off by default.
   That dependency enables regex's full `unicode` feature, which Cargo then compiles into every
   binary of a workspace build and put `galion-guard` over 2 MiB. Nothing in this workspace calls it.
   On x86_64 musl (no AVX2 in the baseline target) this is the code upstream already compiled. On aarch64
   the scalar paths replace NEON (same maths, slower).

Each source change carries a `galion candle-zip-fix` comment. Remove this directory and the
`[patch.crates-io]` entry in the workspace `Cargo.toml` once a candle release requires zip >= 9.
