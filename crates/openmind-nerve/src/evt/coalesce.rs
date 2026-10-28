// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Debounce / coalesce: a burst of escalation-worthy events becomes ONE batch.
//!
//! Only events at or above the escalation floor (high) enter. Dedupe already
//! happened in the hub (identical key inside the dedupe window). A batch
//! closes when no new event arrived for `debounce_s` (the burst settled) or
//! the first event is `max_wait_s` old (a never-ending storm still gets
//! reported). Inside one batch the same key is kept once with a count.

use super::{NerveEvent, Severity};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct Batch {
    pub id: u64,
    pub opened: u64,
    pub closed: u64,
    pub max_severity: Severity,
    pub events: Vec<NerveEvent>,
    /// Events folded into this batch (including same-key repeats).
    pub folded: u32,
}

impl Batch {
    /// Short, secret-free summary for the escalation prompt and the log line.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} event(s), max {}, {}s: ",
            self.events.len(),
            self.max_severity.as_str(),
            self.closed.saturating_sub(self.opened)
        );
        let parts: Vec<String> = self
            .events
            .iter()
            .take(12)
            .map(|e| {
                format!(
                    "{} {}{}{}",
                    e.severity.as_str(),
                    e.kind,
                    e.unit
                        .as_deref()
                        .map(|u| format!(" {u}"))
                        .unwrap_or_default(),
                    if e.count > 1 {
                        format!(" x{}", e.count)
                    } else {
                        String::new()
                    }
                )
            })
            .collect();
        s.push_str(&parts.join("; "));
        s
    }
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "opened": crate::util::ts(self.opened), "closed": crate::util::ts(self.closed),
               "max_severity": self.max_severity, "folded": self.folded, "summary": self.summary(),
               "seqs": self.events.iter().map(|e| e.seq).collect::<Vec<_>>()})
    }
}

pub struct Coalescer {
    pub min: Severity,
    pub debounce_s: u64,
    pub max_wait_s: u64,
    pending: Vec<NerveEvent>,
    first: Option<u64>,
    last: u64,
    folded: u32,
    next_id: u64,
}

impl Coalescer {
    pub fn new(min: Severity, debounce_s: u64, max_wait_s: u64) -> Self {
        Coalescer {
            min,
            debounce_s,
            max_wait_s,
            pending: Vec::new(),
            first: None,
            last: 0,
            folded: 0,
            next_id: 1,
        }
    }

    /// Offer an event; true if it joined the pending batch.
    pub fn offer(&mut self, ev: &NerveEvent, now: u64) -> bool {
        if ev.severity < self.min {
            return false;
        }
        self.folded += 1;
        self.last = now;
        self.first.get_or_insert(now);
        if let Some(p) = self.pending.iter_mut().find(|p| p.key == ev.key) {
            p.count = p.count.saturating_add(ev.count.max(1));
            if ev.severity > p.severity {
                p.severity = ev.severity;
            }
        } else if self.pending.len() < 64 {
            self.pending.push(ev.clone());
        }
        true
    }

    /// When the pending batch is due (unix secs), if any.
    pub fn deadline(&self) -> Option<u64> {
        let f = self.first?;
        Some((self.last + self.debounce_s).min(f + self.max_wait_s))
    }

    /// Close and return the batch if it is due at `now`.
    pub fn flush_due(&mut self, now: u64) -> Option<Batch> {
        if self.deadline()? > now {
            return None;
        }
        let events = std::mem::take(&mut self.pending);
        let opened = self.first.take().unwrap_or(now);
        let b = Batch {
            id: self.next_id,
            opened,
            closed: now,
            max_severity: events
                .iter()
                .map(|e| e.severity)
                .max()
                .unwrap_or(Severity::High),
            events,
            folded: std::mem::take(&mut self.folded),
        };
        self.next_id += 1;
        Some(b)
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, unit: &str, sev: Severity) -> NerveEvent {
        NerveEvent::new("journal", kind, sev, "m").unit(unit)
    }

    #[test]
    fn burst_becomes_one_batch() {
        let mut c = Coalescer::new(Severity::High, 30, 120);
        // crash loop: exit, restart scheduled, exit, ... over 20 s
        for (i, t) in (100..120).step_by(2).enumerate() {
            let k = if i % 2 == 0 {
                "unit.exit"
            } else {
                "unit.restart_scheduled"
            };
            assert!(c.offer(&ev(k, "hermes-rs.service", Severity::High), t));
        }
        assert!(c.offer(
            &ev("unit.start_limit", "hermes-rs.service", Severity::Critical),
            121
        ));
        assert!(!c.offer(&ev("unit.started", "tuner.service", Severity::Medium), 121));
        assert_eq!(c.deadline(), Some(151));
        assert!(c.flush_due(150).is_none());
        let b = c.flush_due(151).unwrap();
        assert_eq!(b.events.len(), 3);
        assert_eq!(b.folded, 11);
        assert_eq!(b.max_severity, Severity::Critical);
        assert!(b.summary().contains("unit.exit hermes-rs.service x5"));
        assert!(c.flush_due(10_000).is_none());
        assert_eq!(c.deadline(), None);
    }

    #[test]
    fn endless_storm_capped_by_max_wait() {
        let mut c = Coalescer::new(Severity::High, 30, 120);
        let mut batches = 0;
        for t in (0..600).step_by(10) {
            c.offer(&ev("unit.exit", "x.service", Severity::High), t);
            if c.flush_due(t).is_some() {
                batches += 1;
            }
        }
        // one batch per 120 s of storm, not one per event (60 events)
        assert_eq!(batches, 4);
    }
}
