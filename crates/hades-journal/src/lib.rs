// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! One JSON record of a [`hades_gate::decide`] result.
//!
//! [`record`] does not write a file, spawn a process, or use the network.
//! Default decision is dry-run. `kill` and `rotate` are refused.
//! [`Decision::WouldApply`](hades_gate::Decision::WouldApply) is still not an
//! apply: this crate does not run the action.

#![forbid(unsafe_code)]

use hades_gate::Decision;

/// Classify `action` with `token` and return one JSON object.
///
/// Keys are `action`, `decision` (`DryRun`, `WouldApply`, or `Refused`), and
/// `ts` from [`zagreb_time::now_rfc3339`]. Nothing is stored on disk.
#[must_use]
pub fn record(action: &str, token: &str) -> serde_json::Value {
    serde_json::json!({
        "action": action,
        "decision": decision_name(hades_gate::decide(action, token)),
        "ts": zagreb_time::now_rfc3339(),
    })
}

fn decision_name(decision: Decision) -> &'static str {
    match decision {
        Decision::DryRun => "DryRun",
        Decision::WouldApply => "WouldApply",
        Decision::Refused => "Refused",
    }
}
