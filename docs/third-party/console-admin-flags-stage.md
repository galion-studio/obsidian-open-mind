<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: console-admin-flags-stage (wave-4 slot 313)

No new dependencies. `Cargo.toml` and `Cargo.lock` are unchanged.

| Item | Licence | Source | Use | Why |
|---|---|---|---|---|
| `crates/console-static/tests/units/mod.rs` | MIT (in-tree) | adapted from `crates/galion-backup/tests/units/mod.rs` (slot 269, `swarm/backup-rust-unit`, same project) | test-only systemd unit reader | Same `[Section]`/`Key=Value`/list-reset rules. Added: backslash line continuation (the console ExecStart spans lines), argv split, path-list coverage. About 120 lines of std, so no crate is needed. |
