// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1) for the B2 adversarial compliance set:
//! builds the RELEASE binary, starts it on 127.0.0.1:20021 (slot 132 range
//! 20020-20029) and sends every committed adversarial case (dev, holdout,
//! blind1) over real HTTP to `/compliance/check` and, as an order rationale, to
//! `/order`. An attack must come back refused under its expected category on
//! both, with no position opened; a benign case must be allowed and fill.
//! `GET /adversarial` must agree with the in-process board. Then the server is
//! killed. Paper only: the server has no route to any live broker.

use galion_wallstreet::adversarial;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Slot 363 (real-test-port-env): `WALLSTREET_ADV_REAL_PORT` moves the port (unset = 20021, slot 132).
const PORT_ENV: &str = "WALLSTREET_ADV_REAL_PORT";
const DEFAULT_PORT: u16 = 20021;

fn port() -> u16 {
    galion_realtest::ports::base_from_env(PORT_ENV, DEFAULT_PORT, 1)
}

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

fn http(method: &str, path: &str, body: &str) -> (u16, Value) {
    let port = port();
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
            let b = out.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
            return (code, serde_json::from_str(b).unwrap_or(Value::Null));
        }
        assert!(Instant::now() < deadline, "server did not start on {port}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn cat_str(v: &Value) -> Option<String> {
    v.get("category")
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[test]
fn real_adversarial_set_over_http() {
    let port = port();
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let bin = release_bin();
    let _srv = Kill(
        Command::new(&bin)
            .args(["serve", "--bind", &format!("127.0.0.1:{port}")])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start release binary"),
    );
    let (c, h) = http("GET", "/health", "");
    assert_eq!(c, 200);
    assert_eq!(h["mode"], "paper");
    assert_eq!(h["live_broker"], false);

    let started = Instant::now();
    let (mut screen_ok, mut order_ok, mut total) = (0, 0, 0);
    let mut fails = Vec::new();
    for (set, case) in adversarial::cases() {
        total += 1;
        let want = case.expect.map(|c| {
            serde_json::to_value(c)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        });
        let body = json!({ "text": case.text }).to_string();
        let (c, v) = http("POST", "/compliance/check", &body);
        assert_eq!(c, 200);
        let got = cat_str(&v);
        if got == want {
            screen_ok += 1;
        } else {
            fails.push(format!(
                "{set}/{} screen want {want:?} got {got:?}",
                case.id
            ));
        }
        // Same text as an order rationale on a fresh book.
        assert_eq!(http("POST", "/reset", "{}").0, 200);
        let order = json!({
            "symbol": "GLXA", "side": "buy", "qty": 1,
            "rationale": case.text, "ticket": format!("REAL-ADV-{}", case.id)
        })
        .to_string();
        let (c, o) = http("POST", "/order", &order);
        let (_, pos) = http("GET", "/positions", "");
        let open = pos["positions"]
            .as_array()
            .map(|a| a.iter().any(|p| p["qty"].as_i64().unwrap_or(0) != 0))
            .unwrap_or(false);
        let code = match o["status"].as_str() {
            Some("refused") => o["refusal"]["code"].as_str().unwrap_or("").to_string(),
            Some(s) => s.to_string(),
            None => String::new(),
        };
        let ok = match &want {
            Some(w) => c == 422 && code == format!("compliance_{w}") && !open,
            None => c == 200 && code == "filled" && open,
        };
        if ok {
            order_ok += 1;
        } else {
            fails.push(format!(
                "{set}/{} order want {want:?} got http {c} code {code} open {open}",
                case.id
            ));
        }
    }
    let ms = started.elapsed().as_millis();
    eprintln!(
        "REAL adversarial over HTTP: screen {screen_ok}/{total}, order {order_ok}/{total}, {ms} ms"
    );
    assert!(fails.is_empty(), "{fails:#?}");

    let (c, b) = http("GET", "/adversarial", "");
    assert_eq!(c, 200);
    let local = adversarial::run();
    assert_eq!(b["total"], json!(local.total));
    assert_eq!(b["passed"], json!(local.passed));
    assert_eq!(b["v1_passed"], json!(local.v1_passed));
    assert_eq!(b["percent"], json!(100));
    assert!(!b["generated_at"].as_str().unwrap().ends_with('Z'));
    eprintln!(
        "GET /adversarial: {}% ({}/{}), v1 {}% ({}/{})",
        b["percent"], b["passed"], b["total"], b["v1_percent"], b["v1_passed"], b["total"]
    );
}
