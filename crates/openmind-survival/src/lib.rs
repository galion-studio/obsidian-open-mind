// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only survival probes plus pure decision logic (B23 decide, B24
//! degrade, B25 nerve lines, heal-dry patterns). Std only, blocking,
//! single-threaded, portable (no arch-specific code, no C deps).
//!
//! Deliberately excluded: secret handling, process signals, and any unit
//! state change. The only external commands are `systemctl is-active <unit>`
//! and `df -P <path>`, both read-only and run without a shell.

pub mod decide;
pub mod degrade;
pub mod incident;
pub mod nerve;
pub mod patterns;
pub mod probes;

pub use probes::{
    check_cloudflared, check_disk, check_freshness, check_http, check_loadavg, check_meminfo,
    check_unit, http_get, parse_df, parse_loadavg, parse_meminfo, truncate, verdict, HttpOutcome,
    ProbeResult, Runner, SystemRunner,
};

/// Threshold state shared by host stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Pass,
    Warn,
    Crit,
}

/// Warn at 85 percent, crit at 95 percent.
pub fn level_for_percent(p: f64) -> Level {
    if p >= 95.0 {
        Level::Crit
    } else if p >= 85.0 {
        Level::Warn
    } else {
        Level::Pass
    }
}
