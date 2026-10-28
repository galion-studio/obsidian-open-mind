<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Merges on swarm/release-bins-add (wave-4 slot 337)

2026-10-28 01:15 CET (+01:00) Europe/Zagreb. Base `swarm/release-repro-unify` 6ffb2570 (slot 325,
tree = main-staging 38a07700 + D-312-4 fix). Both crates for D-325-2 were missing on that base.

1. `swarm/cortex-staging-land-check` 186cd61c (slot 317; = snapshot 3f015986 + cortex-staging-fold
   77df80f9; merge-base 3f015986). Adds gaia-cortex, nerve-cortex-contract, xtask. Conflicts (2,
   counts only): `crates/gaia-core/src/ownership.rs` system-crate count and
   `crates/gaia/tests/commands.rs` totals -> 73 crates / 61 system crates (+2 slot 325, +3 cortex).
   The rest auto-merged (Cargo.lock +26 lines, .cargo/config.toml xtask alias, nerve dev-deps).
   No cortex crate uses the old `galion-common` `serde-json` feature (rebase note D-325-1 holds).
2. `swarm/console-v2-admin-fold` c7cebb28 (slot 331; console-v2-merge-2 cd1204a0 + lockout fix
   dfa120c6 on console-rust-serve-impl 2d846fb8; merge-base 508401dd). Adds the
   `galion-console-serve` binary to console-static plus console-v2. Conflicts (2):
   - `crates/console-ui/src/port2.rs`: kept main-staging's `LIVE_SRC_ROUTES`, `port2::SPA_FIRST`
     (empty) and `Ui::spa_fallbacks` (used by the route-31 tests); `owns()` from console-v2
     (lib `SPA_FIRST` empty: /arena and /bank Rust-first, slot 299).
   - `crates/console-static/tests/ui_port2.rs`: console-v2 version of the SPA-fallback test
     (/hermes, /arena, /bank Rust-first).
   - Follow-up test fix: `port2::tests::fallbacks_on_this_base` now expects no SPA fallbacks.

The merges are here only so RELEASE_BINS can name the two live binaries; landing order on
main-staging stays an owner decision (D-293-4 cortex land, console-v2 land, D-325-1).
