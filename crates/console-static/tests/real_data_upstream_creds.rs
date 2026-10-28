// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for wave 4 slot 365 (data-upstream-strip-creds, F-351-1):
//! `--data-upstream` used to forward the browser's `Cookie` (the paired
//! phone's `galion_admin` token) and `Authorization` to the data reader.
//! Now only an allowlist of headers reaches the reader and a reader's
//! `Set-Cookie` never reaches the browser.
//!
//! Starts the real release `galion-console-static` (live `--dist` shape,
//! self-upstream local API, `--ui rust`, a temp `--admin-store`) on the box,
//! ports 22350-22359, with a capturing fake reader that records every full
//! request head it receives. Paths checked:
//!
//! * loopback `GET`/`HEAD /api/v1/bank` with fixture cookie + bearer + key headers;
//! * remote console host (`console.galion.app`) after a real pairing
//!   (`POST /api/v1/admin`), i.e. the real paired-phone credential;
//! * the Rust `/bank` page render (console-ui forwards the browser cookie
//!   to its self-upstream, which then hits the data proxy);
//! * `--upstream` API proxy regression: the console API still gets the cookie.
//!
//! Negative control: `DATA_CREDS_BIN=/path/to/old/galion-console-static
//! DATA_CREDS_EXPECT_LEAK=1` runs the same probe against a pre-fix binary
//! and asserts that the reader DID see the credentials (F-351-1 reproduced).
//!
//! `GALION_REAL=1 cargo test --release -p console-static --test real_data_upstream_creds -- --nocapture --test-threads=1`
//! Fixture tokens only (no live secrets).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Fixture markers: any of these in a reader-side head is a leak.
const FIX_BEARER: &str = "fixBearer365xyz";
const FIX_APIKEY: &str = "fixApiKey365xyz";
const FIX_COOKIE: &str = "fixCookie365xyz";
const FIX_PROXY: &str = "fixProxyAuth365";
const PAIR_CODE: &str = "PAIR365";

type Log = Arc<Mutex<Vec<String>>>;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn expect_leak() -> bool {
    std::env::var("DATA_CREDS_EXPECT_LEAK").as_deref() == Ok("1")
}

fn bin() -> PathBuf {
    std::env::var("DATA_CREDS_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_galion-console-static")))
}

/// Raw request; returns (status, head, body).
fn send(addr: &str, raw: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, head.to_string(), body.to_string())
}

const BANK: &str = r#"{"purses":[{"name":"OpenRouter","usd":9.5,"measured":true,"note":"key limit left"}],"totalUsd":9.5,"weekTokensIn":1,"weekTokensOut":1,"weekCostUsd":1.4,"dailyUsd":0.2,"daysLeft":47.5,"riverIfMovedDailyUsd":0.1,"note":"slot365 capturing reader"}"#;

/// Capturing reader: records each full request head; answers 200 with a
/// `Set-Cookie` that tries to forge the admin cookie.
fn capturing_reader(addr: &'static str, log: Log) {
    let l = TcpListener::bind(addr).expect("bind reader");
    thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let req = String::from_utf8_lossy(&buf).to_string();
            let head = req.split("\r\n\r\n").next().unwrap_or("").to_string();
            log.lock().unwrap().push(head);
            let body = if req.starts_with("HEAD ") { "" } else { BANK };
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: galion_admin=forgedByReader; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                BANK.len()
            );
        }
    });
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-365-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn dist_dir(tag: &str) -> PathBuf {
    let d = tmp(&format!("dist-{tag}"));
    std::fs::create_dir_all(d.join("static/assets")).unwrap();
    std::fs::write(d.join("static/assets/app.css"), "body{}").unwrap();
    d
}

/// sha256 hex via the crate under test is not public in tests; shell out to
/// coreutils `sha256sum` (box tool, no Python).
fn sha_hex(v: &str) -> String {
    let mut c = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("sha256sum");
    c.stdin.take().unwrap().write_all(v.as_bytes()).unwrap();
    let out = c.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn start(bind: &str, args: &[&str]) -> Kill {
    let child = Command::new(bin())
        .args(["--bind", bind, "--quiet"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn console");
    let g = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(bind).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "console did not start"
        );
        thread::sleep(Duration::from_millis(50));
    }
    g
}

fn leaks(head: &str) -> Vec<&'static str> {
    let low = head.to_ascii_lowercase();
    let mut v = Vec::new();
    for (name, needle) in [
        ("bearer", FIX_BEARER),
        ("api-key", FIX_APIKEY),
        ("cookie-value", FIX_COOKIE),
        ("proxy-auth", FIX_PROXY),
    ] {
        if head.contains(needle) {
            v.push(name);
        }
    }
    if low.contains("\r\ncookie:") {
        v.push("Cookie header");
    }
    if low.contains("\r\nauthorization:") {
        v.push("Authorization header");
    }
    if low.contains("galion_admin=") {
        v.push("galion_admin");
    }
    v
}

