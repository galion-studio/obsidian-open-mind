// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wake and escalation counters for `GET /stats`.
//!
//! A *wake* is the main event loop returning from its blocking wait: one per
//! event delivered by a source (journal line, inotify batch, pidfd exit,
//! HTTP event) or per timer deadline (heartbeat, debounce flush, deferred
//! engine cycle). Rates are reported over the last hour (sliding) and as an
//! average since start.

use super::Severity;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

const HOUR: u64 = 3600;
const KEEP: usize = 100_000;

#[derive(Default)]
struct Window(VecDeque<u64>, u64);

impl Window {
    fn hit(&mut self, t: u64) {
        self.0.push_back(t);
        self.1 += 1;
        if self.0.len() > KEEP {
            self.0.pop_front();
        }
    }
    fn last_hour(&mut self, now: u64) -> usize {
        while self.0.front().is_some_and(|&x| x + HOUR <= now) {
            self.0.pop_front();
        }
        self.0.len()
    }
}

pub struct Stats {
    pub started: u64,
    wakes: Window,
    pub wakes_by: BTreeMap<String, u64>,
    escalations: Window,
    engine_cycles: Window,
    pub published_by_sev: BTreeMap<&'static str, u64>,
    pub deduped: u64,
    pub batches: u64,
    pub escalation_refused: u64,
    pub llm_calls: u64,
    pub llm_usd: f64,
    pub source_errors: BTreeMap<String, u64>,
    pub last_wake: Option<u64>,
}

impl Stats {
    pub fn new(now: u64) -> Self {
        Stats {
            started: now,
            wakes: Window::default(),
            wakes_by: BTreeMap::new(),
            escalations: Window::default(),
            engine_cycles: Window::default(),
            published_by_sev: BTreeMap::new(),
            deduped: 0,
            batches: 0,
            escalation_refused: 0,
            llm_calls: 0,
            llm_usd: 0.0,
            source_errors: BTreeMap::new(),
            last_wake: None,
        }
    }
    pub fn wake(&mut self, why: &str, t: u64) {
        self.wakes.hit(t);
        *self.wakes_by.entry(why.to_string()).or_default() += 1;
        self.last_wake = Some(t);
    }
    pub fn escalated(&mut self, t: u64) {
        self.escalations.hit(t);
    }
    pub fn engine_cycle(&mut self, t: u64) {
        self.engine_cycles.hit(t);
    }
    pub fn published(&mut self, s: Severity, _t: u64) {
        *self.published_by_sev.entry(s.as_str()).or_default() += 1;
    }
    pub fn source_error(&mut self, src: &str) {
        *self.source_errors.entry(src.to_string()).or_default() += 1;
    }
    pub fn wakes_total(&self) -> u64 {
        self.wakes.1
    }
    pub fn escalations_total(&self) -> u64 {
        self.escalations.1
    }

    pub fn to_json(&mut self, now: u64) -> Value {
        let up = now.saturating_sub(self.started).max(1);
        let per_h = |n: u64| ((n as f64) * 3600.0 / up as f64 * 100.0).round() / 100.0;
        json!({
            "mode": "event",
            "uptime_s": up,
            "started": crate::util::ts(self.started),
            "wakes_total": self.wakes.1,
            "wakes_last_hour": self.wakes.last_hour(now),
            "wakes_per_hour_avg": per_h(self.wakes.1),
            "wakes_by": self.wakes_by,
            "escalations_total": self.escalations.1,
            "escalations_last_hour": self.escalations.last_hour(now),
            "escalations_per_hour_avg": per_h(self.escalations.1),
            "engine_cycles_total": self.engine_cycles.1,
            "engine_cycles_last_hour": self.engine_cycles.last_hour(now),
            "batches": self.batches,
            "escalation_refused": self.escalation_refused,
            "llm_calls": self.llm_calls,
            "llm_usd": self.llm_usd,
            "published_by_severity": self.published_by_sev,
            "deduped": self.deduped,
            "source_errors": self.source_errors,
            "last_wake": self.last_wake.map(crate::util::ts),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sliding_hour_and_average() {
        let mut s = Stats::new(0);
        for t in [10, 20, 3000, 3700] {
            s.wake("journal", t);
        }
        s.escalated(3000);
        let j = s.to_json(3700);
        assert_eq!(j["wakes_total"], 4);
        assert_eq!(j["wakes_last_hour"], 2); // 10 and 20 aged out at 3700
        assert_eq!(j["escalations_last_hour"], 1);
        assert_eq!(j["wakes_by"]["journal"], 4);
        assert!((j["wakes_per_hour_avg"].as_f64().unwrap() - 3.89).abs() < 0.01);
    }
}
