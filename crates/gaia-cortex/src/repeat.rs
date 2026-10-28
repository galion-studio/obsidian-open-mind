// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Repeat watch: let the flap rule see the crash loops the nerve dedupes
//! (slot 235, T5.1 chaos finding F1).
//!
//! The event-mode nerve (`swarm/nerve-event-driven` bb287cc, live on the
//! VPS) folds every event whose `key` it saw within `dedupe_s` (600 s) into
//! the stored event and only raises that event's `count` in its ring; the
//! stream sent the event once, when it was new. A unit that keeps crashing
//! is therefore ONE event on the stream for as long as it keeps crashing,
//! and the cortex's flap rule (3 failures in 900 s) never fires.
//!
//! The fix needs no nerve change: for every unit failure event the cortex
//! decided on, it remembers `(seq, key, count)`. While at least one such
//! event is watched, a poller re-reads the nerve ring (`GET /evt?since=`,
//! loopback, at most one call every `repeat_poll_s`) and turns every rise in
//! `count` into a repeat event for the decider: a copy of the event with
//! `count` = the new occurrences and `data.repeat = {of_seq, total, new}`.
//! The rules count occurrences, so the flap rule and the caps see the loop.
//!
//! - Idle (nothing watched) = the poller sleeps on a condvar: no calls.
//! - A watch ends `flap_window_s` after the last rise (the rule could not
//!   use older repeats), when the event left the ring, or when the event at
//!   that seq is no longer the same one (`key`/`kind`/`unit` differ, or the
//!   count went down: a restarted nerve numbers its events from 1 again).
//! - At most [`MAX_WATCH`] events are watched (oldest dropped).

use nerve_cortex_contract::{Event, EventKind, REPEAT_FIELD};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Watched events, at most.
pub const MAX_WATCH: usize = 64;
/// Ring pages read per poll (the nerve answers at most 1000 events each).
pub const MAX_PAGES: usize = 4;
pub const PAGE: usize = 1000;

#[derive(Debug, Clone)]
struct Item {
    ev: Event,
    last: u32,
    until: u64,
}

#[derive(Debug, Default, Clone)]
pub struct RepeatStats {
    pub watched_total: u64,
    pub polls: u64,
    pub poll_errors: u64,
    /// Repeat events handed to the decider.
    pub repeat_events: u64,
    /// Occurrences they carried (sum of the `count` rises).
    pub repeats: u64,
    pub expired: u64,
    pub left_ring: u64,
    pub replaced: u64,
    pub last_poll: Option<String>,
}

#[derive(Default)]
struct Inner {
    items: BTreeMap<u64, Item>,
    stats: RepeatStats,
}

pub struct RepeatWatch {
    inner: Mutex<Inner>,
    cv: Condvar,
    /// A watch lives this long after the last rise (the flap window).
    pub window_s: u64,
}

/// Is this a unit failure the flap rule counts (same kinds as
/// `rules::Subject::failures`)?
pub fn is_failure(e: &Event) -> bool {
    match e.kind() {
        EventKind::UnitCrash | EventKind::StartLimit | EventKind::RestartScheduled => {
            e.unit().is_some()
        }
        EventKind::Oom => e.unit().is_some(),
        EventKind::UnitState => e.to_state() == Some("failed") && e.unit().is_some(),
        _ => false,
    }
}

