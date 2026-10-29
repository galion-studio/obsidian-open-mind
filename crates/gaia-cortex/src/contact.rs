// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real nerve reachability for `GET /health` (slot 347, F-337-1).
//!
//! Before: `/health` said `"nerve": true` whenever a nerve URL was
//! configured, even with nothing listening on the nerve port. Now every
//! place that talks to the nerve records the outcome here (lock-free
//! atomics, shared by the stream subscriber, the pull loop, the decider's
//! action calls and the courier's alert posts):
//!
//! - success: a stream (re)connect answered 200, any stream line (events
//!   and the nerve's 5-minute keepalives), `GET /stats` / `/incidents`,
//!   `/events` pulls, `POST /actions` and `POST /event` that got an answer;
//! - failure: connect refused / timed out, a stream that ended (EOF, reset,
//!   idle timeout), a non-2xx post, a failed pull.
//!
//! `nerve` in `/health` is true only when a nerve is configured, the newest
//! recorded attempt succeeded, and the last success is at most `stale_s`
//! old. So a killed nerve reads `false` at the next failed attempt (the
//! open stream breaks at once), and a hung nerve that keeps the socket
//! open reads `false` once no line arrived for `stale_s`.
//!
//! Zero idle work: nothing here polls; it only records what the existing
//! links already do.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

/// Stream state as seen by the subscriber thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// No stream subscriber (no nerve, or `nerve_stream` off).
    Off = 0,
    /// Connected and reading.
    Connected = 1,
    /// Not connected (between reconnects).
    Down = 2,
}

/// Default staleness with the stream on: the nerve writes a keepalive every
/// 300 s, so two missed keepalives plus margin (= the stream idle timeout,
/// 11 min) mean the nerve is not talking.
pub const STREAM_STALE_S: u64 = 660;

