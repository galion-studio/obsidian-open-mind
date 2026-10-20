// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only survival probes and a pure healer decision function.
//!
//! Probes are blocking and single-threaded. [`decide`] only returns
//! suggestions: it does not spawn a process or open a socket. When a
//! service probe is down, the suggestion names the other services that
//! still work and the services that are paused.
//!
//! Deliberately excluded: secret handling, process signals, and any unit
//! state change. The only external commands are `systemctl is-active <unit>`
//! and `df -P <path>`, both read-only and run without a shell.

pub mod decide;
pub mod incident;
pub mod probes;

pub use decide::{decide, DegradationRule, Escalation, Suggestion, DEGRADATION_RULES};
pub use probes::{
    check_cloudflared, check_disk, check_freshness, check_http, check_loadavg, check_meminfo,
    check_unit, parse_df, parse_loadavg, parse_meminfo, ProbeResult, Runner, SystemRunner,
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
