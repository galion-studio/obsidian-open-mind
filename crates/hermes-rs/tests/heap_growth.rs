// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 198 hermes-mem-growth: heap profile of the gateway's chat path.
//!
//! A std-only counting global allocator (this test binary only) tracks live
//! heap bytes. Each scenario builds a fresh in-process `Gateway`, warms it
//! up, then measures the live heap retained per chat over a few thousand
//! `/v1/chat/completions` requests handled in-process (no sockets, no
//! network, no spend). SQLite's own C allocations are not counted (they are
//! bounded by its page cache; the release soak `real_mem_growth` covers RSS).
//!
//! The pre-198 `MockProvider` (unbounded request log) is reproduced with
//! `call_log_cap = usize::MAX`; clearing its log must give the growth back,
//! which pins the leak on that one structure (attribution). Every other
//! configuration must retain (almost) nothing per chat.
//!
//! Run: `HMG_HEAP=1 cargo test --release -p hermes-rs --test heap_growth -- --nocapture`

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::http::{Request, Response};
use hermes_rs::provider::{ChatRequest, ChatResult, MockProvider, Provider, StreamEvent, Target};
use hermes_rs::session::SessionStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{ClockTool, Registry};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        p
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            LIVE.fetch_add(new as isize - l.size() as isize, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

const KEY: &str = "heap-growth-key-0123456789abcdef";
// Debug builds run ~10x slower: smaller window, same warm-up past the caches.
const WARM: usize = 2_600;
const N: usize = if cfg!(debug_assertions) { 1_000 } else { 3_000 };
/// Flat = under this many retained bytes per chat after warm-up.
const FLAT_B_PER_CHAT: f64 = 32.0;

/// Production-shaped provider: answers like the echo mock, records nothing
/// (what `CurlProvider` keeps per call: nothing).
struct NoLog(MockProvider);
impl Provider for NoLog {
    fn chat(
        &self,
        t: &Target,
        req: &ChatRequest,
        _c: &AtomicBool,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResult, String> {
        let r = (self.0.behaviour)(t, req);
        sink(StreamEvent::Content(&r.content));
        Ok(r)
    }
}

fn home(tag: &str) -> std::path::PathBuf {
    let h = std::env::temp_dir().join(hermes_rs::new_id(&format!("hmg-heap-{tag}-")));
    std::fs::create_dir_all(&h).unwrap();
    // a persona-sized SOUL.md so each request carries a realistic system prompt
    let soul = "You are Hermes, the Galion assistant. Answer briefly and honestly. ".repeat(40);
    std::fs::write(h.join("SOUL.md"), soul).unwrap();
    h
}

#[derive(Clone, Copy, PartialEq)]
enum StoreKind {
    Sqlite,
    Jsonl,
}

fn gateway(
    provider: Arc<dyn Provider>,
    store: StoreKind,
    tag: &str,
) -> (Arc<Gateway>, std::path::PathBuf) {
    let h = home(tag);
    let mut cfg = Config::defaults(&h);
    // offline mock: lift the default 500 calls/day cap (no spend possible)
    cfg.spend.daily_call_cap = 0;
    let cfg = Arc::new(cfg);
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let mut tools = Registry::new();
    tools.add(Arc::new(ClockTool));
    let agent = hermes_rs::agent::boss(cfg.clone(), provider, spend, tools);
    let st: Arc<dyn SessionStore> = match store {
        #[cfg(feature = "sqlite")]
        StoreKind::Sqlite => {
            Arc::new(hermes_rs::session_sqlite::SqliteStore::open_rw(&h.join("state.db")).unwrap())
        }
        #[cfg(not(feature = "sqlite"))]
        StoreKind::Sqlite => Arc::new(hermes_rs::session::MemStore::default()),
        StoreKind::Jsonl => {
            Arc::new(hermes_rs::session::JsonlStore::open(h.join("sessions")).unwrap())
        }
    };
    (Arc::new(Gateway::new(cfg, agent, st, Some(KEY.into()))), h)
}

fn chat(gw: &Arc<Gateway>, text: &str, sid: Option<&str>) -> u16 {
    let body = serde_json::json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]});
    let mut headers = vec![
        ("authorization".to_string(), format!("Bearer {KEY}")),
        ("content-type".to_string(), "application/json".to_string()),
    ];
    if let Some(s) = sid {
        headers.push(("x-hermes-session-id".to_string(), s.to_string()));
    }
    let req = Request {
        method: "POST".into(),
        path: "/v1/chat/completions".into(),
        query: String::new(),
        headers,
        body: body.to_string().into_bytes(),
    };
    match gw.handle(req) {
        Response::Full { status, .. } => status,
        Response::Stream { status, .. } => status,
    }
}

/// Load: mostly new sessions, every 10th a reused session id (short
/// conversation, rotated every 6 turns).
fn drive(gw: &Arc<Gateway>, from: usize, n: usize) {
    for i in from..from + n {
        let code = if i % 10 == 3 {
            let sid = format!("hmg-sess-{}", i / 60);
            chat(gw, &format!("turn {i}"), Some(&sid))
        } else {
            chat(gw, &format!("hi {i}"), None)
        };
        assert_eq!(code, 200, "chat {i}");
    }
}

