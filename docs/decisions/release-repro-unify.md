<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Decision: per-package release builds are byte-identical to the release set (D-312-4)

Slot 325, branch `swarm/release-repro-unify`, base main-staging 38a07700 (tree snapshot 3f015986,
tree 4864589f), 2026-10-27 (Europe/Zagreb, CET +01:00). No deploy.

## Problem

Slot 312 found that `cargo build --release -p hermes-gateway` (or `-p openmind-nerve`, or the pair)
gives different bytes than the same binary built inside the 12-package release set. Cargo (resolver 2)
unifies dependency features only over the packages selected on the command line
(`resolver.feature-unification = "selected"`), so a shared dependency is compiled with different
`--cfg feature=...` flags, gets a different metadata hash and different code.

`cargo tree -e normal,build` on 38a07700, each release binary alone vs the full set, found 5 sources
(x86_64 and aarch64 musl alike, 15 mismatching package lines per target):

| Dependency | Alone | In the release set | Who adds it |
|---|---|---|---|
| galion-common | `default` | `default,serde-json` | desk-hermes, link-watch (`features = ["serde-json"]`) |
| indexmap | (none) | `default,std` | toml_edit (via openmind-config); serde_json `preserve_order` alone adds no features |
| rusqlite | `bundled,modern_sqlite` | `+backup` | galion-backup, openmind-memory |
| serde_core | `alloc,std` (serde_json only) / `result,std` (serde only) | `alloc,result,std` | serde_json adds `alloc`, serde adds `result` |

Affected bins: nerve, hermes-gateway, memory, queue, galion-one, galion-desk, tuner, desk-hermes
(and the nerve+gateway pair). galion-health, link-watch, console-static and galion-backup were
already stable.

## Prior art (research first, rules: no reinventing)

- **Unstable cargo `-Zfeature-unification` / `resolver.feature-unification = "workspace"`** does
  exactly this, but is nightly-only on cargo 1.99.1 (`warning: ignoring resolver.feature-unification
  without -Zfeature-unification`, checked on the box; listed under Unstable Features in the Cargo
  Book, https://doc.rust-lang.org/cargo/reference/unstable.html#feature-unification). Not usable.
- **cargo-hakari workspace-hack** (guppy-rs, MIT OR Apache-2.0,
  https://github.com/guppy-rs/guppy/tree/main/tools/hakari): a generated crate every member depends
  on, listing the union features. It unifies over the *whole* workspace (voice/candle crates
  included) and adds every shared dep to every member, which would grow small bins and their build.
  We use the **pattern only**, hand-kept and minimal. No code or tool copied, no new dependency.
- **The repo's own precedent**: `swarm/serde-order-pin` (docs/decisions/serde-order-pin.md) pinned
  serde_json `preserve_order` once in `[workspace.dependencies]` for the same reason.

## Fix (converge, prefer the smaller side)

1. **galion-common has no features.** The serde_json `jsonout` writer (CPython `json.dumps`) moved
   unchanged to a new crate `crates/galion-jsonout`; desk-hermes and link-watch depend on it
   (their `jsonout.rs` re-exports are unchanged). tuner, galion-one etc. never compile serde_json
   through galion-common again, alone or in the set (the "alone" bytes win: galion-one is now the
   same sha256 as the old solo build).
2. **rusqlite** is a workspace dependency with `bundled, backup`; the five users say
   `{ workspace = true }`. Same sizes: LTO drops the unused backup API.
3. **serde** workspace features `derive, alloc`, so every serde user gets serde_core `alloc,result,std`.
4. **crates/galion-feature-pin** (no code, `#![no_std]`, empty lib) depends on `indexmap` (default,
   the toml_edit set) and `serde`. The 5 release bins that reach serde_json without toml
   (openmind-nerve, hermes-gateway, openmind-memory, openmind-queue, desk-hermes) depend on it. It
   lists only crates those bins already link, so no new code enters any binary.

No new third-party dependency (Cargo.lock: only the two new workspace crates).

## Guard

`crates/galion-feature-pin/tests/release_feature_unify.rs` (runs in every `cargo test`, ~1 s,
`cargo tree --offline --locked`, no build): for x86_64 and aarch64 musl, every release binary alone,
plus three partial sets, must have exactly the full-set feature line for every package; and
galion-common may not get `[features]` or optional deps back. On the base it fails with the
30 mismatches above (negative control). Adding a feature to a shared dependency in one member now
fails CI with the package and both feature sets named.

`crates/galion-feature-pin/tests/real_release_repro.rs` (`GALION_REAL=1`): the byte-level proof
(full set, then every bin alone in its own fresh target dir, plus the nerve+gateway pair, sha256).

## Release recipe impact

The release `-p` set no longer matters: `cargo build --release -p X` gives the release bytes of X.
The slot 312 recipe note "the package set matters" can be dropped once this lands. Bytes of the
8 affected bins change versus 38a07700 builds (same sizes); nothing is deployed.

## Not covered / owner decisions

- D-325-1: land on main-staging (owner). Rebase note: slot 312 `release-artifacts-refresh` and any
  branch adding `features = ["serde-json"]` to galion-common must switch to `galion-jsonout`.
- D-325-2: gaia-cortex (`--features llm-advisor`) and galion-console-serve (branch
  console-rust-serve-impl) are built in separate commands; add them to `RELEASE_BINS` when they
  land on main-staging.
- D-325-3: aarch64 byte proof not run (no C cross compiler / zig on the box, D-318-3); the aarch64
  feature graph is covered by the guard.
- D-325-4: Gaia ownership rows for the two new crates are judgement calls (pending Slavik):
  `galion-feature-pin` at release-artifacts / HEPHAESTUS (alternative cargo-build), `galion-jsonout`
  at zagreb-clock / AETHER next to `galion-common` (alternative core). See docs/gaia-ownership.md.
