<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: hermes-offhost-keyrot (wave-4 slot 340)

No new dependencies and no copied code. `Cargo.toml` and `Cargo.lock` are
unchanged. The rotation window reuses the slot 314 HMAC (std over
`galion_common::sha256`), `galion_common::time::require_rfc3339` and
`hermes_rs::time::parse_rfc3339`. Key delivery uses systemd's own
`LoadCredential=` (no secret-manager crate). The real test uses std,
`serde_json` (MIT/Apache-2.0, already used) and `hermes-rs`; its harness is
adapted from `real_offhost_f1f2.rs` in this repo (MIT).
