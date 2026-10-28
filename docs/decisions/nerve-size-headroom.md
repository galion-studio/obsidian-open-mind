<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Decisions: nerve-size-headroom (wave-4 slot 348, answers D-337-3)

2026-10-28 03:45 CET (+01:00) Europe/Zagreb. Evidence: docs/real-tests/nerve-size-headroom.md.

Done on the branch: openmind-nerve x86_64 musl 2,020,224 -> 1,957,160 B (headroom 76,928 ->
139,992 B) with lld RELR + ICF via crates/openmind-nerve/build.rs and rust-lld for x86_64 musl in
.cargo/config.toml. No nerve-specific cap needed. Nothing deployed.

Owner items:

- **D-348-1 keep `--icf=all`?** Gives ~21 KB. Rust allows folding (functions are `unnamed_addr`);
  all nerve tests and the release-bytes smoke pass. Option: drop it and keep RELR only (headroom
  119,352 B). Recommendation: keep.
- **D-348-2 land order with slot 329.** Both this branch and `swarm/hermes-gateway-2mib` a0d099c2
  add the same `[target.x86_64-unknown-linux-musl] linker = "rust-lld"` block to
  `.cargo/config.toml` (textual conflict only; keep one block). 329's SQLite OMIT/no-unwind C flags
  do not touch nerve (pure Rust).
- **D-348-3 RELR for other static-pie bins.** The same 3-line build.rs would save ~20-50 KB on
  every x86_64 musl bin (e.g. openmind-memory, desk-hermes). Not done here (scope: nerve). Option:
  one shared helper in a later slot.
- **D-348-4 headroom guard.** `real_size_headroom` asserts >= 128 KiB headroom on x86_64. Option:
  wire it into the release recipe/CI after the cortex/nerve pair deploy runbook (slot 357) lands.
