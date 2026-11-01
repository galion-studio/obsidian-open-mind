// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Real-life test (2026-11-01 CET): the real galion-one binary forwards the
// galion.app account routes to a real galion-accounts binary, and shows the
// placeholder / 503s while accounts is down. Ports 19975..19979 (accounts
// slot; movable with ONE_ACCOUNTS_REAL_PORT_BASE). Skipped unless
// GALION_REAL=1. The "accounts up" half needs GALION_ACCOUNTS_BIN (a built
// galion-accounts); without it only the "down" half runs. No Stripe keys:
// card checkout must stay "not connected".
//
//   GALION_REAL=1 GALION_ACCOUNTS_BIN=/path/to/galion-accounts \
//     cargo test -p galion-one --test real_one_accounts_proxy -- --nocapture

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::ports::Block;

/// one +0, mouth +1, accounts public +2, accounts internal +3, dead +4.
static PORTS: Block = Block::new("ONE_ACCOUNTS_REAL_PORT_BASE", 19975, 5);

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Resp {
    code: u16,
    head: String,
    raw_head: String,
    body: String,
}

fn send(addr: &str, raw: &[u8]) -> Option<Resp> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(15))).ok()?;
    s.write_all(raw).ok()?;
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let code = text.split_whitespace().nth(1)?.parse().ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    Some(Resp {
        code,
        head: head.to_ascii_lowercase(),
        raw_head: head.to_string(),
        body: body.to_string(),
    })
}

