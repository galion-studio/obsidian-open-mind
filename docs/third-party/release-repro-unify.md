<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party: release-repro-unify (wave-4 slot 325)

Written 2026-10-27 23:10 CET (+01:00) Europe/Zagreb.

**No new outside code and no new crates.io packages.** `Cargo.lock` only gains the two workspace
crates `galion-feature-pin` and `galion-jsonout` (ours, MIT). `rusqlite 0.32` (MIT) and `indexmap 2`
(Apache-2.0 OR MIT) were already in the lockfile; they now come from `[workspace.dependencies]`.

## Prior art read (pattern only, no code copied)

| source | licence | what we took |
|---|---|---|
| cargo-hakari "workspace-hack" (https://github.com/guppy-rs/guppy, `tools/hakari`) | MIT OR Apache-2.0 (checked in its Cargo.toml 2026-10-27) | the idea: one crate that every release bin depends on, which turns on the union of features, so any `-p` subset resolves the same. We keep it hand-written and tiny (2 deps) instead of running the tool. |
| Cargo Book, "Features / Feature unification" and `-Zfeature-unification` / `resolver.feature-unification = "workspace"` | MIT OR Apache-2.0 (rust-lang/cargo) | confirms the cause; the workspace option is nightly-only on cargo 1.99.1 (checked: stable prints an unstable-flag warning), so we cannot rely on it yet. |
| repo precedent serde-order-pin | MIT (ours) | pinning features in `[workspace.dependencies]` |
