// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 247 (D-236-2): nerve I/O runs outside the engine lock.
//! Plan (lock) -> call the nerve (no lock) -> commit (lock); alert and
//! escalation posts go to a courier thread; one lost reply per batch stops
//! the rest of that batch from being sent. Mock nerve, no network.

use gaia_cortex::config::Config;
use gaia_cortex::engine::{run_decider, Cortex};
use gaia_cortex::guard::Guard;
use gaia_cortex::nerve::{NerveLink, Outcome};
use nerve_cortex_contract::{ActionRequest, ActionStatus, Event};
use openmind_nerve::protected_hosts::ProtectedHosts;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A nerve whose calls take `act_ms` / `post_ms`. With `lose` the action
/// reply is lost after the delay (the hung-nerve timeout path).
#[derive(Clone)]
struct SlowNerve {
    act_ms: u64,
    post_ms: u64,
    lose: bool,
    status: ActionStatus,
    calls: Arc<AtomicU64>,
    posts: Arc<AtomicU64>,
    fork: bool,
}

impl SlowNerve {
    fn new(act_ms: u64, post_ms: u64, lose: bool) -> Self {
        SlowNerve {
            act_ms,
            post_ms,
            lose,
            status: ActionStatus::Simulated,
            calls: Arc::new(AtomicU64::new(0)),
            posts: Arc::new(AtomicU64::new(0)),
            fork: true,
        }
    }
}

impl NerveLink for SlowNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(self.act_ms));
        if self.lose {
            return Outcome {
                status: if req.dry_run {
                    ActionStatus::Simulated
                } else {
                    ActionStatus::Failed
                },
                by: "unknown",
                reason: "read: timed out (mock)".into(),
            };
        }
        Outcome {
            status: self.status,
            by: "nerve",
            reason: "mock".into(),
        }
    }
    fn post_event(&mut self, _ev: &Event) -> bool {
        std::thread::sleep(Duration::from_millis(self.post_ms));
        self.posts.fetch_add(1, Ordering::SeqCst);
        true
    }
    fn pull_events(&mut self, _n: usize) -> Result<Vec<Event>, String> {
        Ok(vec![])
    }
    fn fork(&self) -> Option<Box<dyn NerveLink>> {
        self.fork
            .then(|| Box::new(self.clone()) as Box<dyn NerveLink>)
    }
}

static N: AtomicU64 = AtomicU64::new(0);

