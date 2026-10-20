// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The concurrency governor: a weighted semaphore over agent slots with a
//! global cap (default 16; the request limit measured on 2026-10-20 is about
//! 18-20 concurrent agents) and a cap per sector. A permit gives its slots
//! back when dropped, so a panicking worker cannot leak capacity.

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct State {
    used: usize,
    peak: usize,
    per_sector: BTreeMap<String, usize>,
    granted: u64,
    denied: u64,
}

#[derive(Debug)]
struct Inner {
    cap: usize,
    sector_caps: BTreeMap<String, usize>,
    state: Mutex<State>,
    cv: Condvar,
}

/// Cheap to clone; all clones share one pool.
#[derive(Clone, Debug)]
pub struct Governor(Arc<Inner>);

/// Slots held by one running agent session.
#[derive(Debug)]
pub struct Permit {
    gov: Governor,
    sector: String,
    slots: usize,
}

impl Permit {
    pub fn slots(&self) -> usize {
        self.slots
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut st = self.gov.lock();
        st.used = st.used.saturating_sub(self.slots);
        if let Some(n) = st.per_sector.get_mut(&self.sector) {
            *n = n.saturating_sub(self.slots);
        }
        drop(st);
        self.gov.0.cv.notify_all();
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub cap: usize,
    pub used: usize,
    pub free: usize,
    pub peak: usize,
    pub granted: u64,
    pub denied: u64,
    pub per_sector: BTreeMap<String, (usize, usize)>,
}

impl Governor {
    /// `cap` worker slots; `sector_caps` maps sector id to its own ceiling.
    pub fn new(cap: usize, sector_caps: BTreeMap<String, usize>) -> Governor {
        Governor(Arc::new(Inner {
            cap: cap.max(1),
            sector_caps,
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn cap(&self) -> usize {
        self.0.cap
    }

    fn sector_cap(&self, sector: &str) -> usize {
        self.0
            .sector_caps
            .get(sector)
            .copied()
            .unwrap_or(self.0.cap)
    }

    /// Slots are clamped to the sector cap so a wide task can still run.
    pub fn clamp(&self, sector: &str, slots: usize) -> usize {
        slots.max(1).min(self.sector_cap(sector)).min(self.0.cap)
    }

    fn fits(&self, st: &State, sector: &str, slots: usize) -> bool {
        let in_sector = st.per_sector.get(sector).copied().unwrap_or(0);
        st.used + slots <= self.0.cap && in_sector + slots <= self.sector_cap(sector)
    }

    /// Take `slots` now or not at all.
    pub fn try_acquire(&self, sector: &str, slots: usize) -> Option<Permit> {
        let slots = self.clamp(sector, slots);
        let mut st = self.lock();
        if !self.fits(&st, sector, slots) {
            st.denied += 1;
            return None;
        }
        Some(self.grant(&mut st, sector, slots))
    }

    /// Wait up to `timeout` for `slots`.
    pub fn acquire_timeout(&self, sector: &str, slots: usize, timeout: Duration) -> Option<Permit> {
        let slots = self.clamp(sector, slots);
        let deadline = Instant::now() + timeout;
        let mut st = self.lock();
        while !self.fits(&st, sector, slots) {
            let now = Instant::now();
            if now >= deadline {
                st.denied += 1;
                return None;
            }
            st = self
                .0
                .cv
                .wait_timeout(st, deadline - now)
                .map(|(g, _)| g)
                .unwrap_or_else(|p| p.into_inner().0);
        }
        Some(self.grant(&mut st, sector, slots))
    }

    fn grant(&self, st: &mut State, sector: &str, slots: usize) -> Permit {
        st.used += slots;
        st.peak = st.peak.max(st.used);
        st.granted += 1;
        *st.per_sector.entry(sector.to_string()).or_insert(0) += slots;
        Permit {
            gov: self.clone(),
            sector: sector.to_string(),
            slots,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.lock();
        let mut per_sector = BTreeMap::new();
        for (s, cap) in &self.0.sector_caps {
            per_sector.insert(
                s.clone(),
                (st.per_sector.get(s).copied().unwrap_or(0), *cap),
            );
        }
        Snapshot {
            cap: self.0.cap,
            used: st.used,
            free: self.0.cap.saturating_sub(st.used),
            peak: st.peak,
            granted: st.granted,
            denied: st.denied,
            per_sector,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gov() -> Governor {
        let caps = [("a".to_string(), 3), ("b".to_string(), 2)]
            .into_iter()
            .collect();
        Governor::new(4, caps)
    }

    #[test]
    fn global_and_sector_caps_hold() {
        let g = gov();
        let p1 = g.try_acquire("a", 3).unwrap();
        assert!(g.try_acquire("a", 1).is_none(), "sector a is full");
        let p2 = g.try_acquire("b", 1).unwrap();
        assert!(g.try_acquire("b", 1).is_none(), "global cap 4 is full");
        assert_eq!(g.snapshot().used, 4);
        drop(p1);
        assert_eq!(g.snapshot().used, 1);
        assert!(g.try_acquire("b", 1).is_some());
        drop(p2);
        assert_eq!(g.snapshot().peak, 4);
    }

    #[test]
    fn wide_task_is_clamped_to_sector_cap() {
        let g = gov();
        let p = g.try_acquire("b", 9).unwrap();
        assert_eq!(p.slots(), 2);
    }

    #[test]
    fn waiter_wakes_when_slots_return() {
        let g = gov();
        let p = g.try_acquire("a", 3).unwrap();
        let g2 = g.clone();
        let h = std::thread::spawn(move || {
            g2.acquire_timeout("a", 2, Duration::from_secs(5))
                .map(|p| p.slots())
        });
        std::thread::sleep(Duration::from_millis(30));
        drop(p);
        assert_eq!(h.join().unwrap(), Some(2));
    }

    #[test]
    fn many_threads_never_exceed_cap() {
        let g = Governor::new(5, BTreeMap::new());
        let max_seen = Arc::new(Mutex::new(0usize));
        let hs: Vec<_> = (0..24)
            .map(|_| {
                let g = g.clone();
                let m = max_seen.clone();
                std::thread::spawn(move || {
                    let p = g.acquire_timeout("x", 2, Duration::from_secs(10)).unwrap();
                    let used = g.snapshot().used;
                    let mut mm = m.lock().unwrap();
                    *mm = (*mm).max(used);
                    drop(mm);
                    std::thread::sleep(Duration::from_millis(2));
                    drop(p);
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert!(*max_seen.lock().unwrap() <= 5);
        assert_eq!(g.snapshot().used, 0);
    }
}
