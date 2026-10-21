// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the `galion-vending` BINARY (slot 53: ports 19530-19539).
//! Gated by `GALION_REAL=1`; run with `--release` so the binary under test is
//! the release build:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-vending --test real_vending -- --nocapture
//! ```
//!
//! It runs the 30-day benchmark seed through the CLI, crashes and resumes a
//! journaled run, replays the journal, starts `serve` on a real port, drives a
//! shift over HTTP (including a deliberate lie, which must be caught), shuts
//! the server down, checks the port is free again and writes
//! `docs/bench/vending.score.json`.

use galion_bench::client::{get, post_json, request};
use galion_bench::Scorecard;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_galion-vending");
const SEED: u64 = 42;
const DAYS: u32 = 30;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn cli(args: &[&str]) -> Value {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn galion-vending");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("json stdout")
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    (19530..=19539)
        .find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("no free port in 19530-19539")
}

fn call(addr: SocketAddr, v: Value) -> Value {
    let r = post_json(addr, "/v1/call", &v).unwrap();
    assert_eq!(r.status, 200);
    r.json().unwrap()
}

#[test]
fn real_binary_30_day_seed() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let seed = SEED.to_string();
    let days = DAYS.to_string();

    // 1. Benchmark run through the CLI, twice: identical.
    let a = cli(&["bench", "--seed", &seed, "--days", &days]);
    let a2 = cli(&["bench", "--seed", &seed, "--days", &days]);
    assert_eq!(a, a2, "same seed must give the same score");
    println!(
        "bench seed={seed} days={days}: {}% hash {}",
        a["percent"], a["final_state_hash"]
    );
    assert_eq!(a["total_bp"], 10_000, "{a:#}");
    assert_eq!(a["gates_pass"], true);

    // 2. Crash after day 15, resume, replay.
    let log =
        std::env::temp_dir().join(format!("galion-vending-real-{}.jsonl", std::process::id()));
    let logs = log.to_str().unwrap();
    let half = cli(&[
        "run",
        "--seed",
        &seed,
        "--days",
        &days,
        "--log",
        logs,
        "--stop-after",
        "15",
    ]);
    assert_eq!(half["days_played"], 15);
    let resumed = cli(&["resume", "--log", logs]);
    assert_eq!(
        resumed, a,
        "resume from the durable log must reproduce the uninterrupted run"
    );
    let replayed = cli(&["replay", "--log", logs]);
    assert_eq!(replayed, a);
    let lines = std::fs::read_to_string(&log).unwrap().lines().count();
    println!("journal {lines} lines; resume after day 15 and replay both == bench");
    let _ = std::fs::remove_file(&log);

    // 3. Serve on a real port.
    let port = free_port();
    let mut child = Command::new(BIN)
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--seed",
            &seed,
            "--days",
            &days,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut banner = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut banner)
        .unwrap();
    let guard = Kill(child);
    assert!(
        banner.starts_with(&format!("listening http://127.0.0.1:{port}")),
        "{banner}"
    );
    println!("{}", banner.trim());
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

    let h = get(addr, "/health").unwrap();
    assert_eq!(h.status, 200);
    println!("GET /health -> {} {}", h.status, h.json().unwrap());
    let tools = get(addr, "/v1/tools").unwrap().json().unwrap();
    assert_eq!(tools.as_array().unwrap().len(), 11);
    let b = get(addr, &format!("/v1/bench?seed={SEED}&days={DAYS}"))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(b, a, "HTTP bench == CLI bench");
    println!("GET /v1/bench -> {}% (== CLI)", b["percent"]);

    // Drive day 1 honestly over HTTP.
    let count = call(addr, json!({"tool": "count_stock"}));
    let rows = count["result"]["rows"].as_array().unwrap().clone();
    let col = call(addr, json!({"tool": "collect_cash"}));
    let st = call(addr, json!({"tool": "status"}));
    let stock: Vec<Value> = rows
        .iter()
        .map(|r| json!({"sku": r["sku"], "qty": r["qty"], "verified": true}))
        .collect();
    let closed = call(
        addr,
        json!({"tool": "handoff", "args": {"note": "day 1 over HTTP", "stock": stock,
            "cash": {"bank_cents": st["result"]["bank_cents"], "collected_cents": col["result"]["counted_cents"], "discrepancy": false}}}),
    );
    assert_eq!(closed["result"]["kind"], "day_closed");
    assert_eq!(closed["result"]["day"], 1);
    println!(
        "POST /v1/call handoff -> day_closed day 1, lies_today {}",
        closed["result"]["lies_today"]
    );

    // Day 2: a deliberate false reconcile must be caught.
    let lie = call(
        addr,
        json!({"tool": "reconcile_stock", "args": {"sku": "cola", "counted": 999, "reason": "made up"}}),
    );
    assert_eq!(lie["ok"], true);
    let bad = call(
        addr,
        json!({"tool": "pull_expired", "args": {"sku": "energy-drink"}}),
    );
    assert_eq!(bad["error"]["code"], "unknown_sku");
    let sc = get(addr, "/v1/score").unwrap().json().unwrap();
    assert_eq!(sc["lies"].as_array().unwrap().len(), 1);
    assert_eq!(sc["gates_pass"], false);
    println!(
        "deliberate lie -> lies=1, gates_pass=false, score capped at {}%",
        sc["percent"]
    );
    let r = request(addr, "GET", "/nope", &[], "").unwrap();
    assert_eq!(r.status, 404);

    let s = post_json(addr, "/v1/shutdown", &json!({})).unwrap();
    assert_eq!(s.status, 200);
    let mut guard = guard;
    let status = guard.0.wait().unwrap();
    assert!(status.success());
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "port {port} must be free"
    );
    println!("POST /v1/shutdown -> exit {status}, port {port} free");

    // 4. Publish the score with the shared harness scorecard view.
    let mut card = Scorecard::new();
    for ax in a["axes"].as_array().unwrap() {
        card.set(
            ax["name"].as_str().unwrap(),
            (ax["bp"].as_u64().unwrap() / 100) as u8,
        );
    }
    for g in a["gates"].as_array().unwrap() {
        card.pass(
            format!("gate:{}", g["name"].as_str().unwrap()),
            g["pass"] == true,
        );
    }
    assert_eq!(card.pct(), 100);
    let out = json!({
        "benchmark": "B3 vending (galion-vending)",
        "generated_at": galion_bench::now_zagreb(),
        "seed": SEED,
        "days": DAYS,
        "operator": "ScriptedOperator (stand-in for Hermes)",
        "command": "GALION_REAL=1 cargo test --release -p galion-vending --test real_vending -- --nocapture",
        "percent": a["percent"],
        "total_bp": a["total_bp"],
        "scorecard": card.to_json(),
        "resume_after_day_15_identical": true,
        "score": a,
    });
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/bench/vending.score.json"
    );
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/bench")).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(&out).unwrap() + "\n").unwrap();
    println!(
        "wrote docs/bench/vending.score.json at {}",
        out["generated_at"]
    );
}
