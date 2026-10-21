// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! POSEIDON for Gaia: compute resources and budgets.
//!
//! * [`engine`]: the queue state machine. Priorities, leases with visibility
//!   timeouts and fencing tokens, retries with exponential backoff, a
//!   dead-letter queue, per-agent quotas and spend caps that hard-stop.
//! * [`store`]: the durable append-only log (checksummed lines, torn-tail
//!   repair, compaction). The engine is event-sourced: every change is a
//!   [`engine::Record`] that is written to the log before it is applied.
//! * [`vram`]: the VRAM-pool registry and the owner's capacity goal of at
//!   least 1500 GB of VRAM. It is a model only: it never spends anything.
//! * [`gaia`]: how these map onto the four POSEIDON systems.
//! * [`http`], [`server`]: a small std-only HTTP/1.1 service and client.
//! * [`auth`]: bearer-token auth for non-loopback peers (shared secret in a
//!   mode-0600 file); loopback needs no token unless configured.
//!
//! Spend is tracked as integers only. Nothing here calls a paid API, moves
//! money or talks to a GPU provider.

pub mod auth;
pub mod engine;
pub mod gaia;
pub mod http;
pub mod server;
pub mod store;
pub mod vram;

/// Milliseconds since the Unix epoch (wall clock, so leases survive restarts).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A Europe/Zagreb RFC 3339 stamp with an explicit offset, from epoch ms.
pub fn zagreb(ms: u64) -> String {
    zagreb_time::from_unix((ms / 1000) as i64)
}

/// Like [`zagreb`] but with milliseconds, e.g. `2026-10-20T19:00:00.123+02:00`.
pub fn zagreb_ms(ms: u64) -> String {
    let s = zagreb(ms);
    if s.len() > 19 {
        format!("{}.{:03}{}", &s[..19], ms % 1000, &s[19..])
    } else {
        s
    }
}
