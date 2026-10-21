// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 206: is the RSS growth seen in `real_soak_10k` (glibc build) a leak
//! in POSEIDON's own data structures, or memory the allocator keeps?
//! A counting global allocator measures the bytes the process really holds
//! (live heap) after each round of N enqueues + N lease/acks through the
//! real HTTP server (in-process, loopback 127.0.0.1:20769). Gated:
//! `POSEIDON_HEAP=1` (N from `POSEIDON_HEAP_JOBS`, default 10000; rounds
//! from `POSEIDON_HEAP_ROUNDS`, default 6). Run in release.

use openmind_queue::{http, server};
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

struct Counting;
static LIVE: AtomicI64 = AtomicI64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size() as i64, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size() as i64, Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if !q.is_null() {
            LIVE.fetch_add(n as i64 - l.size() as i64, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

const ADDR: &str = "127.0.0.1:20769";

fn call(m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(ADDR, m, p, body.as_deref(), Duration::from_secs(10)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[test]
fn live_heap_is_flat_across_rounds() {
    if std::env::var("POSEIDON_HEAP").as_deref() != Ok("1") {
        eprintln!("skipped: set POSEIDON_HEAP=1 (release) to run the heap-rounds test");
        return;
    }
    let n: u64 = std::env::var("POSEIDON_HEAP_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let rounds: u64 = std::env::var("POSEIDON_HEAP_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(6);
    let dir = std::env::temp_dir().join(format!("poseidon-206-heap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let h = server::start(server::Options {
        bind: ADDR.into(),
        data: dir.clone(),
        ..server::Options::default()
    })
    .unwrap();
    let mut rows = Vec::new();
    for r in 0..rounds {
        let threads: Vec<_> = (0..4)
            .map(|t| {
                std::thread::spawn(move || {
                    for i in 0..n / 4 {
                        let b = json!({"agent": format!("a{}", i % 8), "cost": 1,
                                       "payload": format!("r{r} t{t} j{i} {}", "x".repeat(64))});
                        assert_eq!(call("POST", "/jobs", Some(b)).0, 201);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let full = LIVE.load(Ordering::Relaxed);
        let threads: Vec<_> = (0..8)
            .map(|w| {
                std::thread::spawn(move || loop {
                    let (_, l) = call("POST", "/lease", Some(json!({"worker": format!("h{w}")})));
                    let Some(id) = l["id"].as_u64() else { break };
                    let (c, _) = call(
                        "POST",
                        &format!("/jobs/{id}/ack"),
                        Some(json!({"token": l["token"]})),
                    );
                    assert_eq!(c, 200);
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        // let connection threads finish
        std::thread::sleep(Duration::from_millis(200));
        let (_, st) = call("GET", "/stats", None);
        let drained = LIVE.load(Ordering::Relaxed);
        let row = json!({"round": r + 1, "live_heap_full": full, "live_heap_drained": drained,
                         "rss_kib": rss_kib(), "store_lines": st["store"]["lines"],
                         "compactions": st["store"]["compactions"], "depth": st["depth"]});
        eprintln!(
            "{} heap: {row}",
            openmind_queue::zagreb_ms(openmind_queue::now_ms())
        );
        rows.push(row);
    }
    h.stop();
    let _ = std::fs::remove_dir_all(&dir);
    // compare drained live heap from round 2 on (round 1 sizes the maps)
    let d = |i: usize| rows[i]["live_heap_drained"].as_i64().unwrap();
    let last = rows.len() - 1;
    let growth_per_round = (d(last) - d(1)) / (last as i64 - 1).max(1);
    eprintln!(
        "heap: RESULT drained live heap r2 {} B -> r{} {} B, {} B/round ({} B/job)",
        d(1),
        last + 1,
        d(last),
        growth_per_round,
        growth_per_round / n as i64
    );
    // the log between compactions differs by round, so allow 64 B/job noise
    assert!(
        growth_per_round < 64 * n as i64,
        "live heap grows {growth_per_round} B per {n}-job round"
    );
}
