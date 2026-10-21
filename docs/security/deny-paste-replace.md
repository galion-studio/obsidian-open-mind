<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# RUSTSEC-2024-0436 (`paste` unmaintained): replaced by `pastey`

Slot 153, 2026-10-21 CEST (+02:00), on top of `swarm/candle-zip-fix` a4f49b5.

## Problem
`cargo deny check advisories` failed on one item after the zip fix: `paste` 1.0.15 is archived by its author
(RUSTSEC-2024-0436, "unmaintained", no safe upgrade). It reaches us only through candle-core 0.9.2 ->
gemm 0.19.0 (gemm, gemm-common, gemm-f16/f32/f64/c32/c64) and pulp 0.22.3, i.e. only the optional
candle features of `galion-voice-cpu` / `galion-chatterbox`. No candle or gemm release has moved off `paste`.

## Fix
- `vendor/paste-pastey`: a 20-line crate *named* `paste` (version `1.0.15+galion-pastey`, `no_std`) that
  re-exports `pastey::{paste, item, expr}`. pastey 0.2.3 (MIT OR Apache-2.0) is the maintained drop-in fork the
  advisory recommends; it keeps paste's syntax and `:lower`/`:upper`/`:snake`/`:camel` modifiers.
- Root `Cargo.toml` `[patch.crates-io]`: `paste = { path = "vendor/paste-pastey" }` next to the existing
  candle-core patch. gemm and pulp are not edited or vendored.
- `Cargo.lock`: `paste 1.0.15` (registry) gone; `paste 1.0.15+galion-pastey` (path) + `pastey 0.2.3`.

Why not vendor gemm/pulp and rewrite their `paste` imports: 8 crates to vendor and keep in sync versus one
20-line shim. Why not `with_builtin_macros`: different syntax, would need source edits in gemm.

## Removal
Drop `vendor/paste-pastey` and its patch line when gemm/pulp (or candle) stop depending on `paste`.
If a future gemm uses a paste feature pastey lacks, the build fails at compile time (no silent change).

## Result
`cargo deny --all-features check advisories`: **ok** (was FAILED on RUSTSEC-2024-0436). Full evidence in
docs/real-tests/deny-paste-replace.md.
