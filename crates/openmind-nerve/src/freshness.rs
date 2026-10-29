// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/state` freshness (slot 448, nerve-state-ts-staleness, 2026-10-30 CET +01:00).
//!
//! `/state` `ts` is the time of the last engine cycle, not the time of the
//! request. In event mode (`NERVE_MODE=event`, the live mode) a quiet nerve
//! runs one cycle per liveness heartbeat (`NERVE_EVT_HEARTBEAT`, default
//! 900 s), so `ts` can sit still for up to ~15 min **by design**. Live VPS
//! read-only probe 2026-10-29 23:49-23:50 CET: `/state` ts held at 23:45:31
//! for 3 probes over 40 s while `/health` ts moved with the clock and the
//! nerve was healthy (score 96, cycle 20).
//!
//! A reader that judges the nerve by "`ts` older than N minutes" therefore
//! raises a false "nerve down" on a quiet, healthy nerve. This module makes
//! the nerve publish its own staleness limit instead of every reader guessing:
//!
//! - `/state` gains `freshness = {max_age_s, stale_after, stale_after_unix,
//!   basis}`: past `stale_after` with no newer cycle, the nerve itself says
//!   its state loop is late (for example the event runtime hung while the
//!   HTTP thread still answers).
//! - `/health` gains `state_ts`, `state_age_s`, `state_max_age_s` and
//!   `state_fresh` (computed at request time, `null` before the first cycle).
//!
//! The limit is automatic (event mode: heartbeat + max_wait + margin;
//! polling: 3 intervals + margin) and can be pinned with
//! `NERVE_STATE_MAX_AGE=<seconds>` (10..=86400; unset, `auto` or `0` = auto).
//! Purely additive: no existing field changes meaning, nothing new runs.

use serde_json::{json, Value};

/// Slack for one slow cycle (probes + journal scan) on top of the schedule.
pub const MARGIN_S: u64 = 60;
/// Lowest accepted `NERVE_STATE_MAX_AGE`.
pub const MIN_S: u64 = 10;
/// Highest accepted `NERVE_STATE_MAX_AGE`.
pub const MAX_S: u64 = 86_400;

/// How the nerve schedules its cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Polling mode: one cycle every `interval_s`.
    Poll { interval_s: u64 },
    /// Event mode: a heartbeat cycle every `heartbeat_s`; an event cycle
    /// waits at most `max_wait_s` after the first event of a burst.
    Event { heartbeat_s: u64, max_wait_s: u64 },
}

impl Schedule {
    /// Longest normal gap between two cycles plus [`MARGIN_S`].
    pub fn auto_max_age(self) -> u64 {
        match self {
            Schedule::Poll { interval_s } => interval_s.max(1).saturating_mul(3) + MARGIN_S,
            Schedule::Event {
                heartbeat_s,
                max_wait_s,
            } => heartbeat_s.max(1) + max_wait_s + MARGIN_S,
        }
    }

    pub fn basis(self) -> &'static str {
        match self {
            Schedule::Poll { .. } => "poll: 3 x interval + margin",
            Schedule::Event { .. } => "event: heartbeat + max_wait + margin",
        }
    }
}

/// Effective limit from `NERVE_STATE_MAX_AGE` (`get` = env lookup).
/// Returns `(seconds, basis)`; a bad value is an error (the nerve refuses to
/// start rather than silently guessing).
pub fn max_age_from_env(
    get: &dyn Fn(&str) -> Option<String>,
    sched: Schedule,
) -> Result<(u64, &'static str), String> {
    match get("NERVE_STATE_MAX_AGE").as_deref().map(str::trim) {
        None | Some("") | Some("auto") | Some("0") => Ok((sched.auto_max_age(), sched.basis())),
        Some(v) => match v.parse::<u64>() {
            Ok(n) if (MIN_S..=MAX_S).contains(&n) => Ok((n, "NERVE_STATE_MAX_AGE")),
            _ => Err(format!(
                "NERVE_STATE_MAX_AGE must be auto or {MIN_S}..={MAX_S} seconds, got {v:?}"
            )),
        },
    }
}

/// The `/state` `freshness` object for a cycle at `now` (unix seconds).
pub fn state_block(now: u64, max_age_s: u64, basis: &str) -> Value {
    let after = now.saturating_add(max_age_s);
    json!({
        "max_age_s": max_age_s,
        "stale_after": crate::util::ts(after),
        "stale_after_unix": after,
        "basis": basis,
    })
}

