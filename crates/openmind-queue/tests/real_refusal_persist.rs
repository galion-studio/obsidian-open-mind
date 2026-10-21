// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary `kill -9` loop for the refusal counters, gated by
//! `GALION_REAL=1`. Starts the release `openmind-queue` (path from
//! `POSEIDON_BIN`, default `$CARGO_TARGET_DIR/release/openmind-queue`) on
//! 127.0.0.1:20310 (wave-4 slot 161 range 20310-20319) and, per round:
//!
//! * serial phase: a known number of quota (429), budget (402, at the door
//!   and at lease time), backpressure (503) and stale-ack (404/409)
//!   refusals, sometimes an `/admin/compact`, then SIGKILL right after the
//!   last answer; after the restart every counter must equal the client's
//!   own count exactly;
//! * burst phase (every 4th round): 4 threads hammer refusals while the
//!   server is SIGKILLed mid-burst; after the restart each counter must be
//!   at least the number of refusals the clients saw answered and at most
//!   the number they sent.
//!
//! Prints one summary line per round; the record lives in
//! `docs/real-tests/poseidon-refusal-persist.md`.

use openmind_queue::http;
use serde_json::{json, Value};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:20310";

fn bin() -> String {
    std::env::var("POSEIDON_BIN").unwrap_or_else(|_| {
        let t = std::env::var("CARGO_TARGET_DIR")
            .unwrap_or_else(|_| "/workspace/swarm/target/poseidon-refusal-persist".into());
        format!("{t}/release/openmind-queue")
    })
}

