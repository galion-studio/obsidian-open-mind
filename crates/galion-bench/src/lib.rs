// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-bench`: the shared harness for the banking (B1), Wall Street (B2)
//! and vending (B3) benchmarks.
//!
//! * [`fixture`]: scripted request -> response fixtures, one JSON object per
//!   line (JSONL). Format: `docs/bench/harness.md`.
//! * [`server`]: a std-only blocking mock HTTP server that answers from those
//!   fixtures and keeps a journal of every hit with a Europe/Zagreb timestamp.
//! * [`client`]: a tiny std-only HTTP/1.1 client for tests to hit the mock.
//! * [`score`]: [`Scorecard`], equal-weight axes averaged to 0-100.
//!
//! Safety: the mock binds loopback only unless explicitly told otherwise, has
//! no outbound networking at all, and never forwards to a real bank, broker or
//! supplier. Fixtures are recorded / hand-written data, never live calls.

pub mod client;
pub mod fixture;
pub mod http;
pub mod score;
pub mod server;

pub use client::{request, Response};
pub use fixture::{load_jsonl, parse_jsonl, Fixture, FixtureError, FixtureSet};
pub use score::Scorecard;
pub use server::{Hit, MockServer};

/// Europe/Zagreb RFC 3339 "now" (explicit offset, never `Z`).
pub fn now_zagreb() -> String {
    zagreb_time::now_rfc3339()
}
