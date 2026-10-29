// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-gate: the mandatory sandbox gate between Hermes's work and live.
//!
//! ```text
//! Hermes (tools)  --> Proposal (patch + base commit, sha256)          [proposal]
//!                 --> static security checks on the patch             [checks]
//!                 --> sandbox deploy: throwaway worktree at base,
//!                     git apply, then each plan stage (build, test,
//!                     live-style deploy + smoke) under openmind-sandbox [stage]
//!                 --> PassRecord, Ed25519-signed by the sandbox key    [record]
//!                 --> owner approval: Ed25519 signature over
//!                     (proposal sha256, record sha256, target head)    [apply]
//!                 --> apply: only with a valid record + approval, only
//!                     at the same base commit, to a non-live target in
//!                     this prototype                                   [apply]
//! ```
//!
//! Model tiers ([`tier`]): deep reasoning (writing the proposal, reviewing a
//! failed record) goes to the high tier; routine steps (summaries, status
//! lines) go to the low-cost tier; the gate itself never calls a model.
//!
//! Prototype: never pushes, never deploys, and [`apply::apply`] refuses
//! every path under the live roots (`/opt/galion`, `/etc`, `/root`, ...).

pub mod apply;
pub mod checks;
pub mod git;
pub mod proposal;
pub mod record;
pub mod stage;
pub mod tier;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("bad hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

/// Europe/Zagreb RFC 3339 stamp (the house format).
pub fn now_stamp() -> String {
    hermes_rs::time::zagreb_rfc3339(hermes_rs::time::now_secs())
}
