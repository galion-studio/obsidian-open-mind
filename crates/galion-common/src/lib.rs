// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-common: the helpers that used to be copied crate to crate.
//!
//! Everything here is std only, portable scalar code (no asm, no SIMD, no
//! CPU-specific codegen), and total: malformed input returns an error, never a panic.
//!
//! - [`sha256`]: FIPS 180-4 SHA-256 (NIST vectors in the tests) and [`hex`].
//! - [`base64`]: RFC 4648 standard and URL-safe alphabets, encode and decode.
//! - [`json`]: JSON string escaping (CPython `ensure_ascii` compatible), and
//!   with feature `serde-json` the `jsonout` CPython `json.dumps` writer.
//! - [`time`]: Europe/Zagreb RFC 3339 with an explicit offset, never `Z`.
//! - [`http`]: strict HTTP/1.x request parser, a bounded thread-per-connection
//!   server (deadlines, size limits, `Connection: close`) and a small client.

#![forbid(unsafe_code)]

pub mod base64;
pub mod hex;
pub mod http;
pub mod json;
pub mod sha256;
pub mod time;

#[cfg(feature = "serde-json")]
pub mod jsonout;
