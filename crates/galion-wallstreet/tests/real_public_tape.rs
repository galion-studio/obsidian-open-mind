// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1), slot 174 bench-ws-ecb-data: builds the
//! RELEASE binary and serves the PAPER broker on the REAL-PUBLIC tapes:
//! ECB reference rates on 127.0.0.1:20440 and Treasury par-yield proxies on
//! 127.0.0.1:20441 (slot range 20440-20449). Drives a full replay session over
//! real HTTP, checks the hand-computed P&L, attribution on every surface,
//! /reset keeping the tape, and the CLI. Kills both servers at the end.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const FX_PORT: u16 = 20440;
const UST_PORT: u16 = 20441;

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

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
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
            eprintln!(":{port} {method} {path} -> {code}");
            return (code, b);
        }
        assert!(Instant::now() < deadline, "server did not start on {port}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn jv(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or(Value::Null)
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

fn serve(bin: &PathBuf, port: u16, tape: &str) -> Kill {
    Kill(
        Command::new(bin)
            .args([
                "serve",
                "--bind",
                &format!("127.0.0.1:{port}"),
                "--tape",
                tape,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start release binary"),
    )
}

/// One EUR/USD replay session; returns (pnl_cents, report fingerprint).
fn fx_session() -> (i64, String) {
    let p = FX_PORT;
    let (c, b) = http(p, "POST", "/reset", "");
    assert_eq!(
        (c, jv(&b)["as_of"].clone()),
        (200, Value::from("2026-08-26"))
    );
    let (c, b) = http(p, "GET", "/quote?symbol=EURUSD", "");
    assert_eq!(c, 200);
    let q = jv(&b);
    assert_eq!(q["price"], "116.69");
    assert_eq!(q["delayed"], true);
    assert!(q["source"]
        .as_str()
        .unwrap()
        .contains("Source: ECB statistics."));
    let (c, b) = http(
        p,
        "POST",
        "/order",
        r#"{"symbol":"EURUSD","side":"buy","qty":100,"ticket":"B2B-REAL","rationale":"replay of ECB reference rates"}"#,
    );
    assert_eq!(c, 200, "{b}");
    assert_eq!(jv(&b)["fill"]["price"], "116.75");
    // oversized: 300 lots = 35 007.00 > 25 000.00 -> refused + kill switch
    let (c, b) = http(
        p,
        "POST",
        "/order",
        r#"{"symbol":"EURUSD","side":"buy","qty":300,"ticket":"B2B-REAL"}"#,
    );
    assert_eq!(c, 422);
    assert_eq!(jv(&b)["refusal"]["code"], "kill_switch_oversized_order");
    let (c, b) = http(
        p,
        "POST",
        "/order",
        r#"{"symbol":"EURGBP","side":"buy","qty":1,"ticket":"B2B-REAL"}"#,
    );
    assert_eq!(c, 422);
    // engaged switch is checked first; after reset GBP is still restricted
    let code = jv(&b)["refusal"]["code"].as_str().unwrap().to_string();
    assert!(
        code == "kill_switch_engaged" || code == "restricted_symbol",
        "{code}"
    );
    let (c, _) = http(p, "POST", "/kill/reset", r#"{"operator":"real-test-174"}"#);
    assert_eq!(c, 200);
    let (c, b) = http(
        p,
        "POST",
        "/order",
        r#"{"symbol":"EURGBP","side":"buy","qty":1,"ticket":"B2B-REAL"}"#,
    );
    assert_eq!(
        (c, jv(&b)["refusal"]["code"].clone()),
        (422, Value::from("restricted_symbol"))
    );
    let (c, b) = http(p, "POST", "/advance", r#"{"days":100}"#);
    assert_eq!(
        (c, jv(&b)["as_of"].clone()),
        (200, Value::from("2026-10-20"))
    );
    let (c, b) = http(
        p,
        "POST",
        "/close",
        r#"{"symbol":"EURUSD","ticket":"B2B-REAL"}"#,
    );
    assert_eq!(c, 200, "{b}");
    assert_eq!(jv(&b)["fill"]["price"], "112.09");
    let (c, b) = http(p, "GET", "/report?ticket=B2B-REAL", "");
    assert_eq!(c, 200);
    let r = jv(&b);
    let a = &r["account"];
    let pnl = a["pnl_cents"].as_i64().unwrap();
    assert_eq!(
        a["realized_cents"].as_i64().unwrap() + a["unrealized_cents"].as_i64().unwrap()
            - a["fees_cents"].as_i64().unwrap(),
        pnl
    );
    let (c, md) = http(p, "GET", "/report.md?ticket=B2B-REAL", "");
    assert_eq!(c, 200);
    assert!(md.contains("PAPER"));
    (pnl, r["fingerprint"].as_str().unwrap().to_string())
}

#[test]
fn real_public_tapes_over_http() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let bin = release_bin();
    let t0 = Instant::now();
    let fx = serve(&bin, FX_PORT, "ecb-fx");
    let ust = serve(&bin, UST_PORT, "ust");
    let (c, b) = http(FX_PORT, "GET", "/health", "");
    eprintln!("fx first /health after {} ms", t0.elapsed().as_millis());
    assert_eq!(c, 200);
    let h = jv(&b);
    assert_eq!(
        (h["mode"].clone(), h["live_broker"].clone()),
        ("paper".into(), false.into())
    );
    assert_eq!(
        (h["tape"].clone(), h["data_class"].clone()),
        ("ecb-fx".into(), "REAL-PUBLIC".into())
    );
    assert_eq!(h["attribution"], "Source: ECB statistics.");
    assert!(
        h["time"].as_str().unwrap().ends_with("+02:00")
            || h["time"].as_str().unwrap().ends_with("+01:00")
    );

    let (c, b) = http(FX_PORT, "GET", "/data", "");
    assert_eq!(c, 200);
    let d = jv(&b);
    assert_eq!(
        (
            d["days"].clone(),
            d["first_day"].clone(),
            d["last_day"].clone()
        ),
        (40.into(), "2026-08-26".into(), "2026-10-20".into())
    );
    assert!(d["terms_urls"].to_string().contains("usage_policy"));
    let (c, n) = http(FX_PORT, "GET", "/data/notice", "");
    assert_eq!(c, 200);
    assert!(n.contains("Source: ECB statistics.") && n.contains("NOT MIT"));
    let (c, b) = http(FX_PORT, "GET", "/universe", "");
    assert_eq!(c, 200);
    assert!(b.contains("EURUSD") && b.contains("EURJPY"));

    let (p1, f1) = fx_session();
    let (p2, f2) = fx_session();
    eprintln!("B2B-REAL pnl_cents={p1} fingerprint={f1}");
    assert_eq!(p1, p2);
    assert_eq!(f1, f2, "real-tape report must be reproducible after /reset");
    // 100 lots in at 116.75, out at 112.09, 5 bps fee each side
    // refused orders add no fills -> -47 744 cents
    assert_eq!(p1, -47_744);

    let (c, b) = http(FX_PORT, "GET", "/backtest?data=ecb-fx", "");
    assert_eq!(c, 200);
    let bt = jv(&b);
    assert_eq!(bt.as_array().unwrap().len(), 4);
    assert_eq!(bt[0]["result"]["fingerprint"], "fnv1a64:0ea17fa4f45eaaed");
    let (c, _) = http(FX_PORT, "GET", "/backtest?data=yahoo", "");
    assert_eq!(c, 400);
    let (c, b) = http(FX_PORT, "GET", "/backtest", "");
    assert_eq!(
        (c, jv(&b)["fingerprint"].clone()),
        (200, Value::from("fnv1a64:19424da702570a34"))
    );

    // Treasury tape
    let (c, b) = http(UST_PORT, "GET", "/health", "");
    assert_eq!((c, jv(&b)["tape"].clone()), (200, Value::from("ust")));
    let (c, b) = http(UST_PORT, "GET", "/quote?symbol=UST10Y", "");
    assert_eq!(c, 200);
    assert!(jv(&b)["source"]
        .as_str()
        .unwrap()
        .contains("Not endorsed by the U.S. Treasury"));
    let (c, b) = http(
        UST_PORT,
        "POST",
        "/order",
        r#"{"symbol":"UST10Y","side":"buy","qty":100,"ticket":"B2B-UST"}"#,
    );
    assert_eq!(c, 200, "{b}");
    let (c, _) = http(UST_PORT, "POST", "/advance", r#"{"days":100}"#);
    assert_eq!(c, 200);
    let (c, b) = http(UST_PORT, "GET", "/quote?symbol=UST10Y", "");
    assert_eq!(
        (c, jv(&b)["price"].clone(), jv(&b)["as_of"].clone()),
        (200, Value::from("60.32"), Value::from("2026-10-20"))
    );
    let (c, b) = http(UST_PORT, "GET", "/backtest?data=ust", "");
    assert_eq!(c, 200);
    assert_eq!(
        jv(&b)[0]["result"]["fingerprint"],
        "fnv1a64:c55c83952c5c599d"
    );
    let (c, b) = http(UST_PORT, "POST", "/reset", "");
    assert_eq!(
        (c, jv(&b)["as_of"].clone()),
        (200, Value::from("2026-08-24"))
    );

    eprintln!(
        "RSS fx={} KiB ust={} KiB",
        rss_kib(fx.0.id()),
        rss_kib(ust.0.id())
    );

    // CLI
    let out = Command::new(&bin).args(["data", "ust"]).output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        jv(&String::from_utf8_lossy(&out.stdout))["data_class"],
        "REAL-PUBLIC"
    );
    let out = Command::new(&bin)
        .args(["data", "--notice"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Source: ECB statistics."));
    let out = Command::new(&bin)
        .args(["backtest", "--data", "ecb-fx"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = Command::new(&bin)
        .args(["serve", "--tape", "stooq", "--bind", "127.0.0.1:20442"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown tape refused before binding"
    );
    drop(fx);
    drop(ust);
    assert!(
        TcpStream::connect(("127.0.0.1", FX_PORT)).is_err(),
        "fx server stopped"
    );
}