/// `/health` fields: age of the last cycle and whether it is within limits.
/// `state_unix == 0` (no cycle yet) or `max_age_s == 0` (not configured, e.g.
/// a library user) give `null` for the verdict.
pub fn health_fields(state_ts: &Value, state_unix: u64, max_age_s: u64, now: u64) -> Value {
    if state_unix == 0 {
        return json!({"state_ts": Value::Null, "state_age_s": Value::Null,
            "state_max_age_s": max_age_s, "state_fresh": Value::Null});
    }
    let age = now.saturating_sub(state_unix);
    let fresh = if max_age_s == 0 {
        Value::Null
    } else {
        Value::Bool(age <= max_age_s)
    };
    json!({"state_ts": state_ts, "state_age_s": age, "state_max_age_s": max_age_s, "state_fresh": fresh})
}

/// Reader helper: is this `/state` body stale at `now` (unix seconds)?
/// `None` when the body carries no `freshness` block (an older nerve): the
/// caller must then fall back to its own rule, and should allow at least the
/// event-mode default of 900 + 120 + 60 s before calling the nerve late.
pub fn is_stale(state: &Value, now: u64) -> Option<bool> {
    state["freshness"]["stale_after_unix"]
        .as_u64()
        .map(|after| now > after)
}

/// Fallback limit for readers of an older nerve with no `freshness` block:
/// the live event-mode defaults (heartbeat 900 s, max_wait 120 s).
pub const LEGACY_EVENT_DEFAULT_S: u64 = 900 + 120 + MARGIN_S;

#[cfg(test)]
mod tests {
    use super::*;

    fn env(v: Option<&'static str>) -> impl Fn(&str) -> Option<String> {
        move |k| {
            (k == "NERVE_STATE_MAX_AGE")
                .then(|| v.map(String::from))
                .flatten()
        }
    }

    #[test]
    fn auto_limits_cover_the_schedule() {
        let live = Schedule::Event {
            heartbeat_s: 900,
            max_wait_s: 120,
        };
        assert_eq!(live.auto_max_age(), 1080);
        assert_eq!(live.auto_max_age(), LEGACY_EVENT_DEFAULT_S);
        assert_eq!(Schedule::Poll { interval_s: 30 }.auto_max_age(), 150);
        assert_eq!(Schedule::Poll { interval_s: 0 }.auto_max_age(), 63);
        // The 14 min stall seen in slot 420 (20:10:13 held ~840 s) is normal.
        assert!(840 <= live.auto_max_age());
    }

    #[test]
    fn env_override_is_bounded() {
        let s = Schedule::Poll { interval_s: 30 };
        for v in [None, Some(""), Some("auto"), Some("0"), Some(" auto ")] {
            assert_eq!(max_age_from_env(&env(v), s).unwrap().0, 150, "{v:?}");
        }
        assert_eq!(
            max_age_from_env(&env(Some("12")), s).unwrap(),
            (12, "NERVE_STATE_MAX_AGE")
        );
        assert_eq!(max_age_from_env(&env(Some("86400")), s).unwrap().0, 86_400);
        for bad in ["9", "86401", "-5", "1e3", "fast", "12s"] {
            assert!(
                max_age_from_env(&env(Some(bad)), s).is_err(),
                "{bad} accepted"
            );
        }
    }

    #[test]
    fn state_block_and_reader_agree() {
        let b = state_block(1_000, 1_080, "x");
        assert_eq!(b["stale_after_unix"], 2_080);
        assert!(b["stale_after"].as_str().unwrap().contains("+0"));
        let st = json!({"ts": "t", "freshness": b});
        assert_eq!(is_stale(&st, 1_900), Some(false));
        assert_eq!(is_stale(&st, 2_080), Some(false));
        assert_eq!(is_stale(&st, 2_081), Some(true));
        assert_eq!(is_stale(&json!({"ts": "t"}), 99_999), None);
    }

    #[test]
    fn health_fields_track_age() {
        let ts = json!("2026-10-29T23:45:31+01:00");
        let h = health_fields(&ts, 1_000, 1_080, 1_261);
        assert_eq!(h["state_age_s"], 261);
        assert_eq!(h["state_fresh"], true);
        assert_eq!(h["state_ts"], ts);
        assert_eq!(
            health_fields(&ts, 1_000, 1_080, 2_081)["state_fresh"],
            false
        );
        // no cycle yet
        let h = health_fields(&Value::Null, 0, 1_080, 5);
        assert!(h["state_fresh"].is_null() && h["state_age_s"].is_null());
        // not configured
        assert!(health_fields(&ts, 1_000, 0, 1_001)["state_fresh"].is_null());
        // clock went backwards: age 0, still fresh
        assert_eq!(health_fields(&ts, 1_000, 60, 900)["state_age_s"], 0);
    }
}
