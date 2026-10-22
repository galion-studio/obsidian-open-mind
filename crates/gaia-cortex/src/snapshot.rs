// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Read snapshots for the API (slot 236, T5.1 follow-up F5).
//!
//! The decider holds the engine lock for a whole batch, including nerve
//! HTTP calls (up to 3 x 3 s per subject against a hung nerve). API reads
//! must not wait for that: every read route first tries the lock and, when
//! the decider holds it, answers from the snapshot published after the last
//! batch, marked `busy: true` with its age. The snapshot lives outside the
//! engine lock (shared `Arc`), so reading it never touches the engine.
//!
//! Invalid push items counted while the engine is busy go to an atomic
//! counter and are folded into `Stats::events_invalid` by the next batch.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Read routes served from the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Health,
    Stats,
    Incidents,
    Decisions,
    Alerts,
    Escalations,
}

#[derive(Debug, Clone)]
struct Snap {
    at: Instant,
    health: Value,
    stats: Value,
    incidents: Value,
    /// Newest first, the whole ring (<= 200).
    decisions: Value,
    alerts: Value,
    escalations: Value,
}

/// Shared between the engine (writer, under its lock) and the API (reader,
/// never under the engine lock).
#[derive(Debug, Default)]
pub struct Snapshot {
    inner: Mutex<Option<Snap>>,
    invalid_pending: AtomicU64,
    published: AtomicU64,
    /// The decider is inside a batch (plan, nerve calls, commit).
    busy: AtomicBool,
}

/// One snapshot read: the body and how old it is.
#[derive(Debug, Clone)]
pub struct Read {
    pub body: Value,
    pub age_ms: u64,
}

impl Snapshot {
    /// Store fresh views. Called by the engine with its lock held.
    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        health: Value,
        stats: Value,
        incidents: Value,
        decisions: Value,
        alerts: Value,
        escalations: Value,
    ) {
        let s = Snap {
            at: Instant::now(),
            health,
            stats,
            incidents,
            decisions,
            alerts,
            escalations,
        };
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
        self.published.fetch_add(1, Ordering::Relaxed);
    }

    /// How many times a snapshot was stored.
    /// Set by the decider around each batch (slot 247: the engine lock is
    /// no longer held across the nerve calls, so "busy" is this flag).
    pub fn set_busy(&self, b: bool) {
        self.busy.store(b, Ordering::Relaxed);
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    pub fn published(&self) -> u64 {
        self.published.load(Ordering::Relaxed)
    }

    /// The last stored view of `route` (`n` = newest-first cap for list
    /// routes). `None` before the first publish.
    pub fn read(&self, route: Route, n: usize) -> Option<Read> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let s = g.as_ref()?;
        let take = |v: &Value| match v {
            Value::Array(a) => Value::Array(a.iter().take(n).cloned().collect()),
            other => other.clone(),
        };
        let body = match route {
            Route::Health => s.health.clone(),
            Route::Stats => s.stats.clone(),
            Route::Incidents => s.incidents.clone(),
            Route::Decisions => take(&s.decisions),
            Route::Alerts => take(&s.alerts),
            Route::Escalations => s.escalations.clone(),
        };
        Some(Read {
            body,
            age_ms: s.at.elapsed().as_millis() as u64,
        })
    }

    /// Count invalid push items without the engine lock.
    pub fn add_invalid(&self, n: u64) {
        self.invalid_pending.fetch_add(n, Ordering::Relaxed);
    }

    /// Invalid items counted since the last [`Snapshot::take_invalid`].
    pub fn pending_invalid(&self) -> u64 {
        self.invalid_pending.load(Ordering::Relaxed)
    }

    /// Move the pending invalid count out (the engine adds it to its stats).
    pub fn take_invalid(&self) -> u64 {
        self.invalid_pending.swap(0, Ordering::Relaxed)
    }
}

/// Mark a snapshot body as stale: objects get `busy: true` and
/// `snapshot_age_ms`; arrays are returned as they are (the HTTP headers
/// carry the same information).
pub fn mark_busy(mut v: Value, age_ms: u64) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.insert("busy".into(), json!(true));
        o.insert("snapshot_age_ms".into(), json!(age_ms));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_until_published_then_sliced() {
        let s = Snapshot::default();
        assert!(s.read(Route::Stats, 10).is_none());
        s.store(
            json!({"ok": true}),
            json!({"events_in": 3}),
            json!({}),
            json!([1, 2, 3, 4]),
            json!([]),
            json!({"open": []}),
        );
        assert_eq!(s.published(), 1);
        assert_eq!(s.read(Route::Decisions, 2).unwrap().body, json!([1, 2]));
        assert_eq!(s.read(Route::Stats, 2).unwrap().body["events_in"], 3);
    }

    #[test]
    fn invalid_counter_moves_once() {
        let s = Snapshot::default();
        s.add_invalid(2);
        s.add_invalid(3);
        assert_eq!(s.pending_invalid(), 5);
        assert_eq!(s.take_invalid(), 5);
        assert_eq!(s.take_invalid(), 0);
    }

    #[test]
    fn busy_marks_objects_only() {
        let v = mark_busy(json!({"a": 1}), 7);
        assert_eq!(v["busy"], true);
        assert_eq!(v["snapshot_age_ms"], 7);
        assert_eq!(mark_busy(json!([1]), 7), json!([1]));
    }
}
