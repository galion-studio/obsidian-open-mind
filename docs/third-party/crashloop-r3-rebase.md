<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: crashloop-r3-rebase (wave-4 slot 381)

No new dependency beyond the slot-355 port. Same table as
`docs/third-party/hermes-rs-crashloop-r3.md`:

| Name | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| libc | 0.2.191 (already in Cargo.lock on b7fbbc78; only the hermes-tools -> libc edge is new) | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | `statvfs`/`access(W_OK)` in `hermes_tools::config::write_problem` |

No code copied. Note: the harness of `real_crashloop_r3.rs` was adapted from
`real_offhost_keyrot.rs` (slot 340, same repo, MIT); that file lives on the
hermes-offhost branches, which are NOT part of this port.
