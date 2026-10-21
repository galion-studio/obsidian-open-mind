// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for slot 218 (gaia-poseidon-handle), gated: runs only with
//! `GALION_REAL=1`. Ports 20880-20889 (slot 218), loopback only.
//!
//! Drives POSEIDON's real `SubordinateFunction::handle` **in process**
//! through `Gaia::dispatch` (ownership routing, judge, bus) against:
//!
//! * A: the release `openmind-queue` binary on 127.0.0.1:20880 (throwaway
//!   data dir): every compute system x verb, the typed contract on every
//!   answer, submit + spend-cap hard stop as a bus event, read-only batch
//!   leaves spend / leases / acks identical, `kill -9` -> refused +
//!   degraded + bus event, restart on the same dir -> re-handshake, back to
//!   terraforming, job survived;
//! * B: an impostor HTTP service on 20881 that is not openmind-queue: only
//!   `GET /healthz` ever reaches it, every directive refused off-contract;
//! * C: a queue look-alike on 20882 whose dispatch answers break the
//!   contract (unknown outcome): refused off-contract, POSEIDON degraded.
//!
//! Binary: `$CARGO_TARGET_DIR/release/openmind-queue` (build it first).

use gaia_core::{Directive, FunctionId, Gaia, Lifecycle, Outcome, Report, System, Verb};
use gaia_poseidon::QueueConfig;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const QUEUE: &str = "127.0.0.1:20880";
const IMPOSTOR: &str = "127.0.0.1:20881";
const LOOKALIKE: &str = "127.0.0.1:20882";

fn real() -> bool {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skipped: set GALION_REAL=1 (and build the release openmind-queue)");
        return false;
    }
    true
}

fn bin(name: &str) -> String {
    let t = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "/workspace/swarm/target/gaia-poseidon-handle".into());
    format!("{t}/release/{name}")
}

struct Queue(Child);

