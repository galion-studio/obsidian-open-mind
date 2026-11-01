// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test: the built binary on 127.0.0.1:19970 (public) and :19971
//! (internal), production argon2 cost, a real SQLite file. Loopback only, no
//! Stripe (it must report "not connected"). Gated:
//!
//! ```text
//! GALION_REAL=1 cargo test -p galion-accounts --test real_binary -- --ignored --nocapture
//! ```

use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_accounts::client;
use galion_accounts::plans::Plan;
use galion_common::http::{send, ClientRequest, ClientResponse};

const BIN: &str = env!("CARGO_BIN_EXE_galion-accounts");
const PUBLIC: &str = "127.0.0.1:19970";
const INTERNAL: &str = "127.0.0.1:19971";
const TOKEN: &str = "real-test-internal-token";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn call(
    addr: &str,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<&str>,
) -> ClientResponse {
    let a: SocketAddr = addr.parse().unwrap();
    let b = body.unwrap_or("").as_bytes();
    let mut r = if method == "GET" {
        ClientRequest::get(path, addr)
    } else {
        ClientRequest::post(path, addr, "application/json", b)
    };
    if let Some(c) = cookie {
        r.headers.push(("Cookie".into(), c.into()));
    }
    send(a, &r, Duration::from_secs(10), 1 << 20).expect("request")
}

fn j(r: &ClientResponse) -> serde_json::Value {
    serde_json::from_slice(&r.body).unwrap_or_default()
}

#[test]
#[ignore = "real-life: set GALION_REAL=1 and pass --ignored"]
fn real_binary_end_to_end() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: GALION_REAL=1 not set");
        return;
    }
    let dir = std::path::PathBuf::from(format!(
        "/workspace/swarm/tmp/accounts/real-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let pepper = dir.join("pepper");
    let db = dir.join("accounts.sqlite");
    let out = Command::new(BIN)
        .args(["new-pepper", "--out", pepper.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    // Refuses to overwrite an existing pepper.
    let again = Command::new(BIN)
        .args(["new-pepper", "--out", pepper.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!again.status.success());

    // stripe-setup dry run: no key needed, nothing sent.
    let dry = Command::new(BIN)
        .args([
            "stripe-setup",
            "--webhook-url",
            "https://galion.app/webhooks/stripe",
        ])
        .env_remove("STRIPE_SECRET_KEY")
        .output()
        .unwrap();
    let dry_txt = String::from_utf8_lossy(&dry.stdout);
    assert!(dry.status.success(), "{dry_txt}");
    assert!(dry_txt.contains("EUR 20.00/month") && dry_txt.contains("EUR 220.00/month"));

    let child = Command::new(BIN)
        .args([
            "serve",
            "--listen",
            PUBLIC,
            "--internal",
            INTERNAL,
            "--db",
            db.to_str().unwrap(),
            "--pepper-file",
            pepper.to_str().unwrap(),
            "--insecure-cookies",
        ])
        .env("ACCOUNTS_INTERNAL_TOKEN", TOKEN)
        .env(
            "ACCOUNTS_CASH_ADDRESS",
            "Galion (test)|PO Box 0|10000 Zagreb",
        )
        .env_remove("STRIPE_SECRET_KEY")
        .env_remove("STRIPE_WEBHOOK_SECRET")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _k = Kill(child);
    let t0 = Instant::now();
    while std::net::TcpStream::connect(PUBLIC).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let page = call(PUBLIC, "GET", "/account", None, None);
    assert_eq!(page.status, 200);
    assert!(page.header("content-security-policy").is_some());

    let t = Instant::now();
    let cr = call(PUBLIC, "POST", "/api/account/create", None, Some("{}"));
    let create_ms = t.elapsed().as_millis();
    assert_eq!(cr.status, 200);
    let number = j(&cr)["number"].as_str().unwrap().to_string();
    let cookie = cr
        .header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let t = Instant::now();
    let li = call(
        PUBLIC,
        "POST",
        "/api/account/login",
        None,
        Some(&format!(r#"{{"number":"{number}"}}"#)),
    );
    let login_ms = t.elapsed().as_millis();
    assert_eq!(li.status, 200);

    let me = j(&call(PUBLIC, "GET", "/api/account/me", Some(&cookie), None));
    assert_eq!(me["plan"], "free");
    assert_eq!(me["methods"]["stripe"]["connected"], false);
    assert_eq!(me["methods"]["cash"]["connected"], true);
    let co = call(
        PUBLIC,
        "POST",
        "/api/pay/stripe/checkout",
        Some(&cookie),
        Some(r#"{"plan":"plus"}"#),
    );
    assert_eq!(co.status, 503);

    // Cash: reference, then the operator CLI credits it.
    let cash = j(&call(
        PUBLIC,
        "POST",
        "/api/pay/cash",
        Some(&cookie),
        Some(r#"{"plan":"pro"}"#),
    ));
    let reference = cash["reference"].as_str().unwrap().to_string();
    let cli = Command::new(BIN)
        .args([
            "cash-credit",
            "--ref",
            &reference,
            "--amount-cents",
            "22000",
            "--db",
            db.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(String::from_utf8_lossy(&cli.stdout).contains("\"days_added\":30"));

    // The consumer helper (what the console / Galion One / Hermes call).
    let session = cookie.split_once('=').unwrap().1;
    let e = client::check_session(INTERNAL.parse().unwrap(), TOKEN, session);
    assert_eq!(e.plan, Plan::Pro);
    assert!(e.active && e.at_least(Plan::Plus) && e.at_least(Plan::Pro));
    let bad = client::check_session(INTERNAL.parse().unwrap(), "wrong", session);
    assert_eq!(bad.plan, Plan::Free);
    assert!(!bad.at_least(Plan::Plus));

    // The DB file holds no account number.
    let mut raw = std::fs::read(&db).unwrap();
    raw.extend(std::fs::read(dir.join("accounts.sqlite-wal")).unwrap_or_default());
    let digits = number.replace(' ', "");
    assert!(!raw.windows(digits.len()).any(|w| w == digits.as_bytes()));

    eprintln!(
        "real: create {create_ms} ms, login {login_ms} ms (argon2id 19 MiB t=2), db+wal {} bytes",
        raw.len()
    );
}
