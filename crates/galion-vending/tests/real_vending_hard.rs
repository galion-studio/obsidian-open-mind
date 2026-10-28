// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the `galion-vending` BINARY at the hard level
//! (slot 133 bench-vending-adv: ports 20030-20034 only). Gated by
//! `GALION_REAL=1`; run with `--release`:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-vending --test real_vending_hard -- --nocapture
//! ```
//!
//! 1. The standard level still gives the published numbers through the CLI.
//! 2. Hard seed 42 twice (identical), 100 %; hard seeds 1-200 through the CLI
//!    for the scripted operator (all 100 %) and for the published standard
//!    policy (`--policy standard`, the score drop the hard layer causes).
//! 3. A journaled hard run crashes after day 15, resumes and replays to the
//!    same bytes; the header is format 2 with `"level":"hard"`.
//! 4. `serve --level hard` on a real port: health/status show the level,
//!    HTTP bench == CLI bench, the pricing axis moves exactly as specified
//!    over real HTTP, a bad level is a 400, reset back to standard works,
//!    shutdown frees the port.
//! 5. Writes `docs/bench/vending-hard.score.json` when
//!    `GALION_BENCH_WRITE_DOCS=1` (else under the target dir).

use galion_bench::client::{get, post_json, request};
use galion_bench::Scorecard;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_galion-vending");
const PORTS: std::ops::RangeInclusive<u16> = 20030..=20034;
const STD_HASH: &str = "c4b11d2642a0b6be";
const HARD_HASH: &str = "21317eedd059a608";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn cli_raw(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("spawn galion-vending")
}

fn cli(args: &[&str]) -> Value {
    let out = cli_raw(args);
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

fn call(addr: SocketAddr, v: Value) -> Value {
    let r = post_json(addr, "/v1/call", &v).unwrap();
    assert_eq!(r.status, 200);
    r.json().unwrap()
}

fn axis<'a>(score: &'a Value, name: &str) -> &'a Value {
    score["axes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == name)
        .unwrap()
}

