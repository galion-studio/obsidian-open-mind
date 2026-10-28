// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Lineage: every boot, directive, birth, containment and override is recorded
//! with its parent, its tick and a Europe/Zagreb timestamp. The simulation
//! clock is fixed (base second + tick * step) so runs are reproducible.

use crate::json::Json;

/// Deterministic simulation clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimClock {
    /// Unix second of tick 0.
    pub base_unix: i64,
    /// Simulated seconds per tick.
    pub tick_secs: i64,
}

impl SimClock {
    /// 2026-10-18 21:00 CEST (+02:00), one minute per tick.
    pub const DEFAULT: SimClock = SimClock {
        base_unix: 1_792_350_000,
        tick_secs: 60,
    };

    /// Europe/Zagreb RFC 3339 for a tick (never `Z`).
    pub fn at(self, tick: u64) -> String {
        let t = i64::try_from(tick).unwrap_or(i64::MAX / 2);
        zagreb_time::from_unix(
            self.base_unix
                .saturating_add(t.saturating_mul(self.tick_secs)),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineageRecord {
    pub id: u64,
    pub parent: Option<u64>,
    pub tick: u64,
    pub at: String,
    pub actor: String,
    pub kind: String,
    pub detail: String,
}

impl LineageRecord {
    pub fn to_json(&self) -> Json {
        Json::obj()
            .with("id", self.id)
            .with("parent", self.parent)
            .with("tick", self.tick)
            .with("at", self.at.as_str())
            .with("actor", self.actor.as_str())
            .with("kind", self.kind.as_str())
            .with("detail", self.detail.as_str())
    }
}

/// Bounded lineage log. When full, the oldest records are evicted and counted.
#[derive(Clone, Debug)]
pub struct Lineage {
    records: Vec<LineageRecord>,
    cap: usize,
    next: u64,
    pub evicted: u64,
}

impl Lineage {
    pub fn new(cap: usize) -> Lineage {
        Lineage {
            records: Vec::new(),
            cap: cap.max(1),
            next: 1,
            evicted: 0,
        }
    }

    pub fn record(
        &mut self,
        parent: Option<u64>,
        tick: u64,
        at: String,
        actor: &str,
        kind: &str,
        detail: &str,
    ) -> u64 {
        let id = self.next;
        self.next += 1;
        if self.records.len() >= self.cap {
            self.records.remove(0);
            self.evicted += 1;
        }
        self.records.push(LineageRecord {
            id,
            parent,
            tick,
            at,
            actor: actor.to_string(),
            kind: kind.to_string(),
            detail: detail.to_string(),
        });
        id
    }

    pub fn records(&self) -> &[LineageRecord] {
        &self.records
    }

    pub fn get(&self, id: u64) -> Option<&LineageRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Walk parents from `id` back to the root.
    pub fn ancestry(&self, id: u64) -> Vec<u64> {
        let mut out = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            if out.contains(&c) || out.len() > self.cap {
                break;
            }
            out.push(c);
            cur = self.get(c).and_then(|r| r.parent);
        }
        out
    }

    pub fn count(&self, kind: &str) -> usize {
        self.records.iter().filter(|r| r.kind == kind).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_is_zagreb_and_deterministic() {
        let c = SimClock::DEFAULT;
        assert_eq!(c.at(0), "2026-10-18T21:00:00+02:00");
        assert_eq!(c.at(10), "2026-10-18T21:10:00+02:00");
        assert!(!c.at(5).ends_with('Z'));
    }

    #[test]
    fn ancestry_and_eviction() {
        let mut l = Lineage::new(3);
        let a = l.record(None, 0, "x".into(), "GAIA", "seed", "s");
        let b = l.record(Some(a), 1, "x".into(), "ELEUTHIA", "birth", "p");
        assert_eq!(l.ancestry(b), vec![b, a]);
        l.record(None, 2, "x".into(), "GAIA", "k", "");
        l.record(None, 3, "x".into(), "GAIA", "k", "");
        assert_eq!(l.records().len(), 3);
        assert_eq!(l.evicted, 1);
    }
}