fn req(method: &str, host: &str, path: &str, extra: &str, body: &str) -> Vec<u8> {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn one(method: &str, path: &str, extra: &str, body: &str) -> Resp {
    send(PORTS.addr(0), &req(method, "galion.app", path, extra, body))
        .unwrap_or_else(|| panic!("no answer for {method} {path}"))
}

fn wait_up(addr: &str, raw: &[u8]) {
    let start = Instant::now();
    while send(addr, raw).is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "{addr} never came up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The checks that must hold while galion-accounts is not answering.
fn assert_down(stage: &str) {
    let r = one("GET", "/account", "", "");
    assert_eq!(r.code, 200, "{stage}");
    assert!(
        r.body.contains("Account numbers are coming soon"),
        "{stage}: placeholder"
    );
    let r = one("HEAD", "/api/checkout?plan=plus", "", "");
    assert_eq!(r.code, 503, "{stage}: price buttons stay Coming soon");
    let r = one(
        "POST",
        "/api/account/create",
        "Content-Type: application/json\r\n",
        "{}",
    );
    assert_eq!(r.code, 503, "{stage}");
    assert!(r.body.contains("\"unavailable\""), "{stage}: {}", r.body);
    let r = one("POST", "/webhooks/stripe", "", "{}");
    assert_eq!(r.code, 503, "{stage}: Stripe retries later");
    let r = one("GET", "/account/account.css", "", "");
    assert_eq!(r.code, 503, "{stage}");
    eprintln!("{stage}: placeholder + 503s ok");
}

#[test]
fn real_accounts_proxy() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }
    let bin = std::env::var("GALION_ONE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-one").to_string());
    let dir = Dir(std::env::temp_dir().join(format!("one-acct-real-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&dir.0);
    std::fs::create_dir_all(dir.0.join("home/one")).unwrap();
    std::fs::write(dir.0.join("home/one/index.html"), "<html>fx</html>").unwrap();
    std::fs::write(dir.0.join("hermes.env"), "").unwrap();
    let upstream = PORTS.addr(2);
    let _one = Kill(
        Command::new(&bin)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", dir.0.join("home"))
            .env("GALION_HOME", dir.0.join("home"))
            .env("ONE_NO_WARM", "1")
            .env("ONE_BIND", PORTS.addr(0))
            .env("ONE_MOUTH_BIND", PORTS.addr(1))
            .env("ONE_HERMES_DIR", dir.0.join("hermes"))
            .env("ONE_HERMES_ENV", dir.0.join("hermes.env"))
            .env("ONE_DEVICE_STORE", dir.0.join("missing-store.json"))
            .env("ONE_ACCOUNTS_UPSTREAM", upstream)
            .env("GALION_MIND", PORTS.addr(4))
            .env("GALION_EAR", PORTS.addr(4))
            .env("GALION_EYE", PORTS.addr(4))
            .env("GALION_MEMORY", PORTS.addr(4))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start galion-one"),
    );
    wait_up(PORTS.addr(0), &req("GET", "x", "/health", "", ""));

    // 1. Accounts not running.
    assert_down("accounts down");
    // Other hosts never reach accounts.
    let r = send(
        PORTS.addr(0),
        &req("HEAD", "one.galion.app", "/api/checkout?plan=plus", "", ""),
    )
    .unwrap();
    assert_ne!(r.code, 503, "one.galion.app is not proxied");
    // Chunked bodies are refused before any forwarding.
    let r = send(
        PORTS.addr(0),
        b"POST /webhooks/stripe HTTP/1.1\r\nHost: galion.app\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n",
    )
    .unwrap();
    assert_eq!(r.code, 411);

    let Ok(accounts) = std::env::var("GALION_ACCOUNTS_BIN") else {
        eprintln!("GALION_ACCOUNTS_BIN unset: skipped the accounts-up half");
        return;
    };
    // 2. Real galion-accounts, no Stripe, test-cost argon2.
    let pepper = dir.0.join("pepper");
    let st = Command::new(&accounts)
        .args(["new-pepper", "--out"])
        .arg(&pepper)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success());
    let mut acct = Kill(
        Command::new(&accounts)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("ACCOUNTS_TEST_COST", "1")
            .env("ACCOUNTS_TRUST_PROXY", "1")
            .args(["serve", "--listen", upstream, "--internal", PORTS.addr(3)])
            .arg("--db")
            .arg(dir.0.join("accounts.sqlite"))
            .arg("--pepper-file")
            .arg(&pepper)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start galion-accounts"),
    );
    wait_up(upstream, &req("GET", "x", "/api/account/plans", "", ""));

    let r = one("GET", "/account", "", "");
    assert_eq!(r.code, 200);
    assert!(
        !r.body.contains("coming soon"),
        "real page, not the placeholder"
    );
    assert!(r.body.contains("/account/account.js"));
    assert!(r.head.contains("content-security-policy"));
    let r = one("GET", "/account/account.css", "", "");
    assert_eq!(r.code, 200);
    assert!(r.head.contains("content-type: text/css"));
    let r = one("GET", "/account/account.js", "", "");
    assert_eq!(r.code, 200);

    // Card checkout: still not connected (no Stripe keys anywhere).
    let r = one("HEAD", "/api/checkout?plan=plus", "", "");
    assert_eq!(r.code, 503, "HEAD /api/checkout: {}", r.head);
    let r = one("GET", "/api/checkout?plan=pro", "", "");
    assert_eq!(r.code, 303);
    assert!(r.head.contains("location: /account?plan=pro"), "{}", r.head);
    let r = one(
        "POST",
        "/webhooks/stripe",
        "Content-Type: application/json\r\n",
        "{}",
    );
    assert_eq!(r.code, 503);
    assert!(
        r.body.contains("not_connected"),
        "accounts' own answer: {}",
        r.body
    );

    // CSRF guard sees the real Host: a foreign Origin is refused.
    let r = one(
        "POST",
        "/api/account/create",
        "Content-Type: application/json\r\nOrigin: https://evil.example\r\n",
        "{}",
    );
    assert!(r.code == 403 || r.code == 415, "foreign origin: {}", r.code);

    // Create a number through the proxy, then read it back with the cookie.
    let r = one(
        "POST",
        "/api/account/create",
        "Content-Type: application/json\r\nOrigin: https://galion.app\r\nCF-Connecting-IP: 203.0.113.7\r\nCookie: galion_one=pairing-secret\r\n",
        "{}",
    );
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains("\"number\""));
    let cookie = r
        .raw_head
        .lines()
        .find_map(|l| {
            l.split_once(": ")
                .filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie"))
                .map(|(_, v)| v)
        })
        .and_then(|c| c.split(';').next())
        .expect("session cookie")
        .to_string();
    assert!(cookie.starts_with("galion_acct="), "{cookie}");
    let r = one(
        "GET",
        "/api/account/me",
        &format!("Cookie: other=1; {cookie}\r\n"),
        "",
    );
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains("\"plan\":\"free\""), "{}", r.body);
    let r = one("GET", "/api/account/me", "", "");
    assert_eq!(r.code, 401, "no cookie, no account: {}", r.body);
    eprintln!("accounts up: page, css, js, checkout 503/303, webhook, create + me ok");

    // 3. Accounts stops: back to the placeholder without restarting galion-one.
    let _ = acct.0.kill();
    let _ = acct.0.wait();
    assert_down("accounts stopped");
}
