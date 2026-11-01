// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Mullvad-style anonymous accounts for the whole Galion platform
//! (galion.app, Galion One, the console, Hermes). Owner request t547u-t550u,
//! 2026-11-01 CET (+01:00). Design: `DESIGN.md` in this crate.
//!
//! * An account is a random 16-digit number ([`number`]). There is no email,
//!   no username, no password. The server stores only a slow keyed argon2id
//!   hash of it ([`hash`]) and a paid-until time ([`store`]).
//! * Login is the number only; sessions are random cookies whose SHA-256 is
//!   stored ([`session`]); failed logins are rate limited in memory only, with
//!   no IP written to disk ([`ratelimit`]).
//! * Payments sit behind one trait ([`pay::PaymentAdapter`]): Stripe
//!   ([`pay::stripe`]), cash by post ([`pay::cash`]) and a crypto stub
//!   ([`pay::crypto`]). Processors only ever see an opaque payment reference.
//! * Other services ask one small internal API whether a session is entitled
//!   ([`server`], [`client`]).

pub mod client;
pub mod hash;
pub mod hmac;
pub mod number;
pub mod page;
pub mod pay;
pub mod plans;
pub mod ratelimit;
pub mod rng;
pub mod server;
pub mod session;
pub mod store;

/// Unix seconds now.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Constant-time byte comparison (length is not secret).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b) {
        d |= x ^ y;
    }
    d == 0
}