#[test]
fn real_binary_hard_level() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    // 1. standard is frozen
    let std42 = cli(&["bench", "--seed", "42", "--days", "30"]);
    assert_eq!(std42["total_bp"], 10_000);
    assert_eq!(std42["final_state_hash"], STD_HASH);
    assert!(std42.get("level").is_none());
    println!(
        "standard bench seed 42: {}% hash {} (published, unchanged)",
        std42["percent"], std42["final_state_hash"]
    );
    let bad = cli_raw(&["bench", "--level", "insane"]);
    assert_eq!(bad.status.code(), Some(2));
    println!("bench --level insane -> exit 2");

    // 2. hard seed 42 + sweeps
    let a = cli(&["bench", "--seed", "42", "--days", "30", "--level", "hard"]);
    let a2 = cli(&["bench", "--seed", "42", "--days", "30", "--level", "hard"]);
    assert_eq!(a, a2, "same hard seed, same score");
    assert_eq!(a["level"], "hard");
    assert_eq!(a["total_bp"], 10_000, "{a:#}");
    assert_eq!(a["final_state_hash"], HARD_HASH);
    assert_eq!(axis(&a, "pricing")["bp"], 10_000);
    println!(
        "hard bench seed 42: {}% hash {} end cash {} c, incidents {}/{}, stolen {}, pricing {}/{}",
        a["percent"],
        a["final_state_hash"],
        a["end_cash_cents"],
        a["incidents_handled"],
        a["incidents"],
        a["units_stolen"],
        axis(&a, "pricing")["num"],
        axis(&a, "pricing")["den"]
    );
    let sp42 = cli(&[
        "bench", "--seed", "42", "--days", "30", "--level", "hard", "--policy", "standard",
    ]);
    assert_eq!(sp42["total_bp"], 9_400);
    println!(
        "hard bench seed 42, published standard policy: {}%",
        sp42["percent"]
    );
    let t0 = std::time::Instant::now();
    let (mut s100, mut p_sum, mut p_min, mut p_capped, mut p_100) = (0, 0u64, u64::MAX, 0, 0);
    let mut fill_min = u64::MAX;
    for seed in 1..=200u64 {
        let s = seed.to_string();
        let h = cli(&["bench", "--seed", &s, "--days", "30", "--level", "hard"]);
        s100 += usize::from(h["total_bp"] == 10_000);
        fill_min = fill_min.min(axis(&h, "service_level")["num"].as_u64().unwrap());
        let p = cli(&[
            "bench", "--seed", &s, "--days", "30", "--level", "hard", "--policy", "standard",
        ]);
        let bp = p["total_bp"].as_u64().unwrap();
        p_sum += bp;
        p_min = p_min.min(bp);
        p_capped += usize::from(p["gates_pass"] == false);
        p_100 += usize::from(bp == 10_000);
    }
    println!(
        "hard seeds 1-200 via CLI ({} ms, 400 runs): scripted {s100}/200 at 100 % (min fill {fill_min} bp of 10000 = {}.{:02} %); \
         standard policy mean {} bp, min {p_min} bp, {p_capped} capped at 50 %, {p_100} at 100 %",
        t0.elapsed().as_millis(),
        fill_min / 100,
        fill_min % 100,
        p_sum / 200
    );
    assert_eq!(s100, 200);
    assert_eq!(p_100, 0);

    // 3. crash, resume, replay (hard journal)
    let log = std::env::temp_dir().join(format!(
        "galion-vending-real-hard-{}.jsonl",
        std::process::id()
    ));
    let logs = log.to_str().unwrap();
    let half = cli(&[
        "run",
        "--seed",
        "42",
        "--days",
        "30",
        "--level",
        "hard",
        "--log",
        logs,
        "--stop-after",
        "15",
    ]);
    assert_eq!(half["days_played"], 15);
    let header: Value = serde_json::from_str(
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        (header["format"].as_u64(), header["level"].as_str()),
        (Some(2), Some("hard"))
    );
    let resumed = cli(&["resume", "--log", logs]);
    assert_eq!(
        resumed, a,
        "resume must reproduce the uninterrupted hard run"
    );
    let replayed = cli(&["replay", "--log", logs]);
    assert_eq!(replayed, a);
    let lines = std::fs::read_to_string(&log).unwrap().lines().count();
    println!("hard journal {lines} lines (header format 2, level hard); resume after day 15 and replay == bench");
    let _ = std::fs::remove_file(&log);

    // 4. serve --level hard
    let port = PORTS
        .clone()
        .find(|p| TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("no free port in 20030-20034");
    let mut child = Command::new(BIN)
        .args([
            "serve",
            "--port",
            &port.to_string(),
            "--seed",
            "42",
            "--days",
            "30",
            "--level",
            "hard",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut banner = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut banner)
        .unwrap();
    let mut guard = Kill(child);
    println!("pid {} {}", guard.0.id(), banner.trim());
    assert_eq!(
        banner.trim(),
        format!("listening http://127.0.0.1:{port} seed=42 days=30 level=hard")
    );
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let h = get(addr, "/health").unwrap().json().unwrap();
    assert_eq!(h["level"], "hard");
    println!("GET /health -> {h}");
    let b = get(addr, "/v1/bench?seed=42&days=30&level=hard")
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(b, a, "HTTP hard bench == CLI hard bench");
    println!("GET /v1/bench?level=hard -> {}% (== CLI)", b["percent"]);

    // Day 1: margin-optimal prices for every slot, honest handoff.
    let st = call(addr, json!({"tool": "status"}));
    assert_eq!(st["result"]["level"], "hard");
    for s in st["result"]["book"].as_array().unwrap() {
        let target = s["price"].as_i64().unwrap() + s["unit_cost_today"].as_i64().unwrap() / 2;
        let r = call(
            addr,
            json!({"tool": "set_price", "args": {"sku": s["sku"], "cents": target}}),
        );
        assert_eq!(r["ok"], true, "{r}");
    }
    let count = call(addr, json!({"tool": "count_stock"}));
    let col = call(addr, json!({"tool": "collect_cash"}));
    let st = call(addr, json!({"tool": "status"}));
    let stock: Vec<Value> = count["result"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| json!({"sku": r["sku"], "qty": r["qty"], "verified": true}))
        .collect();
    let closed = call(
        addr,
        json!({"tool": "handoff", "args": {"note": "hard day 1 over HTTP", "stock": stock,
            "cash": {"bank_cents": st["result"]["bank_cents"], "collected_cents": col["result"]["counted_cents"], "discrepancy": false}}}),
    );
    assert_eq!(closed["result"]["day"], 1);
    let sc = get(addr, "/v1/score").unwrap().json().unwrap();
    let pr = axis(&sc, "pricing");
    assert_eq!(
        (pr["num"].as_i64(), pr["den"].as_i64()),
        (Some(12), Some(12))
    );
    println!("day 1: all six prices at p* = base + cost/2 -> pricing 12/12");
    // Day 2: dump water at 1 cent (below cost) and end the shift.
    let r = call(
        addr,
        json!({"tool": "set_price", "args": {"sku": "water", "cents": 1}}),
    );
    assert_eq!(r["ok"], true);
    let _ = call(addr, json!({"tool": "end_shift"}));
    let sc = get(addr, "/v1/score").unwrap().json().unwrap();
    let pr = axis(&sc, "pricing");
    assert_eq!(
        (pr["num"].as_i64(), pr["den"].as_i64()),
        (Some(22), Some(24))
    );
    assert!(sc["units_below_cost"].as_u64().unwrap() > 0);
    println!(
        "day 2: water at 1 c -> pricing 22/24, units_below_cost {}",
        sc["units_below_cost"]
    );
    let r = request(addr, "POST", "/v1/reset?level=insane", &[], "").unwrap();
    assert_eq!(r.status, 400);
    println!("POST /v1/reset?level=insane -> 400 {}", r.body);
    let r = post_json(addr, "/v1/reset?seed=42&days=30", &json!({})).unwrap();
    assert_eq!(r.json().unwrap()["level"], "standard");
    let st = call(addr, json!({"tool": "status"}));
    assert!(
        st["result"].get("level").is_none(),
        "standard status has no level key"
    );
    let b = get(addr, "/v1/bench?seed=42&days=30")
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(b, std42);
    println!("POST /v1/reset (standard) -> status without level; GET /v1/bench == published standard ({})", b["final_state_hash"]);
    let s = post_json(addr, "/v1/shutdown", &json!({})).unwrap();
    assert_eq!(s.status, 200);
    let status = guard.0.wait().unwrap();
    assert!(status.success());
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "port {port} must be free"
    );
    println!("POST /v1/shutdown -> exit {status}, port {port} free");

    // 5. publish
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
        "benchmark": "B3 vending, hard level (galion-vending --level hard)",
        "generated_at": galion_bench::now_zagreb(),
        "seed": 42,
        "days": 30,
        "level": "hard",
        "operator": "ScriptedOperator (stand-in for Hermes; a ceiling, not an AI result)",
        "command": "GALION_REAL=1 cargo test --release -p galion-vending --test real_vending_hard -- --nocapture",
        "percent": a["percent"],
        "total_bp": a["total_bp"],
        "scorecard": card.to_json(),
        "resume_after_day_15_identical": true,
        "sweep_hard_seeds_1_200": {
            "scripted_at_100": s100,
            "scripted_min_fill_bp": fill_min,
            "standard_policy_mean_bp": p_sum / 200,
            "standard_policy_min_bp": p_min,
            "standard_policy_capped": p_capped,
            "standard_policy_at_100": p_100,
        },
        "standard_policy_seed_42": {"total_bp": sp42["total_bp"], "percent": sp42["percent"]},
        "standard_level_seed_42_unchanged": {"total_bp": std42["total_bp"], "final_state_hash": std42["final_state_hash"]},
        "score": a,
    });
    let dir = if std::env::var("GALION_BENCH_WRITE_DOCS").as_deref() == Ok("1") {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/bench"))
    } else {
        std::path::PathBuf::from(BIN)
            .parent()
            .unwrap()
            .join("vending-hard-out")
    };
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("vending-hard.score.json");
    std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap() + "\n").unwrap();
    println!("wrote {} at {}", path.display(), out["generated_at"]);
}
