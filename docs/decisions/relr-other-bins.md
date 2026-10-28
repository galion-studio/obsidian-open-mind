<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Decisions: relr-other-bins (wave-4 slot 368, prep for D-348-3 / D-348-4)

2026-10-28 06:55 CET (+01:00) Europe/Zagreb. Evidence: docs/real-tests/relr-other-bins.md.
Nothing adopted or deployed on this branch: the build is unchanged. Owner decides.

- **D-368-1 adopt RELR + ICF for the other 13 release bins?** x86_64 musl: -529,184 B over 13
  bins (RELR -438,208, ICF -90,976), e.g. hermes-gateway -85,152, openmind-memory -68,576,
  galion-backup -55,680. aarch64: ICF only, -85,216 B. All smokes, SQLite runs and 923 package
  tests pass on the trimmed bytes (1 pre-existing nerve timing failure, F-368-1, same without
  the flags). Options:
  - (a) one block in `.cargo/config.toml`:
    `[target.x86_64-unknown-linux-musl] rustflags = ["--cfg", "curve25519_dalek_backend=\"serial\"", "-C", "link-arg=-zpack-relative-relocs", "-C", "link-arg=--icf=all"]`
    (and the same without RELR for aarch64). Cargo uses target rustflags INSTEAD of
    `[build] rustflags`, so the curve25519 cfg must be repeated (hades-gate `simd_free` test
    catches a miss). Applies to test binaries too (tested here). One full musl rebuild.
  - (b) copy the nerve build.rs to the 12 other packages (opt-out env per package): no
    rustflags change, bins only, but 12 copies.
  - (c) leave as is; recipe-only flags at release time.
  Recommendation: (a), RELR + ICF, after the cortex/nerve pair deploy lands, so the release
  bytes change once.
- **D-368-2 headroom guard: warn or gate?** New `size_headroom_guard` (galion-feature-pin)
  warns per bin under `GALION_HEADROOM_WARN_KIB` (default 128 KiB) and fails only on
  over-budget/unstripped/missing; `GALION_HEADROOM_STRICT=1` turns warnings into failures.
  Today every bin is >= 136 KiB (nerve tightest), so strict at 128 KiB passes. Options: wire it
  into the release recipe/CI after `real_release_repro` (same `<work>/out/<triple>/full`
  dir) as warn-only (recommended) or strict.
- **D-368-3 hermes-gateway 2 MiB.** RELR+ICF alone leaves the x86_64 gateway 15,856 B over
  2 MiB (cap in size-budget.sh is still 2.5 MiB). Measure together with slot 353 16bfc28d
  before lowering the cap.
