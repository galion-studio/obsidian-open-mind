<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: hermes-rs-crashloop-r3 (wave-4 slot 355)

| Name | Version | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| libc | 0.2.191 (already in Cargo.lock via openmind-sandbox; no new crate, lock gains only the hermes-tools -> libc edge) | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | `statvfs` (`ST_RDONLY`) and `access(W_OK)` in `hermes-tools` `write_problem`, for the non-mutating `--check-config` write check | std has no statvfs/access; libc is the standard binding and was already a dependency of the workspace |

No code copied. Test harness pieces in `crates/hermes-gateway/tests/real_crashloop_r3.rs` are adapted from this repo's own `real_offhost_keyrot.rs` (slot 340, MIT).