fn call(m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(ADDR, m, p, body.as_deref(), Duration::from_secs(5)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

/// The server process; SIGKILLed on drop so a failed assertion never
/// leaves it running on the port.
struct Srv(Option<Child>);
impl Drop for Srv {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn start(data: &std::path::Path, max_depth: u32) -> Srv {
    let mut child = Command::new(bin())
        .args(["serve", "--bind", ADDR, "--max-depth"])
        .arg(max_depth.to_string())
        .arg("--data")
        .arg(data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start release binary (build it first)");
    for _ in 0..100 {
        if http::request(ADDR, "GET", "/healthz", None, Duration::from_millis(200)).is_ok() {
            return Srv(Some(child));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("server did not come up on {ADDR}");
}

fn sigkill(mut s: Srv) {
    let mut c = s.0.take().expect("running");
    c.kill().unwrap(); // SIGKILL on unix
    let _ = c.wait();
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Want {
    quota: u64,
    budget: u64,
    backpressure: u64,
    stale: u64,
}

fn read() -> (Want, Value) {
    let (c, s) = call("GET", "/stats", None);
    assert_eq!(c, 200);
    let st = &s["stats"];
    let g = |k: &str| st[k].as_u64().unwrap();
    (
        Want {
            quota: g("quota_refusals"),
            budget: g("budget_refusals"),
            backpressure: g("backpressure_refusals"),
            stale: g("stale_acks"),
        },
        s["refusals_sidecar"].clone(),
    )
}

/// Tiny deterministic generator so every run does the same work.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

const MAX_DEPTH: u32 = 6;

/// Setup that the log keeps: agent `q` max 1 pending, agent `b` cap 5,
/// agent `l` cap 10 with a cost-6 job leased (so a second cost-6 job is
/// admitted but held at lease time), and the queue filled to MAX_DEPTH.
fn setup() {
    let ok = |c: u16| assert!(c == 200 || c == 201, "setup got {c}");
    ok(call(
        "POST",
        "/quotas",
        Some(json!({"agent":"q","max_pending":1,"max_inflight":1})),
    )
    .0);
    ok(call("POST", "/caps", Some(json!({"scope":"b","cap":5}))).0);
    ok(call("POST", "/caps", Some(json!({"scope":"l","cap":10}))).0);
    ok(call(
        "POST",
        "/jobs",
        Some(json!({"queue":"lq","agent":"l","kind":"echo","cost":6})),
    )
    .0);
    let (_, l) = call(
        "POST",
        "/lease",
        Some(json!({"queue":"lq","worker":"w","lease_ms":3_600_000})),
    );
    assert!(l["id"].is_u64(), "lease {l}");
    ok(call(
        "POST",
        "/jobs",
        Some(json!({"queue":"lq","agent":"l","kind":"echo","cost":6})),
    )
    .0);
    ok(call("POST", "/jobs", Some(json!({"agent":"q","kind":"echo"}))).0);
    // fill the rest of max_depth with agent f
    for _ in 0..(MAX_DEPTH - 3) {
        ok(call("POST", "/jobs", Some(json!({"agent":"f","kind":"echo"}))).0);
    }
}

/// One refusal of kind `k` (0 backpressure, 1 lease-time budget stop,
/// 2 stale ack), counted in `w`.
fn refuse(k: u64, w: &mut Want) {
    match k {
        0 => {
            // depth is at MAX_DEPTH: backpressure (checked before quota)
            let (c, _) = call("POST", "/jobs", Some(json!({"agent":"z","kind":"echo"})));
            assert_eq!(c, 503);
            w.backpressure += 1;
        }
        1 => {
            // lease-time hard stop: l has 6 reserved of cap 10, next costs 6
            let (c, v) = call("POST", "/lease", Some(json!({"queue":"lq","worker":"w"})));
            assert_eq!(c, 200);
            assert_eq!(v["miss"], "budget_hard_stop", "{v}");
            w.budget += 1;
        }
        _ => {
            let (c, _) = call("POST", "/jobs/999999/ack", Some(json!({"token":1})));
            assert_eq!(c, 404);
            w.stale += 1;
        }
    }
}

#[test]
fn real_refusal_counters_survive_kill9_loop() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-binary test");
        return;
    }
    let rounds: u64 = std::env::var("POSEIDON_KILL_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    let data = std::env::temp_dir().join(format!("poseidon-real-refusals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut srv = start(&data, MAX_DEPTH);
    setup();
    let mut want = Want::default();
    let mut rng = Lcg(161);
    let mut compactions = 0;
    let mut bursts = 0;
    let t0 = Instant::now();

    // Quota and door-budget refusals need depth below MAX_DEPTH, so they
    // have their own server in the next test.
    for round in 1..=rounds {
        let n = 5 + rng.next(20);
        for _ in 0..n {
            refuse(rng.next(3), &mut want);
        }
        if rng.next(4) == 0 {
            let (c, _) = call("POST", "/admin/compact", None);
            assert_eq!(c, 200);
            compactions += 1;
        }
        let burst = round % 4 == 0;
        let (lo, hi) = if burst {
            bursts += 1;
            // 4 hammering threads; SIGKILL while they run
            let stop = Arc::new(AtomicBool::new(false));
            let sent = Arc::new(AtomicU64::new(0));
            let seen = Arc::new(AtomicU64::new(0));
            let ts: Vec<_> = (0..4)
                .map(|_| {
                    let (stop, sent, seen) = (stop.clone(), sent.clone(), seen.clone());
                    std::thread::spawn(move || {
                        while !stop.load(Ordering::SeqCst) {
                            sent.fetch_add(1, Ordering::SeqCst);
                            let r = http::request(
                                ADDR,
                                "POST",
                                "/jobs/999999/ack",
                                Some(r#"{"token":1}"#),
                                Duration::from_secs(2),
                            );
                            if matches!(r, Ok((404, _))) {
                                seen.fetch_add(1, Ordering::SeqCst);
                            }
                        }
                    })
                })
                .collect();
            std::thread::sleep(Duration::from_millis(150 + rng.next(200)));
            let before_kill = seen.load(Ordering::SeqCst);
            sigkill(srv);
            stop.store(true, Ordering::SeqCst);
            for t in ts {
                t.join().unwrap();
            }
            let lo = want.stale + seen.load(Ordering::SeqCst);
            let hi = want.stale + sent.load(Ordering::SeqCst);
            eprintln!(
                "round {round:2}: burst seen {} (>= {before_kill} before kill) sent {}",
                lo - want.stale,
                hi - want.stale
            );
            (lo, hi)
        } else {
            sigkill(srv);
            (want.stale, want.stale)
        };
        srv = start(&data, MAX_DEPTH);
        let (got, sc) = read();
        assert_eq!(got.quota, want.quota, "round {round} quota");
        assert_eq!(got.budget, want.budget, "round {round} budget");
        assert_eq!(
            got.backpressure, want.backpressure,
            "round {round} backpressure"
        );
        assert!(
            got.stale >= lo && got.stale <= hi,
            "round {round} stale {} not in [{lo}, {hi}]",
            got.stale
        );
        // the burst's real number becomes the new exact baseline
        want.stale = got.stale;
        assert_eq!(sc["bad_slots"], 0, "round {round}");
        assert_eq!(sc["restored"], true, "round {round}");
        eprintln!(
            "round {round:2}: kill -9 -> restored quota {} budget {} backpressure {} stale {} (seq {})",
            got.quota, got.budget, got.backpressure, got.stale, sc["restored_seq"]
        );
        // the job state survived too: the hour-long lease is still held
        let (_, s) = call("GET", "/stats", None);
        assert_eq!(s["depth"]["leased"], 1, "round {round}");
        assert_eq!(s["depth"]["ready"].as_u64().unwrap() as u32, MAX_DEPTH - 1);
    }
    sigkill(srv);
    eprintln!(
        "kill -9 loop: {rounds} rounds, {compactions} compactions, {bursts} bursts, {:.1} s; final {:?}",
        t0.elapsed().as_secs_f64(),
        want
    );
    let _ = std::fs::remove_dir_all(&data);
}

/// The two counters the task names, at the door: quota (429) and budget
/// (402), with a kill -9 after every single refusal.
#[test]
fn real_quota_and_budget_refusals_survive_kill9_each() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-binary test");
        return;
    }
    let data = std::env::temp_dir().join(format!("poseidon-real-refusals2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut srv = start(&data, 100_000);
    call(
        "POST",
        "/quotas",
        Some(json!({"agent":"q","max_pending":1,"max_inflight":1})),
    );
    call("POST", "/caps", Some(json!({"scope":"b","cap":5})));
    assert_eq!(
        call("POST", "/jobs", Some(json!({"agent":"q","kind":"echo"}))).0,
        201
    );
    let mut want = Want::default();
    for i in 1..=20u64 {
        if i % 2 == 1 {
            assert_eq!(
                call("POST", "/jobs", Some(json!({"agent":"q","kind":"echo"}))).0,
                429
            );
            want.quota += 1;
        } else {
            let (c, _) = call(
                "POST",
                "/jobs",
                Some(json!({"agent":"b","kind":"echo","cost":9})),
            );
            assert_eq!(c, 402);
            want.budget += 1;
        }
        sigkill(srv);
        srv = start(&data, 100_000);
        let (got, _) = read();
        assert_eq!(
            (got.quota, got.budget),
            (want.quota, want.budget),
            "kill {i}"
        );
    }
    eprintln!(
        "door refusals: 20 kills, restored quota {} budget {}",
        want.quota, want.budget
    );
    sigkill(srv);
    let _ = std::fs::remove_dir_all(&data);
}