impl Drop for Queue {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(data: &std::path::Path) -> Queue {
    let mut child = Command::new(bin("openmind-queue"))
        .args(["serve", "--bind", QUEUE, "--data"])
        .arg(data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start release openmind-queue (build it first)");
    for _ in 0..50 {
        // Ready = the queue's own CLI gets 200 from /healthz (not just a bind).
        let ok = Command::new(bin("openmind-queue"))
            .args(["get", "/healthz", "--addr", QUEUE])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Queue(child);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("openmind-queue did not come up on {QUEUE}");
}

/// Operator call with the queue's own CLI (Gaia never does these).
fn op(args: &[&str]) -> Value {
    let out = Command::new(bin("openmind-queue"))
        .args(args)
        .args(["--addr", QUEUE])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "operator {args:?}: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    s.find('{')
        .and_then(|i| serde_json::from_str(s[i..].trim()).ok())
        .unwrap_or(Value::Null)
}

/// Everything that would move if anything spent, leased or acked.
fn money_view() -> Value {
    let s = op(&["get", "/stats"]);
    let st = &s["stats"];
    serde_json::json!({
        "global": s["global"], "agents": s["agents"], "leased": s["depth"]["leased"],
        "acked": st["acked"], "leases": st["leased"], "enqueued": st["enqueued"],
    })
}

fn go(g: &mut Gaia, s: System, v: Verb, p: &str) -> Report {
    let id = g.next_id();
    let d = Directive::new(id, s, v).payload(p);
    let r = g.dispatch(&d);
    assert_ne!(
        r.outcome,
        Outcome::Contained,
        "{s} {v:?} {p:?}: {}",
        r.summary
    );
    assert_eq!(r.function, FunctionId::COMPUTE);
    r
}

fn topics(g: &Gaia) -> Vec<String> {
    g.bus()
        .log()
        .filter(|m| m.topic.starts_with("poseidon-") || m.topic == "budget-hard-stop")
        .map(|m| m.topic.clone())
        .collect()
}

fn state(g: &Gaia) -> Lifecycle {
    g.function_state(FunctionId::COMPUTE).unwrap()
}

#[test]
fn a_real_queue_every_system_and_verb() {
    if !real() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("gaia-poseidon-handle-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut q = start(&dir);
    op(&["post", "/caps", r#"{"scope":"agent-a","cap":50}"#]);
    op(&[
        "post",
        "/vram/devices",
        r#"{"id":"gpu0","kind":"gpu","vram_gb":24,"state":"approved"}"#,
    ]);

    let mut g = gaia::poseidon::booted_with_queue(QueueConfig::new(QUEUE).unwrap());
    assert_eq!(state(&g), Lifecycle::Running);

    // Read-only batch: every system x {status, plan, run, execute}.
    let before = money_view();
    let mut n = 0;
    for s in System::COMPUTE {
        for v in [Verb::Status, Verb::Plan, Verb::Run, Verb::Execute] {
            if s == System::Queues && matches!(v, Verb::Run | Verb::Execute) {
                continue; // submissions are tested below
            }
            for p in ["", "16"] {
                let r = go(&mut g, s, v, p);
                assert!(
                    !r.summary.contains("contract violation"),
                    "{s} {v:?}: {}",
                    r.summary
                );
                let want = match v {
                    Verb::Status if s == System::Queues && p == "16" => Outcome::Done, // job 16 not live
                    Verb::Status => Outcome::Done,
                    Verb::Plan if s == System::VramPool && p.is_empty() => Outcome::Rejected, // needs GB
                    Verb::Plan => Outcome::DryRun,
                    _ => Outcome::Refused,
                };
                assert_eq!(r.outcome, want, "{s} {v:?} {p:?}: {}", r.summary);
                assert_eq!(r.dry_run, want == Outcome::DryRun, "{s} {v:?}");
                println!(
                    "A {s} {} {p:?} -> {} | {}",
                    v.name(),
                    r.outcome.name(),
                    r.summary
                );
                n += 1;
            }
        }
    }
    assert_eq!(n, 28);
    let r = go(&mut g, System::VramPool, Verb::Plan, "48");
    assert!(
        r.summary.contains("fits no approved device"),
        "{}",
        r.summary
    );
    let r = go(&mut g, System::VramPool, Verb::Plan, "x");
    assert_eq!(
        (r.outcome, r.dry_run),
        (Outcome::Rejected, false),
        "{}",
        r.summary
    );
    let r = go(&mut g, System::VramPool, Verb::Status, "");
    assert!(r.summary.contains("gpu0 24 GB"), "{}", r.summary);
    let r = go(&mut g, System::BudgetCaps, Verb::Status, "");
    assert_eq!(r.metric_value("cap:agent-a"), Some(50), "{}", r.summary);
    assert_eq!(
        money_view(),
        before,
        "read-only batch moved spend/leases/acks"
    );
    println!("A read-only batch: {n} + 4 directives, money view unchanged: {before}");

    // Submit, operator worker spends, second submit hits the hard stop.
    let r = go(
        &mut g,
        System::Queues,
        Verb::Run,
        r#"{"agent":"agent-a","kind":"echo","cost":30}"#,
    );
    assert_eq!(r.outcome, Outcome::Done, "{}", r.summary);
    let id = r.metric_value("job_id").unwrap();
    op(&[
        "worker",
        "--worker",
        "rt-w",
        "--max-jobs",
        "1",
        "--idle-exit-ms",
        "1000",
    ]);
    let r = go(
        &mut g,
        System::Queues,
        Verb::Run,
        r#"{"agent":"agent-a","kind":"echo","cost":30}"#,
    );
    assert_eq!(
        (r.outcome, r.metric_value("http_status")),
        (Outcome::Refused, Some(402)),
        "{}",
        r.summary
    );
    let r = go(
        &mut g,
        System::Queues,
        Verb::Execute,
        r#"{"agent":"agent-a","kind":"echo","cost":10}"#,
    );
    assert_eq!(r.outcome, Outcome::Done, "{}", r.summary);
    let live = r.metric_value("job_id").unwrap();
    assert!(live > id);
    assert!(
        topics(&g).contains(&"budget-hard-stop".to_string()),
        "{:?}",
        topics(&g)
    );
    let r = go(&mut g, System::Queues, Verb::Status, &format!("job:{live}"));
    assert_eq!(r.metric_value("live"), Some(1), "{}", r.summary);
    println!(
        "A submit {id} acked by operator worker; hard stop 402; job {live} live: {}",
        r.summary
    );

    // Lease-time budget block: the operator lowers the cap under spent +
    // cost; the operator's worker cannot lease job `live`; POSEIDON reports
    // the queue's u64 counter (the old reader matched it as a bool).
    op(&["post", "/caps", r#"{"scope":"agent-a","cap":35}"#]);
    op(&[
        "worker",
        "--worker",
        "rt-w2",
        "--max-jobs",
        "1",
        "--idle-exit-ms",
        "600",
    ]);
    let r = go(&mut g, System::BudgetCaps, Verb::Status, "");
    let blocked = r.metric_value("blocked:agent-a").unwrap_or(0);
    assert!(blocked >= 1, "{:?}", r.metrics);
    println!(
        "A lease-time budget block: blocked:agent-a={blocked}, spent:agent-a={:?}",
        r.metric_value("spent:agent-a")
    );

    // kill -9: refused, degraded, one bus event.
    let _ = q.0.kill();
    let _ = q.0.wait();
    let r = go(&mut g, System::Queues, Verb::Status, "");
    assert_eq!(
        (r.outcome, r.metric_value("unreachable")),
        (Outcome::Refused, Some(1)),
        "{}",
        r.summary
    );
    let _ = go(&mut g, System::Quotas, Verb::Status, "");
    g.tick();
    assert_eq!(state(&g), Lifecycle::Degraded);
    let t = topics(&g);
    assert_eq!(
        t.iter().filter(|x| *x == "poseidon-unreachable").count(),
        1,
        "{t:?}"
    );
    println!("A kill -9: refused unreachable, POSEIDON degraded, events {t:?}");

    // Restart on the same data dir: re-handshake, recover, job survived.
    let _q2 = start(&dir);
    let r = go(&mut g, System::Queues, Verb::Status, &format!("{live}"));
    assert_eq!(
        (r.outcome, r.metric_value("live")),
        (Outcome::Done, Some(1)),
        "{}",
        r.summary
    );
    g.tick();
    assert_eq!(state(&g), Lifecycle::Running);
    let js = g.function_json(FunctionId::COMPUTE).unwrap().compact();
    assert!(js.contains("\"contract\":\"ok\""), "{js}");
    println!("A restart: job {live} survived, POSEIDON terraforming again; state {js}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A tiny HTTP server: answers every request with `answer(request line)`,
/// records request lines.
fn serve(addr: &str, answer: fn(&str) -> String) -> Arc<Mutex<Vec<String>>> {
    let l = TcpListener::bind(addr).expect("bind slot port");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            let _ = r.read_line(&mut line);
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            let line = line.trim().to_string();
            let b = answer(&line);
            s2.lock().unwrap().push(line);
            let mut c = c;
            let _ = write!(
                c,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                b.len()
            );
        }
    });
    seen
}

#[test]
fn b_impostor_gets_only_healthz() {
    if !real() {
        return;
    }
    let seen = serve(IMPOSTOR, |_| {
        r#"{"ok":true,"service":"galion-one","version":"9"}"#.into()
    });
    let mut g = gaia::poseidon::booted_with_queue(QueueConfig::new(IMPOSTOR).unwrap());
    for s in System::COMPUTE {
        for v in [Verb::Status, Verb::Plan] {
            let r = go(&mut g, s, v, "");
            assert_eq!(
                (r.outcome, r.metric_value("off_contract")),
                (Outcome::Refused, Some(1)),
                "{s}: {}",
                r.summary
            );
        }
    }
    let r = go(
        &mut g,
        System::Queues,
        Verb::Run,
        r#"{"agent":"a","cost":1}"#,
    );
    assert_eq!(r.outcome, Outcome::Refused);
    g.tick();
    assert_eq!(state(&g), Lifecycle::Degraded);
    let lines = seen.lock().unwrap().clone();
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|l| l.starts_with("GET /healthz ")),
        "{lines:?}"
    );
    let t = topics(&g);
    assert_eq!(t, ["poseidon-off-contract"], "{t:?}");
    println!(
        "B impostor saw {} requests, all GET /healthz; POSEIDON degraded; last: {}",
        lines.len(),
        r.summary
    );
}

#[test]
fn c_lookalike_breaking_the_contract() {
    if !real() {
        return;
    }
    let seen = serve(LOOKALIKE, |line| {
        if line.starts_with("GET /healthz") {
            r#"{"ok":true,"function":"POSEIDON","service":"openmind-queue","version":"0.0.0-fake"}"#
                .into()
        } else if line.starts_with("GET /gaia/status") {
            r#"{"health":{"load":0}}"#.into()
        } else {
            r#"{"function":"POSEIDON","system":"queues","verb":"status","outcome":"maybe","summary":"?"}"#.into()
        }
    });
    let mut g = gaia::poseidon::booted_with_queue(QueueConfig::new(LOOKALIKE).unwrap());
    let r = go(&mut g, System::Queues, Verb::Status, "");
    assert_eq!(
        (r.outcome, r.metric_value("off_contract")),
        (Outcome::Refused, Some(1)),
        "{}",
        r.summary
    );
    assert!(
        r.summary.contains("unknown outcome \"maybe\""),
        "{}",
        r.summary
    );
    // The look-alike answers for queues when asked about vram-pool: caught too.
    let r = go(&mut g, System::VramPool, Verb::Plan, "16");
    assert!(r.summary.contains("contract violation"), "{}", r.summary);
    g.tick();
    assert_eq!(state(&g), Lifecycle::Degraded);
    let n = seen.lock().unwrap().len();
    println!(
        "C look-alike: {n} requests; refused off-contract; POSEIDON degraded; {}",
        r.summary
    );
}