fn check_reader_head(what: &str, head: &str) {
    let l = leaks(head);
    println!("  reader head for {what}: leaks {l:?}");
    if expect_leak() {
        return;
    }
    assert!(l.is_empty(), "{what}: reader saw {l:?}:\n{head}");
}

#[test]
fn real_data_upstream_strips_credentials() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const BIND: &str = "127.0.0.1:22350";
    const READER: &str = "127.0.0.1:22351";
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    capturing_reader(READER, Arc::clone(&log));
    let dist = dist_dir("main");
    let adm = tmp("admin");
    let store = adm.join("admin-device.json");
    std::fs::write(
        &store,
        format!("{{\"codeHash\":\"{}\"}}", sha_hex(PAIR_CODE)),
    )
    .unwrap();
    let dists = dist.to_str().unwrap().to_string();
    let stores = store.to_str().unwrap().to_string();
    println!("binary: {}", bin().display());
    let _g = start(
        BIND,
        &[
            "--ui",
            "rust",
            "--dist",
            &dists,
            "--admin-store",
            &stores,
            "--data-upstream",
            READER,
        ],
    );
    let mut leaked_total = 0usize;
    let mut take = |what: &str| -> String {
        let h = log
            .lock()
            .unwrap()
            .pop()
            .unwrap_or_else(|| panic!("{what}: reader saw nothing"));
        if !leaks(&h).is_empty() {
            leaked_total += 1;
        }
        check_reader_head(what, &h);
        h
    };

    // 1) Loopback GET with every credential shape a browser/tool could send.
    let creds = format!(
        "Cookie: galion_admin={FIX_COOKIE}; galion_theme=dark\r\nAuthorization: Bearer {FIX_BEARER}\r\nProxy-Authorization: Basic {FIX_PROXY}\r\nX-Api-Key: {FIX_APIKEY}\r\ncookie: second={FIX_COOKIE}\r\n"
    );
    let (c, h, b) = send(
        BIND,
        &format!("GET /api/v1/bank?fresh=1 HTTP/1.1\r\nHost: 127.0.0.1\r\n{creds}Accept: application/json\r\nUser-Agent: real365/1\r\nIf-None-Match: \"e365\"\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(c, 200, "{h}");
    assert_eq!(b, BANK);
    let rh = take("loopback GET");
    assert!(
        rh.starts_with("GET /api/v1/bank?fresh=1 HTTP/1.1\r\n"),
        "{rh}"
    );
    if !expect_leak() {
        for k in [
            "Host: 127.0.0.1\r\n",
            "Accept: application/json\r\n",
            "User-Agent: real365/1\r\n",
            "If-None-Match: \"e365\"\r\n",
            "X-Forwarded-For: 127.0.0.1\r\n",
            "X-Forwarded-Proto: http\r\n",
        ] {
            assert!(rh.contains(k), "allowlisted {k:?} missing:\n{rh}");
        }
        assert!(
            !h.to_ascii_lowercase().contains("set-cookie"),
            "reader Set-Cookie reached client:\n{h}"
        );
    }
    println!(
        "loopback GET /api/v1/bank -> {c}, client Set-Cookie: {}",
        h.to_ascii_lowercase().contains("set-cookie")
    );

    // 2) HEAD.
    let (c, h, _) = send(
        BIND,
        &format!(
            "HEAD /api/v1/bank HTTP/1.1\r\nHost: 127.0.0.1\r\n{creds}Connection: close\r\n\r\n"
        ),
    );
    assert_eq!(c, 200, "{h}");
    take("loopback HEAD");

    // 3) Real pairing on the remote console host -> real admin cookie.
    let body = format!("{{\"code\":\"{}\"}}", PAIR_CODE.to_ascii_lowercase());
    let (c, h, b) = send(
        BIND,
        &format!(
            "POST /api/v1/admin HTTP/1.1\r\nHost: console.galion.app\r\nOrigin: https://console.galion.app\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert_eq!(c, 200, "pairing: {h}\n{b}");
    let set = h
        .lines()
        .find(|l| {
            l.to_ascii_lowercase()
                .starts_with("set-cookie: galion_admin=")
        })
        .expect("admin cookie");
    let pair = set["set-cookie: ".len()..]
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let token = pair.trim_start_matches("galion_admin=").to_string();
    assert!(token.len() >= 20, "token shape");
    println!(
        "paired: admin cookie len {} (value not printed)",
        token.len()
    );
    // Unpaired remote host is still refused before the reader.
    let before = log.lock().unwrap().len();
    let (c, _, _) = send(
        BIND,
        "GET /api/v1/bank HTTP/1.1\r\nHost: console.galion.app\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(c, 401);
    assert_eq!(log.lock().unwrap().len(), before, "unpaired reached reader");
    // Paired phone GET: passes the gate, reaches the reader without the token.
    let (c, h, _) = send(
        BIND,
        &format!("GET /api/v1/bank HTTP/1.1\r\nHost: console.galion.app\r\nCookie: {pair}; galion_theme=dark\r\nAuthorization: Bearer {FIX_BEARER}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(c, 200, "paired GET: {h}");
    let rh = take("paired phone GET");
    if !expect_leak() {
        assert!(!rh.contains(&token), "admin token reached reader");
        assert!(rh.contains("Host: console.galion.app\r\n"));
    } else {
        assert!(rh.contains(&token), "control: old binary should leak token");
    }

    // 4) Rust /bank page render with the paired cookie: console-ui forwards
    //    the browser cookie to its self-upstream, which hits the data proxy.
    let before = log.lock().unwrap().len();
    let (c, h, page) = send(
        BIND,
        &format!("GET /bank HTTP/1.1\r\nHost: console.galion.app\r\nCookie: {pair}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(c, 200, "/bank: {h}");
    assert!(
        page.contains("OpenRouter") && page.contains("$9.50"),
        "bank rendered"
    );
    let seen: Vec<String> = log.lock().unwrap()[before..].to_vec();
    assert!(!seen.is_empty(), "page render did not reach reader");
    for (i, rh) in seen.iter().enumerate() {
        if !leaks(rh).is_empty() {
            leaked_total += 1;
        }
        check_reader_head(&format!("/bank render #{i}"), rh);
        if !expect_leak() {
            assert!(!rh.contains(&token));
        }
    }
    if !expect_leak() {
        assert!(!h.to_ascii_lowercase().contains("forgedbyreader"));
    }
    println!(
        "/bank render -> {c} {} bytes, reader requests {}",
        page.len(),
        seen.len()
    );

    // 5) Load: 100 credentialed GETs, none leaks.
    let t0 = Instant::now();
    let before = log.lock().unwrap().len();
    for i in 0..100 {
        let ck = if i % 2 == 0 {
            format!("Cookie: {pair}\r\nHost: console.galion.app\r\n")
        } else {
            format!("Host: 127.0.0.1\r\n{creds}")
        };
        let (c, _, _) = send(
            BIND,
            &format!("GET /api/v1/bank HTTP/1.1\r\n{ck}Connection: close\r\n\r\n"),
        );
        assert_eq!(c, 200);
    }
    let ms = t0.elapsed().as_millis();
    let seen: Vec<String> = log.lock().unwrap()[before..].to_vec();
    assert_eq!(seen.len(), 100);
    let n_leak = seen
        .iter()
        .filter(|h| !leaks(h).is_empty() || h.contains(&token))
        .count();
    leaked_total += n_leak;
    println!("100 credentialed GETs in {ms} ms, reader heads with credentials: {n_leak}");
    if expect_leak() {
        assert!(
            leaked_total > 0,
            "control binary did not leak: probe is blind"
        );
        println!("NEGATIVE CONTROL: old binary leaked in {leaked_total} reader heads (F-351-1 reproduced)");
    } else {
        assert_eq!(n_leak, 0);
        assert_eq!(leaked_total, 0);
    }
    let _ = std::fs::remove_dir_all(&dist);
    let _ = std::fs::remove_dir_all(&adm);
}

/// The `--upstream` API proxy keeps forwarding the session cookie (the
/// console API needs it); only the data reader is stripped.
#[test]
fn real_api_upstream_still_gets_cookie() {
    if !real() || expect_leak() {
        eprintln!("skipped: set GALION_REAL=1 (and not the leak control)");
        return;
    }
    const BIND: &str = "127.0.0.1:22352";
    const API: &str = "127.0.0.1:22353";
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    capturing_reader(API, Arc::clone(&log));
    let root = tmp("root");
    std::fs::write(root.join("_shell.html"), "<p>x</p>").unwrap();
    let rs = root.to_str().unwrap().to_string();
    let _g = start(BIND, &["--root", &rs, "--upstream", API]);
    let (c, h, _) = send(
        BIND,
        &format!("GET /api/v1/session HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: galion_admin={FIX_COOKIE}\r\nAuthorization: Bearer {FIX_BEARER}\r\nConnection: close\r\n\r\n"),
    );
    assert_eq!(c, 200, "{h}");
    let rh = log.lock().unwrap().pop().expect("api saw request");
    assert!(rh.contains(&format!("Cookie: galion_admin={FIX_COOKIE}\r\n")));
    assert!(rh.contains(&format!("Authorization: Bearer {FIX_BEARER}\r\n")));
    assert!(
        h.contains("Set-Cookie: galion_admin=forgedByReader"),
        "API Set-Cookie must still pass (pairing/session)"
    );
    println!(
        "--upstream API proxy: Cookie + Authorization forwarded, Set-Cookie passed (unchanged)"
    );
    let _ = std::fs::remove_dir_all(&root);
}
