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

Updated 2026-10-18 21:50 CEST (+02:00).

The first milestone is in. Gaia is modelled on the GAIA system of *Horizon
Zero Dawn* (inspiration only; see the design doc for the trademark note):

- **Design:** [docs/gaia-design.md](docs/gaia-design.md) covers the HZD
  scenario in our own words, the nine subordinate functions, how the game
  shows them, and the mapping of every Obsidian Open Mind system to exactly one
  function, with mermaid diagrams of the hierarchy, the signal-tower bus and the
  lifecycle.
- **Code:** `crates/gaia-core` (orchestrator, `SubordinateFunction` trait,
  `System` ownership table, bus, Alpha registry, master override, lifecycle,
  containment, lineage), nine function crates (`gaia-hades`, `gaia-minerva`,
  `gaia-apollo`, `gaia-hephaestus`, `gaia-artemis`, `gaia-poseidon`,
  `gaia-aether`, `gaia-demeter`, `gaia-eleuthia`) and the `gaia` CLI
  (`status`, `tick --n N`, `systems`, `dispatch`,
  `simulate-faro [--full-power] [--approve ALPHA]`, all JSON).
- **Safety:** simulation only, with no network, no processes, no file writes
  and no secrets. HADES produces plans only: it works in dry-run and refuses
  `Execute` without an Alpha's approval token. A function that acts outside
  its own systems is contained at once and its changes are discarded. There is
  no "Omega" clearance, and real-money access is refused.
- **Checks:** 71 tests pass; fmt and clippy `-D warnings` are clean;
  repo-check is clean on the gaia crates; tier 1 musl builds pass (x86_64
  587,160 B, aarch64 512,920 B); tier 2 `cargo check` passes for Android and
  armv7.

Next milestone: real seed files to candidates to selection by the repo checks
(fmt, clippy, tests, repo-check) with scoring and lineage, and wiring HEPHAESTUS
to openmind-sandbox behind the same review.

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
