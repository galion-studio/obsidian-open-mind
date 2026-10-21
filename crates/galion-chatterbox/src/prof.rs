// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Opt-in wall-clock profiler for the mouth. Off unless the environment
//! variable `CHATTERBOX_PROF=1` is set; then [`span`] guards add their elapsed
//! time to a named bucket and [`report`] returns the table. When off, a span
//! costs one relaxed atomic load.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

static STATE: AtomicU8 = AtomicU8::new(0); // 0 unknown, 1 off, 2 on
static BUCKETS: Mutex<BTreeMap<&'static str, (u64, u128)>> = Mutex::new(BTreeMap::new());

/// True when `CHATTERBOX_PROF` is set to a non-empty value other than `0`.
pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let on = std::env::var("CHATTERBOX_PROF").is_ok_and(|v| !v.is_empty() && v != "0");
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// Times its own lifetime into bucket `name` (when profiling is on).
pub struct Span {
    name: &'static str,
    start: Option<Instant>,
}

pub fn span(name: &'static str) -> Span {
    Span {
        name,
        start: enabled().then(Instant::now),
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(s) = self.start {
            let us = s.elapsed().as_micros();
            if let Ok(mut b) = BUCKETS.lock() {
                let e = b.entry(self.name).or_insert((0, 0));
                e.0 += 1;
                e.1 += us;
            }
        }
    }
}

/// `name calls total_ms` lines, largest first, and clears the buckets.
pub fn report() -> String {
    let mut rows: Vec<(&'static str, (u64, u128))> = match BUCKETS.lock() {
        Ok(mut b) => std::mem::take(&mut *b).into_iter().collect(),
        Err(_) => return String::new(),
    };
    rows.sort_by_key(|r| std::cmp::Reverse(r.1 .1));
    rows.iter()
        .map(|(n, (c, us))| format!("{n:<28} {c:>7} calls {:>10.1} ms\n", *us as f64 / 1000.0))
        .collect()
}
