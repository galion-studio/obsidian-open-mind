<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# vendor/safetensors-mit — clean-room MIT safetensors reader/writer

Galion-written (slot 295, 2026-10-27) replacement for the Apache-2.0-only `safetensors` 0.7.0 crate.
Package name is `safetensors` **only** so the workspace `[patch.crates-io]` entry can swap it in under
candle-core / candle-nn 0.9.2 without touching their sources. It is not the Hugging Face crate.

- Licence: MIT (this directory). Dependencies: `serde` + `serde_json` (both `MIT OR Apache-2.0`, taken
  under MIT, already in the workspace lock). `#![forbid(unsafe_code)]`.
- Written from the public file-format description (8-byte LE header length, JSON header, raw buffer).
  The upstream crate's public item names/signatures were read only to stay call-compatible with candle;
  no upstream code, tests or comments were copied.
- API subset: `tensor::{SafeTensors, TensorView, TensorInfo, Metadata, View, Dtype, SafeTensorError,
  serialize, serialize_to_file}`, `slice::{IndexOp, TensorIndexer, SliceIterator, InvalidSlice}`.
- Stricter than upstream in one place: duplicate JSON keys are rejected (`DuplicateName`) instead of
  last-wins, closing a parser-differential trick. `__metadata__` keys are written sorted (upstream
  writes HashMap order), so serialization is deterministic.

Tests: `tests/format.rs` (round trip, exact wire header, 30+ hostile headers, 500 random slices vs a
naive reference, 20 000 mutated files without panic). Real-model parity and voice end-to-end evidence:
`docs/real-tests/safetensors-mit-alt.md`. Decision memo: `docs/security/safetensors-mit-alt.md`.

Rollback: delete the `safetensors = { path = "vendor/safetensors-mit" }` line in the root `Cargo.toml`
`[patch.crates-io]`, run `cargo update -p safetensors`, and restore the AP-1 exception in `deny.toml`.
