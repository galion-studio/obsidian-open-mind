// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1): builds the RELEASE binary, starts it on
//! 127.0.0.1:19521 (slot 52 range 19520-19529), drives a scripted Hermes ticket
//! through open / oversized order / kill switch / close / reset over real HTTP,
//! checks the P&L report is reproducible after /reset, then kills the server.
//! Paper only: the server has no route to any live broker.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 19521;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn release_bin() -> PathBuf {
    let debug = PathBuf::from(env!("CARGO_BIN_EXE_galion-wallstreet"));
    let target = debug.parent().unwrap().parent().unwrap().to_path_buf();
    let st = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", "galion-wallstreet", "-j", "2"])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("cargo build --release");
    assert!(st.success());
    target.join("release/galion-wallstreet")
}

fn http(method: &str, path: &str, body: &str) -> (u16, String) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", PORT)) {
            s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            let req = format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(req.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            let code: u16 = out
                .split_whitespace()
                .nth(1)
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            let b = out
                .split_once("\r\n\r\n")
                .map(|x| x.1)
                .unwrap_or("")
                .to_string();
            eprintln!("{method} {path} -> {code}");
            return (code, b);
        }
        assert!(Instant::now() < deadline, "server did not start on {PORT}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn jv(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

/// One scripted session; returns the report fingerprint.
fn session() -> String {
    let (c, b) = http("POST", "/reset", "");
    assert_eq!(c, 200, "{b}");
    let (c, b) = http("GET", "/quote?symbol=GLXA", "");
    assert_eq!(c, 200);
    assert_eq!(jv(&b)["delayed"], true);

    let (c, b) = http(
        "POST",
        "/order",
        r#"{"symbol":"GLXA","side":"buy","qty":200,"ticket":"REAL-1","rationale":"momentum, public earnings"}"#,
    );
    assert_eq!(c, 200, "{b}");
    assert_eq!(jv(&b)["status"], "filled");

    let (c, b) = http("POST", "/advance", r#"{"days":10}"#);
    assert_eq!(c, 200, "{b}");

    // Oversized: refused AND trips the kill switch.
    let (c, b) = http(
        "POST",
        "/order",
        r#"{"symbol":"GLXA","side":"buy","qty":100000,"ticket":"REAL-1"}"#,
    );
    assert_eq!(c, 422, "{b}");
    assert_eq!(jv(&b)["refusal"]["code"], "kill_switch_oversized_order");
    let (_, acct) = http("GET", "/account", "");
    assert_eq!(jv(&acct)["kill_switch"]["engaged"], true);

    // New risk refused while engaged; closing allowed.
    let (c, b) = http(
        "POST",
        "/order",
        r#"{"symbol":"HRMS","side":"buy","qty":1,"ticket":"REAL-1"}"#,
    );
    assert_eq!(c, 422);
    assert_eq!(jv(&b)["refusal"]["code"], "kill_switch_engaged");
    let (c, b) = http("POST", "/close", r#"{"symbol":"GLXA","ticket":"REAL-1"}"#);
    assert_eq!(c, 200, "{b}");
    assert_eq!(jv(&b)["status"], "filled");

    // Restricted symbol and insider rationale refused.
    let (c, _) = http("POST", "/kill/reset", r#"{"operator":"real-test"}"#);
    assert_eq!(c, 200);
    let (c, b) = http(
        "POST",
        "/order",
        r#"{"symbol":"RSTR","side":"buy","qty":1,"ticket":"REAL-1"}"#,
    );
    assert_eq!(
        (c, jv(&b)["refusal"]["code"].clone()),
        (422, Value::from("restricted_symbol"))
    );
    let (c, b) = http(
        "POST",
        "/order",
        r#"{"symbol":"OMND","side":"buy","qty":1,"ticket":"REAL-1","rationale":"my friend at Open Mind leaked the numbers"}"#,
    );
    assert_eq!(
        (c, jv(&b)["refusal"]["code"].clone()),
        (422, Value::from("compliance_insider_information"))
    );

    let (c, b) = http("GET", "/report?ticket=REAL-1", "");
    assert_eq!(c, 200);
    let r = jv(&b);
    let a = &r["account"];
    assert_eq!(a["mode"], "paper");
    let (re, un, fe, pnl) = (
        a["realized_cents"].as_i64().unwrap(),
        a["unrealized_cents"].as_i64().unwrap(),
        a["fees_cents"].as_i64().unwrap(),
        a["pnl_cents"].as_i64().unwrap(),
    );
    assert_eq!(re + un - fe, pnl);
    assert_eq!(r["fills"].as_array().unwrap().len(), 2);
    let (c, md) = http("GET", "/report.md?ticket=REAL-1", "");
    assert_eq!(c, 200);
    assert!(md.contains("## What to do next"));
    eprintln!("REAL-1 pnl_cents={pnl} fingerprint={}", r["fingerprint"]);
    r["fingerprint"].as_str().unwrap().to_string()
}

#[test]
fn real_paper_broker_over_http() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let bin = release_bin();
    let _srv = Kill(
        Command::new(&bin)
            .args(["serve", "--bind", &format!("127.0.0.1:{PORT}")])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start release binary"),
    );
    let (c, b) = http("GET", "/health", "");
    assert_eq!(c, 200);
    let h = jv(&b);
    assert_eq!(h["mode"], "paper");
    assert_eq!(h["live_broker"], false);
    assert!(!h["time"].as_str().unwrap().ends_with('Z'));

    let f1 = session();
    let f2 = session();
    assert_eq!(f1, f2, "P&L report must be reproducible");

    // Whole scripted Hermes ticket in one call.
    let t = std::fs::read_to_string(format!(
        "{}/fixtures/tickets/scripted-open-close.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let (c, b) = http("POST", "/ticket", &t);
    assert_eq!(c, 200, "{b}");
    let v = jv(&b);
    let codes: Vec<&str> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["result"].as_str().unwrap())
        .collect();
    eprintln!("ticket steps: {codes:?}");
    assert!(codes.contains(&"kill_switch_oversized_order"));
    let t2 = std::fs::read_to_string(format!(
        "{}/fixtures/tickets/insider-refusal.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let (c, b) = http("POST", "/ticket", &t2);
    assert_eq!(c, 422, "{b}");

    let (c, b) = http("GET", "/score", "");
    assert_eq!(c, 200);
    assert_eq!(jv(&b)["percent"], 100);
    eprintln!("score percent=100 total={}", jv(&b)["total"]);
}
