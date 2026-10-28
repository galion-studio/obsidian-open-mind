// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! GALION_REAL=1 real test for the `hermes-bank` binary (wave 4 slot 366):
//! starts the real binary on loopback ports 22360-22369 (override the base
//! with HERMES_BANK_REAL_PORT), hits it over HTTP and tears it down. Fixture
//! files only: no OpenRouter or River call, no key file.
//!
//! Run: `GALION_REAL=1 cargo test --release -p hermes-rs --test real_bank -- --nocapture`

#![cfg(feature = "sqlite")]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hermes-bank");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn base() -> u16 {
    std::env::var("HERMES_BANK_REAL_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22360)
}

fn fx(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bank")
        .join(name)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn req(port: u16, raw: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

fn get(port: u16, path: &str, extra: &str) -> (u16, String, String) {
    req(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn wait_up(port: u16) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("hermes-bank did not listen on {port}");
}

/// Same rows as tests/bank.rs `fixture_dbs`, relative to the real clock.
fn make_dbs(dir: &Path, now: i64) -> (PathBuf, PathBuf) {
    let mk = |p: &Path, rows: &[(&str, String, i64, i64, f64)]| {
        let c = rusqlite::Connection::open(p).unwrap();
        c.execute_batch(hermes_rs::session_sqlite::SCHEMA).unwrap();
        for (id, at, i, o, cost) in rows {
            c.execute_batch(&format!("INSERT INTO sessions (id, source, started_at, last_activity_at, input_tokens, output_tokens, estimated_cost_usd) VALUES ('{id}', 'api', 1700000000, {at}, {i}, {o}, {cost});")).unwrap();
        }
    };
    let h = |x: i64| (now - x * 3600).to_string();
    let own = dir.join("rs-state.db");
    let legacy = dir.join("state.db");
    mk(
        &own,
        &[
            ("s-own-1", h(1), 1000, 100, 0.9),
            ("s-own-2", h(48), 2000, 200, 0.3),
            ("s-own-old", h(8 * 24), 500000, 50000, 9.0),
            ("s-dup", h(1), 10, 1, 0.05),
        ],
    );
    mk(
        &legacy,
        &[
            ("s-dup", h(1), 99999, 9999, 5.0),
            (
                "s-legacy-1",
                "'2026-10-18 21:12:00+02:00'".into(),
                7000,
                700,
                2.0,
            ),
        ],
    );
    (own, legacy)
}

#[test]
fn real_hermes_bank_serves_budget_aware_snapshot() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let port = base();
    let dir = std::env::temp_dir().join(format!("hermes-bank-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let now = hermes_rs::time::now_secs();
    let (own, legacy) = make_dbs(&dir, now);
    let ledger = dir.join("rs-spend.json");
    let day = hermes_rs::time::zagreb_day(now);
    std::fs::write(
        &ledger,
        format!("{{\"day\":\"{day}\",\"day_usd\":0.4755,\"day_calls\":20}}"),
    )
    .unwrap();
    let common = |cmd: &str| {
        let mut c = Command::new(BIN);
        c.arg(cmd)
            .arg("--or-key-json")
            .arg(fx("key-budget-binds.json"))
            .arg("--river-status")
            .arg("200")
            .arg("--survival")
            .arg(fx("survival.json"))
            .arg("--state-db")
            .arg(&own)
            .arg("--state-db")
            .arg(&legacy)
            .arg("--spend-json")
            .arg(&ledger);
        c
    };

    // report mode
    let t = Instant::now();
    let out = common("report").output().unwrap();
    let report_ms = t.elapsed().as_millis();
    assert!(out.status.success());
    let rep: Value = serde_json::from_slice(&out.stdout).unwrap();
    eprintln!("report {} B in {report_ms} ms", out.stdout.len());

    // serve mode
    let child = common("serve")
        .arg("--bind")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--refresh-s")
        .arg("1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = Kill(child);
    wait_up(port);
    let t = Instant::now();
    let (st, head, body) = get(
        port,
        "/api/v1/bank",
        "Cookie: galion_admin=fixture-cookie-value\r\nAuthorization: Bearer fixture-not-a-key\r\n",
    );
    let ms = t.elapsed().as_millis();
    assert_eq!(st, 200, "{head}");
    assert!(head
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    let v: Value = serde_json::from_str(&body).unwrap();
    eprintln!("GET /api/v1/bank 200 {} B {ms} ms", body.len());
    eprintln!("purse0 {}", v["purses"][0]);
    eprintln!("note {}", v["note"]);
    eprintln!("budget {}", v["budget"]);
    eprintln!("localSpend {}", v["localSpend"]);
    assert_eq!(v["purses"][0]["usd"], json!(2.75), "effective_budget binds");
    assert_eq!(v["binding"], "budget");
    assert_eq!(v["weekTokensIn"], json!(3010));
    assert!((v["weekCostUsd"].as_f64().unwrap() - 1.25).abs() < 1e-9);
    assert_eq!(v["localSpend"]["todayCalls"], json!(20));
    assert_eq!(v["localSpend"]["duplicateRows"], json!(1));
    assert_eq!(v["stale"], false);
    assert!(v["ageS"].as_u64().unwrap() <= 2);
    assert!(!body.contains("fixture-cookie-value") && !body.contains("fixture-not-a-key"));
    // report == serve minus serve-only keys (asOf may differ by a second)
    let strip = |mut x: Value| {
        let o = x.as_object_mut().unwrap();
        for k in ["ageS", "stale", "source", "asOf"] {
            o.remove(k);
        }
        x
    };
    assert_eq!(strip(rep), strip(v.clone()));

    // gate cases
    let (st, head, _) = req(
        port,
        "POST /api/v1/bank HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(st, 405);
    assert!(head.contains("Allow: GET, HEAD"), "{head}");
    let (st, _, body) = req(
        port,
        "HEAD /api/v1/bank HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert_eq!((st, body.len()), (200, 0));
    assert_eq!(get(port, "/api/v1/bankx", "").0, 404);
    assert_eq!(get(port, "/api/spend", "").0, 404);
    assert_eq!(get(port, "/health", "").0, 200);
    assert_eq!(get(port, "/api/v1/bank?x=1", "").0, 200);
    eprintln!(
        "gates: POST 405 (Allow), HEAD 200 empty, /api/v1/bankx 404, /api/spend 404, /health 200"
    );

    let t = Instant::now();
    for _ in 0..200 {
        assert_eq!(get(port, "/api/v1/bank", "").0, 200);
    }
    eprintln!("200 x GET /api/v1/bank {} ms", t.elapsed().as_millis());

    // refresh: a ledger change on disk shows up after a refresh
    std::fs::write(
        &ledger,
        format!("{{\"day\":\"{day}\",\"day_usd\":1.5,\"day_calls\":33}}"),
    )
    .unwrap();
    let t = Instant::now();
    loop {
        let v: Value = serde_json::from_str(&get(port, "/api/v1/bank", "").2).unwrap();
        if v["localSpend"]["todayCalls"] == json!(33) {
            eprintln!(
                "refresh picked up new ledger after {} ms",
                t.elapsed().as_millis()
            );
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(5), "no refresh");
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.0.kill();
    let _ = child.0.wait();
    let mut err = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(
        !err.contains("fixture-cookie-value") && !err.contains("Bearer"),
        "{err}"
    );

    // non-loopback bind refused, bad args exit 2
    let out = Command::new(BIN)
        .args(["serve", "--bind", "0.0.0.0:1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(BIN).args(["nonsense"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    std::thread::sleep(Duration::from_millis(200));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
