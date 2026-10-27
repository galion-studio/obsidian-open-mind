// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Android admin console (skeleton, wave-4 slot 307).
//!
//! The phone is a **browser client only** (decision t272u: OS-agnostic, no
//! Android host, no flash; Galion OS plan §11 handset gate still closed).
//! This service sits on loopback behind the Rust console / Cloudflare and
//! gives the phone:
//!
//! * pairing with a short one-time code issued on the server terminal
//!   (`issue-code`), stored only as a SHA-256 hash, 5 minutes by default,
//!   single use, global failure lockout;
//! * a device token in an `HttpOnly; Secure; SameSite=Strict` cookie, stored
//!   only as a SHA-256 hash; the device is known by a node id
//!   (`node-<8 hex>`), never IMEI / serial / phone number (plan §8);
//! * a hash-chained JSONL audit log (design from galion-os `galion-admin`);
//! * a read-only status page: nerve `/state` summary and console health.
//!
//! Everything is std plus `galion-common` (in-repo HTTP, SHA-256, time) and
//! `serde_json` for the nerve summary.

pub mod audit;
pub mod pairing;
pub mod server;
pub mod status;

use std::io::Read;

/// `n` bytes from the kernel CSPRNG (`/dev/urandom`, present on Linux and
/// Android). Fails closed: no fallback to a weak source.
pub fn random_bytes(n: usize) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open("/dev/urandom")?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b)?;
    Ok(b)
}

/// Seconds since the unix epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
