// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Shared helpers for the `real_port*` live-room tests (slot 364).
//!
//! - [`addr`]: loopback address `base + off`, where `base` comes from
//!   `CONSOLE_REAL_PORT_BASE` (so a swarm slot can run the test inside its own
//!   port range) and defaults to the port the test always used.
//! - [`Shape`]: which console API shape the live VPS `:8080` answers for one
//!   path. Since the Rust cutover (`galion-console-serve --dist --local-api`)
//!   the old Node paths (`/api/hermes/*`, `/api/v1/instances`, `/api/mesh`,
//!   ...) answer 404; the rooms must then say so honestly instead of the test
//!   comparing data that no longer exists (F-349-1).

#![allow(dead_code)]

use std::sync::OnceLock;

/// Env var that moves every port of a `real_port*` test (`base+0..base+9`).
pub const PORT_BASE_ENV: &str = "CONSOLE_REAL_PORT_BASE";

/// Parse a port base: `1024..=65525` so `base+9` still fits.
pub fn parse_base(s: &str) -> Option<u16> {
    s.trim()
        .parse::<u16>()
        .ok()
        .filter(|b| (1024..=65525).contains(b))
}

/// The port base in use (`CONSOLE_REAL_PORT_BASE` or `default`). A set but
/// invalid value panics instead of silently falling back to a shared port.
pub fn base(default: u16) -> u16 {
    static B: OnceLock<u16> = OnceLock::new();
    *B.get_or_init(|| match std::env::var(PORT_BASE_ENV) {
        Ok(v) => parse_base(&v)
            .unwrap_or_else(|| panic!("{PORT_BASE_ENV}={v:?} is not a port base 1024..=65525")),
        Err(_) => default,
    })
}

/// `127.0.0.1:<base+off>` as a `'static` string (built once per offset).
pub fn addr(default: u16, off: u16) -> &'static str {
    static A: OnceLock<Vec<&'static str>> = OnceLock::new();
    let all = A.get_or_init(|| {
        let b = base(default);
        (0..10)
            .map(|i| &*Box::leak(format!("127.0.0.1:{}", b + i).into_boxed_str()))
            .collect()
    });
    all[off as usize]
}

/// What the live console answered for one API path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// 200: the data route exists; compare the room with the data.
    Data,
    /// 404: the route is not served by the live (Rust) console; the room
    /// must render its honest "did not load / HTTP 404" state.
    Retired,
    /// Anything else (401/5xx/0): reported, the room must still not crash.
    Other(u16),
}

pub fn shape(code: u16) -> Shape {
    match code {
        200 => Shape::Data,
        404 => Shape::Retired,
        c => Shape::Other(c),
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn base_parse() {
        assert_eq!(parse_base("22340"), Some(22340));
        assert_eq!(parse_base(" 22340\n"), Some(22340));
        assert_eq!(parse_base("80"), None);
        assert_eq!(parse_base("65530"), None);
        assert_eq!(parse_base("x"), None);
        assert_eq!(shape(200), Shape::Data);
        assert_eq!(shape(404), Shape::Retired);
        assert_eq!(shape(401), Shape::Other(401));
    }
}
