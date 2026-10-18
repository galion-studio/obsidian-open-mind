<!-- SPDX-License-Identifier: MIT -->
# Gaia: the universal creator

Gaia is a side project of **Obsidian Open Mind, made by Galion**.

Gaia is the creator of everything. She is Mother Nature as the raw, primordial
force: the pressure that makes mountains, the heat that makes stars, the
selection that makes species. She is not the soft, pastoral version. She
makes, tests and discards without sentiment, and what survives is what fits.

## What Gaia is in this repo

Gaia is the **universal creator model**. It is the part of the stack whose
job is to create new things: new agents, personas, tools, configs and
projects. It turns a seed (a goal, a constraint set, a persona file) into
working artifacts, then lets the rest of the stack (guardian, healer,
repo-check, the tests) decide what lives.

Planned principles:

1. **Seed in, artifact out.** Gaia starts from a declarative seed file and
   produces concrete, versioned output. It never produces unstructured "vibes".
2. **Selection, not sentiment.** Everything Gaia makes must pass the same
   checks as hand-written code: fmt, clippy `-D warnings`, tests, `repo-check`.
   Whatever fails is discarded.
3. **Variation.** Gaia can produce several candidates for one seed and keep
   the fittest, scored by explicit, inspectable criteria.
4. **Lineage.** Every artifact records its seed, its parent, and the
   Europe/Zagreb timestamp it was created at, so its history can be traced.
5. **No money, no secrets.** Gaia never touches payment accounts (the Revolut
   integration is not approved) and never reads or embeds secret values.

## Status

This branch is just the space for Gaia: a concept doc and the build rules.
There's no code yet. The first milestone is a `crates/gaia-core` crate
covering the seed format, the candidate and lineage types, and the
selection trait, with a mock generator and tests.

## Build rules

Gaia follows [UNIVERSAL-CPU.md](../UNIVERSAL-CPU.md), the same rule as every
other workstream:

- Tier 1 (must build): `x86_64-unknown-linux-musl` and
  `aarch64-unknown-linux-musl`.
- Tier 2 (must pass `cargo check`): `aarch64-linux-android` and
  `armv7-unknown-linux-musleabihf`.
- Portable Rust only: no asm, no `target-cpu` or `target-feature` flags, and
  no SIMD intrinsics.
- Minimal dependencies with `default-features = false`, and no `openssl-sys`.
- A size-optimised release profile, so it runs on phones, laptops and small
  chips.

## License

MIT, `Copyright (c) 3000 Galion / galion-studio`. See [LICENSE](../LICENSE).