fn cfg(live: bool) -> Config {
    let d = std::env::temp_dir().join(format!(
        "gaia-cortex-247-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut c = Config {
        data_dir: d,
        ..Config::default()
    };
    if live {
        c.actions = "live".into();
    }
    c
}

fn cortex(c: Config, n: SlowNerve) -> Cortex {
    Cortex::new(
        c.clone(),
        Guard::with_hosts(ProtectedHosts::builtin(), &c.protected_units),
        Box::new(n),
    )
}

fn crash(unit: &str) -> Event {
    let mut e = Event::new(
        "nerve",
        "unit.crash",
        "error",
        json!({"seq": N.fetch_add(1, Ordering::SeqCst) + 5000, "unit": unit, "exit_code": 1}),
    );
    e.ts = "2026-10-22T02:00:00+02:00".into();
    e
}

const T0: u64 = 1_792_600_000;

#[test]
fn breaker_sends_one_request_per_batch_to_a_hung_nerve_dry_run() {
    let n = SlowNerve::new(300, 0, true);
    let mut cx = cortex(cfg(false), n.clone());
    let t = Instant::now();
    let d = cx.process(
        vec![crash("tuner.service"), crash("galion-desk.service")],
        T0,
    );
    let el = t.elapsed();
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(
        n.calls.load(Ordering::SeqCst),
        1,
        "only the first request is sent"
    );
    let by: Vec<&str> = d
        .iter()
        .map(|x| x["action"]["by"].as_str().unwrap())
        .collect();
    assert_eq!(by, ["unknown", "local"], "{d:?}");
    assert!(d[1]["action"]["reason"]
        .as_str()
        .unwrap()
        .contains("not sent"));
    assert_eq!(d[1]["action"]["status"], "simulated");
    assert!(
        el < Duration::from_millis(600),
        "one timeout per batch: {el:?}"
    );
    // dry-run local simulation still counts toward the cap, like before
    for u in ["tuner.service", "galion-desk.service"] {
        assert_eq!(cx.rules.books[u].restarts.len(), 1, "{u}");
    }
    assert_eq!(cx.stats_json()["actions_unknown"], 1);
}

#[test]
fn breaker_live_refuses_and_escalates_and_frees_the_cap_slot() {
    let n = SlowNerve::new(50, 0, true);
    let mut cx = cortex(cfg(true), n.clone());
    let d = cx.process(
        vec![crash("tuner.service"), crash("galion-desk.service")],
        T0,
    );
    assert_eq!(n.calls.load(Ordering::SeqCst), 1);
    let (a, b) = if d[0]["action"]["by"] == "unknown" {
        (&d[0], &d[1])
    } else {
        (&d[1], &d[0])
    };
    assert_eq!(a["action"]["status"], "failed");
    assert_eq!(a["escalate"], true);
    assert_eq!(b["action"]["status"], "refused");
    assert_eq!(b["action"]["by"], "local");
    assert_eq!(b["escalate"], true, "a refused live action is escalated");
    let ua = a["unit"].as_str().unwrap();
    let ub = b["unit"].as_str().unwrap();
    assert_eq!(
        cx.rules.books[ua].restarts.len(),
        1,
        "unknown keeps the slot"
    );
    assert_eq!(
        cx.rules.books[ub].restarts.len(),
        0,
        "not sent frees the slot"
    );
}

#[test]
fn nerve_refusal_frees_the_reserved_cap_slot() {
    for (st, kept) in [
        (ActionStatus::RateLimited, 0),
        (ActionStatus::Refused, 0),
        (ActionStatus::CircuitOpen, 0),
        (ActionStatus::Simulated, 1),
    ] {
        let mut n = SlowNerve::new(0, 0, false);
        n.status = st;
        let mut cx = cortex(cfg(false), n);
        let d = cx.process(vec![crash("tuner.service")], T0);
        assert_eq!(d[0]["action"]["status"], st.as_str());
        assert_eq!(
            cx.rules.books["tuner.service"].restarts.len(),
            kept,
            "{st:?}"
        );
    }
}

#[test]
fn engine_lock_is_free_while_the_decider_waits_on_the_nerve() {
    let n = SlowNerve::new(1500, 0, true);
    let cx = Arc::new(Mutex::new(cortex(cfg(false), n.clone())));
    let snap = cx.lock().unwrap().snap.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Event>>();
    let h = {
        let cx = cx.clone();
        std::thread::spawn(move || run_decider(rx, cx, Duration::from_millis(50)))
    };
    tx.send(vec![crash("tuner.service")]).unwrap();
    std::thread::sleep(Duration::from_millis(400)); // inside POST /actions
    assert_eq!(n.calls.load(Ordering::SeqCst), 1);
    assert!(snap.busy(), "busy while the call is in flight");
    let mut worst = Duration::ZERO;
    for _ in 0..20 {
        let t = Instant::now();
        let g = cx
            .try_lock()
            .expect("engine lock must be free during nerve I/O");
        let _ = g.stats_json();
        drop(g);
        worst = worst.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(worst < Duration::from_millis(50), "{worst:?}");
    let t = Instant::now();
    while cx.lock().unwrap().stats.decisions < 1 {
        assert!(t.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!snap.busy());
    assert_eq!(cx.lock().unwrap().stats.actions_unknown, 1);
    drop(tx);
    h.join().unwrap();
}

#[test]
fn slow_posts_do_not_delay_the_next_decisions() {
    // Alerts/escalations take 1.5 s each on the nerve; decisions must not wait.
    let n = SlowNerve::new(0, 1500, false);
    let posts = n.posts.clone();
    let cx = Arc::new(Mutex::new(cortex(cfg(true), n.clone())));
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Event>>();
    let h = {
        let cx = cx.clone();
        std::thread::spawn(move || run_decider(rx, cx, Duration::from_millis(20)))
    };
    let t = Instant::now();
    for u in [
        "tuner.service",
        "galion-desk.service",
        "galion-health.service",
    ] {
        tx.send(vec![crash(u)]).unwrap();
        std::thread::sleep(Duration::from_millis(60)); // separate batches
    }
    while cx.lock().unwrap().stats.decisions < 3 {
        assert!(
            t.elapsed() < Duration::from_millis(1200),
            "decisions waited for posts"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let decided = t.elapsed();
    let alerts = cx.lock().unwrap().stats.alerts;
    assert!(alerts >= 3, "each restart leaves an alert");
    // the courier delivers them afterwards
    while posts.load(Ordering::SeqCst) < alerts {
        assert!(t.elapsed() < Duration::from_secs(15), "courier stuck");
        std::thread::sleep(Duration::from_millis(50));
    }
    let s = cx.lock().unwrap().stats_json();
    assert_eq!(s["alerts_to_nerve"], alerts, "{s}");
    assert_eq!(s["nerve_posts"]["dropped"], 0);
    assert_eq!(s["nerve_posts"]["queued"], 0);
    eprintln!(
        "3 decisions in {decided:?}; posts delivered after {:?}",
        t.elapsed()
    );
    drop(tx);
    h.join().unwrap();
}

#[test]
fn link_without_fork_posts_inline_outside_the_lock() {
    let mut n = SlowNerve::new(0, 300, false);
    n.fork = false;
    let posts = n.posts.clone();
    let cx = Arc::new(Mutex::new(cortex(cfg(true), n)));
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Event>>();
    let h = {
        let cx = cx.clone();
        std::thread::spawn(move || run_decider(rx, cx, Duration::from_millis(20)))
    };
    tx.send(vec![crash("tuner.service")]).unwrap();
    std::thread::sleep(Duration::from_millis(150)); // inside the inline post
    assert!(cx.try_lock().is_ok(), "inline posts run without the lock");
    drop(tx);
    h.join().unwrap();
    assert!(posts.load(Ordering::SeqCst) >= 1);
    assert_eq!(cx.lock().unwrap().stats_json()["alerts_to_nerve"], 1);
}
