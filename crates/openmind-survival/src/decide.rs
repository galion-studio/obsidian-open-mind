// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Pure healer decisions.
//!
//! [`decide`] reads probe results and returns suggestions. It does not spawn
//! a process, open a socket, or change unit state. Callers may print the
//! text. They must not treat it as a command.

use crate::ProbeResult;

/// How far a suggestion escalates. Nothing here is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escalation {
    /// Probe passed. Keep watching.
    Observe,
    /// Degraded host or stale file. Tell the operator. Do not suggest a restart.
    Warn,
    /// A unit, tunnel, or service looks down. Suggest a restart; do not perform it.
    SuggestRestart,
}

impl Escalation {
    /// Stable token: `observe`, `warn`, or `suggest-restart`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Warn => "warn",
            Self::SuggestRestart => "suggest-restart",
        }
    }
}

/// One suggestion for one probe. `text` is for a person, never a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub level: Escalation,
    pub text: String,
}

/// Map each probe to one suggestion, in the same order.
///
/// Host pressure (`mem`, `load`, `disk:`), stale files (`fresh:`), and
/// unreadable samples stay at [`Escalation::Warn`]. A failed unit, tunnel,
/// or other service is [`Escalation::SuggestRestart`]. A passing probe is
/// [`Escalation::Observe`].
#[must_use]
pub fn decide(results: &[ProbeResult]) -> Vec<Suggestion> {
    results.iter().map(suggest_one).collect()
}

fn suggest_one(r: &ProbeResult) -> Suggestion {
    let level = escalate(r);
    let text = match level {
        Escalation::Observe => observe_text(&r.name),
        Escalation::Warn => warn_text(r),
        Escalation::SuggestRestart => restart_text(&r.name),
    };
    Suggestion {
        name: r.name.clone(),
        level,
        text,
    }
}

fn escalate(r: &ProbeResult) -> Escalation {
    if r.ok {
        Escalation::Observe
    } else if !matches!(host_kind(&r.name), Host::Other) || r.detail.contains("unparseable") {
        Escalation::Warn
    } else {
        Escalation::SuggestRestart
    }
}

enum Host {
    Memory,
    Load,
    Disk,
    Fresh,
    Other,
}

fn host_kind(name: &str) -> Host {
    if name == "mem" {
        Host::Memory
    } else if name == "load" {
        Host::Load
    } else if name.starts_with("disk:") {
        Host::Disk
    } else if name.starts_with("fresh:") {
        Host::Fresh
    } else {
        Host::Other
    }
}

fn observe_text(name: &str) -> String {
    let s = subject(name);
    format!("{s} ok: keep watching")
}

fn warn_text(r: &ProbeResult) -> String {
    let unreadable = r.detail.contains("unparseable");
    let crit = r.detail.contains("crit");
    match host_kind(&r.name) {
        Host::Fresh => "freshness failed: operator may check the writer service".to_owned(),
        Host::Memory if unreadable => "memory unreadable: operator may check the source".to_owned(),
        Host::Memory if crit => {
            "memory critical: operator may stop an idle heavy service".to_owned()
        }
        Host::Memory => "memory high: operator may stop an idle heavy service".to_owned(),
        Host::Load if unreadable => "load unreadable: operator may check the source".to_owned(),
        Host::Load if crit => "load critical: operator may pause a batch job".to_owned(),
        Host::Load => "load high: operator may pause a batch job".to_owned(),
        Host::Disk if unreadable => "disk unreadable: operator may check the source".to_owned(),
        Host::Disk if crit => "disk critical: operator may free space or rotate logs".to_owned(),
        Host::Disk => "disk high: operator may free space or rotate logs".to_owned(),
        Host::Other if unreadable => "probe unreadable: operator may check the source".to_owned(),
        Host::Other => "probe degraded: operator may look, not restart".to_owned(),
    }
}

fn restart_text(name: &str) -> String {
    if name == "cloudflared" {
        return "cloudflared absent: operator may restart the tunnel".to_owned();
    }
    let s = safe_token(name);
    if name.starts_with("unit:") {
        format!("{s} not active: operator may restart {s}")
    } else {
        format!("{s} down: operator may restart {s}")
    }
}

fn subject(name: &str) -> String {
    match host_kind(name) {
        Host::Memory => "memory".to_owned(),
        Host::Load => "load".to_owned(),
        Host::Disk => "disk".to_owned(),
        Host::Fresh => "freshness".to_owned(),
        Host::Other => safe_token(name),
    }
}

/// Drop path-like or blank tokens so suggestion text never names a file.
fn safe_token(name: &str) -> String {
    let raw = name.strip_prefix("unit:").unwrap_or(name);
    if raw.is_empty() || raw.contains('/') || raw.chars().any(char::is_whitespace) {
        "service".to_owned()
    } else {
        raw.to_owned()
    }
}