struct Measured {
    /// Least-squares slope of live heap over the window, bytes per chat.
    per_chat: f64,
    /// Live heap at window end minus window start.
    grown: isize,
    /// Window max minus min (sawtooth of bounded caches).
    swing: isize,
}

/// Warm-up past every bounded cache (spend map 2000 sessions, runs 200,
/// mock log 256), then sample the live heap every 50 chats.
fn measure(gw: &Arc<Gateway>) -> Measured {
    drive(gw, 0, WARM);
    let l0 = live();
    let mut pts: Vec<(f64, f64)> = vec![(0.0, l0 as f64)];
    let mut done = 0;
    while done < N {
        drive(gw, WARM + done, 50);
        done += 50;
        pts.push((done as f64, live() as f64));
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let num: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let den: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let ys = pts.iter().map(|p| p.1 as isize);
    let (lo, hi) = (ys.clone().min().unwrap(), ys.max().unwrap());
    Measured {
        per_chat: num / den,
        grown: live() - l0,
        swing: hi - lo,
    }
}

static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn heap_profile_chat_path() {
    // ~2.5 min in release, much longer in debug: a measurement, so gated like
    // the real tests. The bound itself is guarded on every run by the
    // provider unit tests (mock_call_log_*).
    if !(std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
        || std::env::var("HMG_HEAP").is_ok_and(|v| v == "1"))
    {
        eprintln!("skipped: set GALION_REAL=1 or HMG_HEAP=1 (heap profile, use --release)");
        return;
    }
    let _g = SERIAL.lock().unwrap();
    let mut report = Vec::new();
    let a0 = ALLOCS.load(Ordering::Relaxed);

    // 1. pre-198 behaviour: unbounded request log
    let mut m = MockProvider::echo();
    m.call_log_cap = usize::MAX;
    let mock = Arc::new(m);
    let (gw, h) = gateway(mock.clone(), StoreKind::Sqlite, "unbounded");
    let r = measure(&gw);
    let kept = mock.calls.lock().unwrap().len();
    let before_clear = live();
    {
        let mut c = mock.calls.lock().unwrap();
        c.clear();
        c.shrink_to_fit();
    }
    let freed = before_clear - live();
    report.push(format!(
        "unbounded log (pre-198): {:.0} B/chat retained ({} B over {N} chats); log holds {kept} requests; clearing it frees {freed} B = {:.1}% of the growth",
        r.per_chat,
        r.grown,
        freed as f64 * 100.0 / r.grown.max(1) as f64
    ));
    assert!(
        r.per_chat > 1000.0,
        "repro failed: only {:.0} B/chat",
        r.per_chat
    );
    assert!(
        freed as f64 >= 0.95 * r.grown as f64,
        "the request log is not the leak: freed {freed} of {}",
        r.grown
    );
    drop(gw);
    let _ = std::fs::remove_dir_all(h);

    // 2. default bounded log, 3. no log, 4. production-shaped provider
    let cases: Vec<(&str, Arc<dyn Provider>, Option<Arc<MockProvider>>)> = {
        let d = Arc::new(MockProvider::echo());
        let off = Arc::new(MockProvider::echo().without_call_log());
        vec![
            ("default cap 256", d.clone(), Some(d)),
            ("--mock (no log)", off.clone(), Some(off)),
            (
                "production-shaped (no log)",
                Arc::new(NoLog(MockProvider::echo())),
                None,
            ),
        ]
    };
    for (name, p, mock) in cases {
        let (gw, h) = gateway(p, StoreKind::Sqlite, "flat");
        let r = measure(&gw);
        let extra = mock
            .as_ref()
            .map(|m| {
                format!(
                    "; log {} of {} calls",
                    m.calls.lock().unwrap().len(),
                    m.calls_total.load(Ordering::Relaxed)
                )
            })
            .unwrap_or_default();
        report.push(format!(
            "{name}: slope {:.1} B/chat ({:+} B over {N} chats, swing {} B){extra}",
            r.per_chat, r.grown, r.swing
        ));
        assert!(
            r.per_chat < FLAT_B_PER_CHAT,
            "{name}: {:.1} B/chat retained (limit {FLAT_B_PER_CHAT})",
            r.per_chat
        );
        drop(gw);
        let _ = std::fs::remove_dir_all(h);
    }

    // 5. pure-Rust JSONL session store (builds without SQLite): its
    // in-memory index holds one row per session by design. Measured, and
    // bounded per session so a regression shows up.
    let (gw, h) = gateway(
        Arc::new(NoLog(MockProvider::echo())),
        StoreKind::Jsonl,
        "jsonl",
    );
    let r = measure(&gw);
    let sessions = N - N / 10; // every chat but the reused-id ones opens one
    let per_session = r.per_chat * N as f64 / sessions as f64;
    report.push(format!(
        "JSONL store index (no-sqlite builds): {:.0} B per new session ({} B over {sessions} sessions) - by design, owner decision",
        per_session, r.grown
    ));
    assert!(
        per_session < 1024.0,
        "JSONL index {per_session:.0} B per session"
    );
    drop(gw);
    let _ = std::fs::remove_dir_all(h);

    report.push(format!(
        "allocations counted: {}",
        ALLOCS.load(Ordering::Relaxed) - a0
    ));
    for l in &report {
        println!("heap: {l}");
    }
}