impl RepeatWatch {
    pub fn new(window_s: u64) -> Self {
        RepeatWatch {
            inner: Mutex::new(Inner::default()),
            cv: Condvar::new(),
            window_s: window_s.max(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Watch a nerve failure event the cortex just decided on. Events
    /// without a nerve `seq` or `key` (push, socket, replay) and repeat
    /// events themselves are not watched. Returns true when newly watched.
    pub fn arm(&self, e: &Event, now: u64) -> bool {
        let (Some(seq), Some(_)) = (e.seq, e.key.as_ref()) else {
            return false;
        };
        if seq == 0 || e.repeat_of().is_some() || !is_failure(e) {
            return false;
        }
        let mut g = self.lock();
        if g.items.contains_key(&seq) {
            return false;
        }
        g.items.insert(
            seq,
            Item {
                ev: e.clone(),
                last: e.occurrences(),
                until: now + self.window_s,
            },
        );
        while g.items.len() > MAX_WATCH {
            let first = *g.items.keys().next().unwrap_or(&seq);
            g.items.remove(&first);
            g.stats.expired += 1;
        }
        g.stats.watched_total += 1;
        drop(g);
        self.cv.notify_all();
        true
    }

    /// The nerve restarted: its seqs mean other events now.
    pub fn clear(&self) {
        let mut g = self.lock();
        let n = g.items.len() as u64;
        g.items.clear();
        g.stats.replaced += n;
    }

    pub fn len(&self) -> usize {
        self.lock().items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Lowest watched seq (the ring read starts after `min - 1`).
    pub fn min_seq(&self) -> Option<u64> {
        self.lock().items.keys().next().copied()
    }

    /// Block until something is watched (no timer while idle).
    pub fn wait_armed(&self) {
        let g = self.lock();
        let _g = self
            .cv
            .wait_while(g, |i| i.items.is_empty())
            .unwrap_or_else(|e| e.into_inner());
    }

    /// Drop watches past their window (also when the nerve is unreachable).
    pub fn expire(&self, now: u64) {
        let mut g = self.lock();
        let before = g.items.len();
        g.items.retain(|_, it| it.until > now);
        g.stats.expired += (before - g.items.len()) as u64;
    }

    pub fn note_poll(&self, ok: bool, ts: String) {
        let mut g = self.lock();
        g.stats.polls += 1;
        if !ok {
            g.stats.poll_errors += 1;
        }
        g.stats.last_poll = Some(ts);
    }

    /// Compare a ring read (events with seq > `since`, oldest first,
    /// `complete` = the read reached the end of the ring) with the watch
    /// and return the repeat events for the decider.
    pub fn apply(&self, ring: &[Event], since: u64, complete: bool, now: u64) -> Vec<Event> {
        let by_seq: BTreeMap<u64, &Event> =
            ring.iter().filter_map(|e| e.seq.map(|s| (s, e))).collect();
        let covered_to = if complete {
            u64::MAX
        } else {
            by_seq.keys().next_back().copied().unwrap_or(since)
        };
        let mut out = vec![];
        let mut g = self.lock();
        let Inner { items, stats } = &mut *g;
        items.retain(|seq, it| {
            if it.until <= now {
                stats.expired += 1;
                return false;
            }
            if *seq <= since || *seq > covered_to {
                return true; // not in this read
            }
            let Some(cur) = by_seq.get(seq) else {
                stats.left_ring += 1;
                return false;
            };
            if cur.key != it.ev.key || cur.kind != it.ev.kind || cur.unit != it.ev.unit {
                stats.replaced += 1;
                return false;
            }
            let total = cur.occurrences();
            if total < it.last {
                stats.replaced += 1;
                return false;
            }
            if total > it.last {
                let new = total - it.last;
                let mut r = it.ev.clone();
                r.seq = None;
                r.count = Some(new);
                r.ts = crate::engine::ts(now);
                r.t = Some(now);
                let mut d = match &it.ev.data {
                    Value::Object(m) => Value::Object(m.clone()),
                    Value::Null => json!({}),
                    other => json!({ "nerve_data": other }),
                };
                d[REPEAT_FIELD] = json!({"of_seq": seq, "total": total, "new": new});
                r.data = d;
                out.push(r);
                stats.repeat_events += 1;
                stats.repeats += new as u64;
                it.last = total;
                it.until = now + self.window_s;
            }
            true
        });
        out
    }

    pub fn to_json(&self, poll_s: u64) -> Value {
        let g = self.lock();
        let s = &g.stats;
        json!({
            "poll_s": poll_s, "watching": g.items.len(),
            "watching_seqs": g.items.keys().take(16).collect::<Vec<_>>(),
            "watched_total": s.watched_total, "polls": s.polls, "poll_errors": s.poll_errors,
            "repeat_events": s.repeat_events, "repeats": s.repeats,
            "expired": s.expired, "left_ring": s.left_ring, "replaced": s.replaced,
            "last_poll": s.last_poll,
        })
    }
}

/// One loopback read of the nerve ring after `since` (paged). Returns the
/// events and whether the read reached the ring's end.
pub fn read_ring(
    addr: std::net::SocketAddr,
    since: u64,
    timeout: Duration,
) -> Result<(Vec<Event>, bool), String> {
    use galion_common::http::{send, ClientRequest};
    let host = addr.to_string();
    let mut out: Vec<Event> = vec![];
    let mut from = since;
    for _ in 0..MAX_PAGES {
        let path = format!(
            "{}?since={from}&n={PAGE}",
            nerve_cortex_contract::NERVE_RING_PATH
        );
        let r = send(
            addr,
            &ClientRequest::get(&path, &host),
            timeout,
            8 * 1024 * 1024,
        )
        .map_err(|e| format!("GET {path}: {e}"))?;
        if r.status != 200 {
            return Err(format!("GET {path}: HTTP {}", r.status));
        }
        let page: Vec<Event> = match serde_json::from_str::<Value>(&r.text()) {
            Ok(Value::Array(a)) => a
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect(),
            _ => return Err(format!("GET {path}: not a JSON array")),
        };
        let n = page.len();
        let last = page.iter().filter_map(|e| e.seq).max();
        out.extend(page);
        match last {
            Some(l) if n >= PAGE && l > from => from = l,
            _ => return Ok((out, true)),
        }
    }
    Ok((out, false))
}

/// Poller thread: sleeps on the watch while it is empty; while something is
/// watched, reads the ring every `every` and hands repeat events to `send`.
/// Returns when `send` reports the decider is gone.
pub fn run_poller(
    watch: std::sync::Arc<RepeatWatch>,
    addr: std::net::SocketAddr,
    every: Duration,
    send: &dyn Fn(Vec<Event>) -> bool,
) {
    loop {
        watch.wait_armed();
        std::thread::sleep(every);
        let now = crate::engine::now_secs();
        let Some(min) = watch.min_seq() else {
            continue;
        };
        let since = min.saturating_sub(1);
        match read_ring(addr, since, Duration::from_secs(3)) {
            Ok((ring, complete)) => {
                watch.note_poll(true, crate::engine::ts(now));
                let reps = watch.apply(&ring, since, complete, now);
                if !reps.is_empty() {
                    eprintln!(
                        "gaia-cortex: repeat watch: {} repeat event(s) from the nerve ring ({:?})",
                        reps.len(),
                        reps.iter()
                            .map(|e| (e.repeat_of(), e.unit(), e.occurrences()))
                            .collect::<Vec<_>>()
                    );
                    if !send(reps) {
                        return;
                    }
                }
            }
            Err(e) => {
                watch.note_poll(false, crate::engine::ts(now));
                watch.expire(now);
                eprintln!("gaia-cortex: repeat watch: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, kind: &str, unit: &str, count: u32) -> Event {
        let mut e = Event::new("journal", kind, "high", json!({"exit_code": "exited"}));
        e.seq = Some(seq);
        e.unit = Some(unit.into());
        e.key = Some(format!("{kind}:{unit}"));
        e.count = Some(count);
        e
    }

    #[test]
    fn arms_only_nerve_failures() {
        let w = RepeatWatch::new(900);
        assert!(w.arm(&ev(5, "unit.failed", "tuner.service", 1), 100));
        assert!(
            !w.arm(&ev(5, "unit.failed", "tuner.service", 1), 100),
            "once"
        );
        assert!(
            !w.arm(&ev(6, "unit.exit", "tuner.service", 1), 100),
            "exit is not a failure"
        );
        assert!(!w.arm(&ev(7, "unit.started", "tuner.service", 1), 100));
        let mut pushed = ev(8, "unit.failed", "x.service", 1);
        pushed.seq = None;
        assert!(!w.arm(&pushed, 100), "no nerve seq");
        assert!(w.arm(&ev(9, "unit.restart_scheduled", "tuner.service", 1), 100));
        assert!(w.arm(&ev(10, "unit.start_limit", "tuner.service", 1), 100));
        assert_eq!(w.len(), 3);
        assert_eq!(w.min_seq(), Some(5));
    }

    #[test]
    fn count_rise_becomes_one_repeat_event() {
        let w = RepeatWatch::new(900);
        w.arm(&ev(5, "unit.failed", "tuner.service", 1), 100);
        // nothing new
        let r = w.apply(&[ev(5, "unit.failed", "tuner.service", 1)], 4, true, 110);
        assert!(r.is_empty());
        // the nerve folded 5 more
        let r = w.apply(
            &[
                ev(5, "unit.failed", "tuner.service", 6),
                ev(6, "unit.exit", "tuner.service", 3),
            ],
            4,
            true,
            120,
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].occurrences(), 5);
        assert_eq!(r[0].repeat_of(), Some(5));
        assert_eq!(r[0].seq, None);
        assert_eq!(r[0].data["repeat"]["total"], 6);
        assert_eq!(r[0].data["exit_code"], "exited", "nerve data kept");
        assert_ne!(
            r[0].fingerprint(),
            ev(5, "unit.failed", "tuner.service", 6).fingerprint()
        );
        // only the next rise is new
        let r = w.apply(&[ev(5, "unit.failed", "tuner.service", 7)], 4, true, 130);
        assert_eq!(r[0].occurrences(), 1);
        let s = w.to_json(30);
        assert_eq!(s["repeats"], 6);
        assert_eq!(s["repeat_events"], 2);
    }

    #[test]
    fn watch_ends_on_window_ring_loss_or_new_nerve() {
        let w = RepeatWatch::new(900);
        w.arm(&ev(5, "unit.failed", "a.service", 1), 100);
        w.arm(&ev(6, "unit.failed", "b.service", 1), 100);
        w.arm(&ev(7, "unit.failed", "c.service", 3), 100);
        w.arm(&ev(8, "unit.failed", "d.service", 1), 100);
        // 5 left the ring, 6 is another event now (restarted nerve),
        // 7's count went down (restarted nerve), 8 unchanged
        let r = w.apply(
            &[
                ev(6, "unit.failed", "zzz.service", 4),
                ev(7, "unit.failed", "c.service", 1),
                ev(8, "unit.failed", "d.service", 1),
            ],
            4,
            true,
            200,
        );
        assert!(r.is_empty());
        assert_eq!(w.len(), 1);
        // window: 8 expires 900 s after arming
        assert!(w.apply(&[], 4, true, 1000).is_empty());
        assert!(w.is_empty());
        let s = w.to_json(30);
        assert_eq!(
            (s["left_ring"].as_u64(), s["replaced"].as_u64()),
            (Some(1), Some(2))
        );
        assert_eq!(s["expired"], 1);
    }

    #[test]
    fn incomplete_read_keeps_later_watches() {
        let w = RepeatWatch::new(900);
        w.arm(&ev(5, "unit.failed", "a.service", 1), 100);
        w.arm(&ev(5000, "unit.failed", "b.service", 1), 100);
        let r = w.apply(&[ev(5, "unit.failed", "a.service", 2)], 4, false, 110);
        assert_eq!(r.len(), 1);
        assert_eq!(w.len(), 2, "5000 was beyond this read, still watched");
    }

    #[test]
    fn window_extends_on_each_rise_and_size_is_bounded() {
        let w = RepeatWatch::new(100);
        w.arm(&ev(5, "unit.failed", "a.service", 1), 0);
        assert_eq!(
            w.apply(&[ev(5, "unit.failed", "a.service", 2)], 4, true, 90)
                .len(),
            1
        );
        // 150 > 0 + 100 but < 90 + 100: still watched
        assert_eq!(
            w.apply(&[ev(5, "unit.failed", "a.service", 3)], 4, true, 150)
                .len(),
            1
        );
        for s in 0..(MAX_WATCH as u64 + 10) {
            w.arm(&ev(100 + s, "unit.failed", "x.service", 1), 150);
        }
        assert_eq!(w.len(), MAX_WATCH);
        w.clear();
        assert!(w.is_empty());
    }
}
