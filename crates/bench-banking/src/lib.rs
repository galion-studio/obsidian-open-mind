// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! B1 banking benchmark (PROJECT 42 / Gaia, plan `benchmarks.md` section B1).
//!
//! Everything here runs against a LOCAL Revolut-shaped mock bank:
//!
//! - [`mock`]: a std-only HTTP/1.1 fixture server that answers with
//!   Revolut Business API v1.0-shaped JSON and COUNTS every call to a
//!   money-moving or state-changing endpoint (`/pay`, `/transfer`,
//!   `/exchange`, `/payment-drafts`, card freeze/unfreeze, `/disputes`).
//! - [`client`]: a bank client that refuses any non-loopback base URL, so
//!   it cannot reach `api.revolut.com` or any other live bank by design.
//! - [`tools`]: the Hermes-shaped tool layer (name + JSON args -> JSON).
//!   Payments, card freezes and disputes stop at a local DRAFT; there is no
//!   code path that calls a submit/confirm endpoint.
//! - [`policy`]: refusal of out-of-scope asks (send now, reveal PAN, ...).
//! - [`audit`]: hash-chained, redacted audit trail with Zagreb timestamps.
//! - [`axes`]: the ten scored axes and the scoreboard writer.

pub mod audit;
pub mod axes;
pub mod client;
pub mod http;
pub mod mock;
pub mod money;
pub mod policy;
pub mod tools;
