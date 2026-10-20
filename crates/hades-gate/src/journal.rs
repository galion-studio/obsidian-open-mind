// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! One JSON record of a [`crate::decide`] result.
//!
//! [`record`] does not write a file, spawn a process, or use the network.
//! Default decision is dry-run. `kill` and `rotate` are refused.
//! [`Decision::WouldApply`](crate::Decision::WouldApply) is still not an
//! apply: this crate does not run the action.

#![forbid(unsafe_code)]

use crate::Decision;

/// Classify `action` with `token` and return one JSON object.
///
/// Keys are `action`, `decision` (`DryRun`, `WouldApply`, or `Refused`), and
/// `ts` from [`zagreb_time::now_rfc3339`]. Nothing is stored on disk.
#[must_use]
pub fn record(action: &str, token: &str) -> serde_json::Value {
    serde_json::json!({
        "action": action,
        "decision": decision_name(crate::decide(action, token)),
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
