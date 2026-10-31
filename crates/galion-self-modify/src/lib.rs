// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Hermes self-change approval core (Slavik-approved design, 2026-10-31).
//!
//! Four signatures, each by a different key held by a different account:
//!
//! 1. **stack** ([`records::stack_sign`], openmind-hermes): origin proof over
//!    `proposal.json` + `change.patch`. Never signs a pass.
//! 2. **test record** ([`records::sign_test`], galion-sm-sign, NEW sandbox
//!    key): the signer re-hashes the artifact and logs written by the test
//!    account (which holds no key) and verifies the stack signature first.
//! 3. **dry-run record** ([`records::sign_dryrun`], same signer): bound to
//!    the test record and the same artifact fingerprint.
//! 4. **owner** ([`owner`], Slavik's device): `{artifact sha256, base commit,
//!    one-time nonce, expiry <= 30 min}` (+ id and both record hashes), as a
//!    WebAuthn/passkey ES256 assertion or a plain Ed25519 key-file signature.
//!
//! [`deploy`] (root) verifies all four plus the fingerprint, burns the nonce,
//! backs up, installs a drop-in, restarts ONCE, health-checks, rolls back on
//! failure and sends an ntfy alert either way. The signer side never runs a
//! process (see the `signer_never_spawns` test). Formats: `FORMATS.md`.

pub mod deploy;
pub mod owner;
pub mod records;
pub mod util;

pub use hermes_rs::self_modify::{hex, key_id, sha256_hex, unhex};