#[derive(Debug, Default)]
pub struct NerveContact {
    /// Unix ms of the last successful contact (0 = never).
    last_ok_ms: AtomicU64,
    /// Unix ms of the last failed attempt (0 = never).
    last_fail_ms: AtomicU64,
    ok: AtomicU64,
    fail: AtomicU64,
    stream: AtomicU8,
    /// What the last failure was (`connect: ...`, `stream ended: ...`).
    /// Never carries secrets: only loopback socket errors and HTTP codes.
    last_error: Mutex<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix ms; every printed time goes through zagreb_time::from_unix)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl NerveContact {
    pub fn ok(&self) {
        self.ok_at(now_ms());
    }

    pub fn ok_at(&self, t_ms: u64) {
        self.last_ok_ms.fetch_max(t_ms, Ordering::Relaxed);
        self.ok.fetch_add(1, Ordering::Relaxed);
    }

    pub fn fail(&self, why: &str) {
        self.fail_at(now_ms(), why);
    }

    pub fn fail_at(&self, t_ms: u64, why: &str) {
        self.last_fail_ms.fetch_max(t_ms, Ordering::Relaxed);
        self.fail.fetch_add(1, Ordering::Relaxed);
        let w: String = why.chars().take(160).collect();
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = w;
    }

    pub fn set_stream(&self, s: Stream) {
        self.stream.store(s as u8, Ordering::Relaxed);
    }

    pub fn stream(&self) -> Stream {
        match self.stream.load(Ordering::Relaxed) {
            1 => Stream::Connected,
            2 => Stream::Down,
            _ => Stream::Off,
        }
    }

    /// Reachable now: newest attempt succeeded and the last success is
    /// at most `stale_s` old.
    pub fn reachable(&self, configured: bool, now_ms: u64, stale_s: u64) -> bool {
        let (ok, fail) = (
            self.last_ok_ms.load(Ordering::Relaxed),
            self.last_fail_ms.load(Ordering::Relaxed),
        );
        configured && ok > 0 && ok >= fail && now_ms.saturating_sub(ok) <= stale_s * 1000
    }

    /// The `/health` fields: `nerve` (reachable), `nerve_configured`, and
    /// `nerve_contact` with the details.
    pub fn health_fields(&self, configured: bool, now_ms: u64, stale_s: u64) -> Value {
        let ok = self.last_ok_ms.load(Ordering::Relaxed);
        let fail = self.last_fail_ms.load(Ordering::Relaxed);
        let t = |ms: u64| {
            if ms == 0 {
                Value::Null
            } else {
                json!(zagreb_time::from_unix((ms / 1000) as i64))
            }
        };
        let age = |ms: u64| {
            if ms == 0 {
                Value::Null
            } else {
                json!(now_ms.saturating_sub(ms) / 1000)
            }
        };
        let err = self
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let reachable = self.reachable(configured, now_ms, stale_s);
        let state = if !configured {
            "none"
        } else if reachable {
            "ok"
        } else if ok == 0 && fail == 0 {
            "unknown"
        } else if ok > 0 && ok >= fail {
            "stale"
        } else {
            "unreachable"
        };
        json!({
            "nerve": reachable,
            "nerve_configured": configured,
            "nerve_contact": {
                "state": state,
                "last_ok": t(ok), "age_s": age(ok),
                "last_fail": t(fail), "fail_age_s": age(fail),
                "last_error": if fail == 0 { Value::Null } else { json!(err) },
                "ok": self.ok.load(Ordering::Relaxed),
                "fail": self.fail.load(Ordering::Relaxed),
                "stream": match self.stream() { Stream::Off => "off", Stream::Connected => "connected", Stream::Down => "down" },
                "stale_s": stale_s,
            }
        })
    }

    /// Overwrite the nerve fields of a `/health` body (fresh or snapshot).
    pub fn overlay(&self, body: &mut Value, configured: bool, stale_s: u64) {
        if let (Some(o), Value::Object(f)) = (
            body.as_object_mut(),
            self.health_fields(configured, now_ms(), stale_s),
        ) {
            o.extend(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_793_000_000_000;

    #[test]
    fn configured_but_never_contacted_is_false() {
        let c = NerveContact::default();
        assert!(!c.reachable(true, T, 660));
        let h = c.health_fields(true, T, 660);
        assert_eq!(h["nerve"], false);
        assert_eq!(h["nerve_configured"], true);
        assert_eq!(h["nerve_contact"]["state"], "unknown");
        assert_eq!(h["nerve_contact"]["last_ok"], Value::Null);
    }

    #[test]
    fn not_configured_is_none() {
        let c = NerveContact::default();
        c.ok_at(T);
        let h = c.health_fields(false, T, 660);
        assert_eq!(h["nerve"], false);
        assert_eq!(h["nerve_contact"]["state"], "none");
    }

    #[test]
    fn fresh_success_is_true_with_age() {
        let c = NerveContact::default();
        c.ok_at(T);
        let h = c.health_fields(true, T + 42_000, 660);
        assert_eq!(h["nerve"], true);
        assert_eq!(h["nerve_contact"]["state"], "ok");
        assert_eq!(h["nerve_contact"]["age_s"], 42);
        assert_eq!(h["nerve_contact"]["ok"], 1);
    }

    #[test]
    fn failure_after_success_is_false_at_once() {
        let c = NerveContact::default();
        c.ok_at(T);
        c.fail_at(T + 1, "stream ended: Io(\"eof\")");
        let h = c.health_fields(true, T + 2, 660);
        assert_eq!(h["nerve"], false);
        assert_eq!(h["nerve_contact"]["state"], "unreachable");
        assert_eq!(
            h["nerve_contact"]["last_error"],
            "stream ended: Io(\"eof\")"
        );
        // recovery: next success flips it back
        c.ok_at(T + 5_000);
        assert!(c.reachable(true, T + 5_001, 660));
    }

    #[test]
    fn old_success_goes_stale() {
        let c = NerveContact::default();
        c.ok_at(T);
        assert!(c.reachable(true, T + 660_000, 660));
        assert!(!c.reachable(true, T + 660_001, 660));
        assert_eq!(
            c.health_fields(true, T + 700_000, 660)["nerve_contact"]["state"],
            "stale"
        );
    }

    #[test]
    fn same_ms_success_wins_and_times_never_go_back() {
        let c = NerveContact::default();
        c.fail_at(T, "connect");
        c.ok_at(T);
        assert!(c.reachable(true, T, 1));
        // an older success arriving late (another thread) keeps the newer one
        c.ok_at(T - 10_000);
        assert_eq!(c.health_fields(true, T, 660)["nerve_contact"]["age_s"], 0);
    }

    #[test]
    fn overlay_replaces_configured_only_flag() {
        let c = NerveContact::default();
        let mut body = json!({"ok": true, "nerve": true});
        c.overlay(&mut body, true, 660);
        assert_eq!(body["nerve"], false);
        assert_eq!(body["ok"], true);
        assert!(body["nerve_contact"].is_object());
    }
}
