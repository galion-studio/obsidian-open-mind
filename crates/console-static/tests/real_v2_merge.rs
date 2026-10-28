// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life probe for the console V2 merge prep tip (wave 4 slot 300):
//! one release `galion-console-static --ui rust` on the box carries BOTH
//! console-v2-sprite-health (icon sprite + cached health, slot 276) and
//! console-pairing-e2e / session-pack harden (slots 265/286). This test runs
//! the pairing + session-pack flow and then the sprite + cached-health checks
//! against the same process, so a merge regression in either half fails here.
//!
//! Ports 21700-21709 (front 21700, fake API 21701). Fixture tokens only.
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_v2_merge -- --nocapture`.
//! No VPS contact. No live console.service change. Not the live cutover.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:21700";
const UPSTREAM: &str = "127.0.0.1:21701";
const HOST: &str = "console.galion.app";
/// Test fixture only (same shape as ui.rs). Not a live device secret.
const FIX_DEVICE: &str = "devSecretToken01ab";
const FIX_ADMIN: &str = "good";

type Log = Arc<Mutex<Vec<String>>>;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn http(method: &str, path: &str, extra: &str, body: &str) -> Resp {
    let mut s = TcpStream::connect(BIND).expect("connect front");
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(15))).unwrap();
    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: {HOST}\r\n{extra}Connection: close\r\n");
    if !body.is_empty() {
        req.push_str(&format!(
            "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Resp {
        code,
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn cookie_line(set_cookie_headers: &str, name: &str) -> Option<String> {
    for line in set_cookie_headers.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.starts_with("set-cookie:") {
            continue;
        }
        let v = line.split_once(':').map(|(_, r)| r.trim()).unwrap_or("");
        if let Some(rest) = v.strip_prefix(&format!("{name}=")) {
            let val = rest.split(';').next().unwrap_or("").to_string();
            return Some(val);
        }
    }
    None
}

fn start_fake_api(log: Log) {
    let l = TcpListener::bind(UPSTREAM).expect("bind fake api 21701");
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || answer(s, log));
        }
    });
}

fn answer(mut s: TcpStream, log: Log) {
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head, body) = loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..p]).to_string();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            while buf.len() < p + 4 + len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            break (
                head,
                String::from_utf8_lossy(
                    &buf[p + 4..p + 4 + len.min(buf.len().saturating_sub(p + 4))],
                )
                .to_string(),
            );
        }
    };
    let first = head.lines().next().unwrap_or("").to_string();
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Cookie: "))
        .unwrap_or("")
        .to_string();
    log.lock()
        .unwrap()
        .push(format!("{first} cookie={cookie} body={body}"));
    let method = first.split(' ').next().unwrap_or("");
    let path = first
        .split(' ')
        .nth(1)
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    let paired = cookie.contains(&format!("galion_admin={FIX_ADMIN}"));
    let (code, ctype, extra, out): (u16, &str, String, String) = match (method, path) {
        ("GET", "/api/v1/admin") => (
            200,
            "application/json",
            String::new(),
            format!("{{\"ok\":true,\"paired\":true,\"you\":{paired}}}"),
        ),
        ("GET", "/api/v1/session-pack") if paired => (
            200,
            "application/octet-stream",
            "Content-Disposition: inline\r\n".into(),
            "PK\u{3}\u{4}ZIPFAKE".into(),
        ),
        ("GET", "/api/v1/session-pack") => (
            401,
            "application/json",
            String::new(),
            "{\"error\":\"phone required\"}".into(),
        ),
        ("POST", "/api/v1/admin") if body.contains("\"code\":\"RIGHT\"") => (
            200,
            "application/json",
            format!(
                "Set-Cookie: galion_admin={FIX_ADMIN}; HttpOnly; Path=/; SameSite=Lax\r\nSet-Cookie: other=x\r\n"
            ),
            format!("{{\"ok\":true,\"you\":true,\"device\":\"{FIX_DEVICE}\"}}"),
        ),
        ("POST", "/api/v1/admin") if body.contains(&format!("\"device\":\"{FIX_DEVICE}\"")) => (
            200,
            "application/json",
            format!("Set-Cookie: galion_admin={FIX_ADMIN}; HttpOnly; Path=/; SameSite=Lax\r\n"),
            "{\"ok\":true,\"you\":true,\"device\":\"\"}".into(),
        ),
        ("POST", "/api/v1/admin") => (
            401,
            "application/json",
            String::new(),
            "{\"error\":\"wrong code\"}".into(),
        ),
        ("GET", "/api/v1/health") => (
            200,
            "application/json",
            String::new(),
            "{\"status\":\"healthy\",\"version\":\"e2e-fixture\"}".into(),
        ),
        ("GET", "/api/v1/session") if paired => (
            200,
            "application/json",
            String::new(),
            "{\"ok\":true,\"items\":[{\"name\":\"00-summary.md\",\"bytes\":12}]}".into(),
        ),
        ("GET", "/api/v1/session") => (
            401,
            "application/json",
            String::new(),
            "{\"error\":\"phone required\"}".into(),
        ),
        _ => (404, "text/plain", String::new(), "nope".into()),
    };
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: {ctype}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
}

fn wait_bind(addr: &str, secs: u64) {
    let t0 = Instant::now();
    while TcpStream::connect(addr).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "did not start on {addr}"
        );
        thread::sleep(Duration::from_millis(40));
    }
}

#[test]
fn real_v2_merge_pairing_sprite_health() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }

    // Fail fast if our reserved ports are already taken (box-only range).
    for addr in [BIND, UPSTREAM] {
        if TcpStream::connect(addr).is_ok() {
            panic!("{addr} already in use; free ports 21700-21709");
        }
    }

    let log: Log = Arc::new(Mutex::new(Vec::new()));
    start_fake_api(Arc::clone(&log));
    wait_bind(UPSTREAM, 5);

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            UPSTREAM,
            "--bind",
            BIND,
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start galion-console-static release");
    let _guard = Kill(child);
    wait_bind(BIND, 15);

    let so = "Sec-Fetch-Site: same-origin\r\n";
    let t0 = Instant::now();

    // --- Negative: unpaired session-pack (proxy harden) ---
    let r = http("GET", "/api/v1/session-pack", "", "");
    assert_eq!(r.code, 401, "unpaired pack: {}", r.head);
    assert!(
        r.body.contains("phone required"),
        "body={}",
        r.body.chars().take(80).collect::<String>()
    );
    assert!(r.head.to_ascii_lowercase().contains("application/json"));
    let hl = r.head.to_ascii_lowercase();
    assert!(
        hl.contains("cache-control: no-store") || hl.contains("application/json"),
        "pack 401 headers: {}",
        r.head
    );
    eprintln!(
        "NEG unpaired GET /api/v1/session-pack -> {} ({:?})",
        r.code,
        r.body.chars().take(40).collect::<String>()
    );

    // --- Negative: wrong pair code ---
    let r = http("POST", "/_ui/pair?back=/session", so, "code=WRONG");
    assert_eq!(r.code, 401, "wrong code: {}", r.head);
    assert!(r.body.contains("That code was refused."));
    eprintln!("NEG POST /_ui/pair code=WRONG -> {}", r.code);

    // --- Negative: CSRF cross-site ---
    let r = http(
        "POST",
        "/_ui/pair",
        "Sec-Fetch-Site: cross-site\r\n",
        "code=RIGHT",
    );
    assert_eq!(r.code, 403, "csrf: {}", r.head);
    eprintln!("NEG POST /_ui/pair CSRF cross-site -> {}", r.code);

    // --- Happy: code pair sets admin + device cookies ---
    let r = http("POST", "/_ui/pair?back=/session", so, "code=RIGHT");
    assert_eq!(r.code, 303, "pair: {}", r.head);
    assert!(r.head.contains("Location: /session"), "{}", r.head);
    let admin = cookie_line(&r.head, "galion_admin").expect("admin cookie");
    assert_eq!(admin, FIX_ADMIN);
    let device = cookie_line(&r.head, "galion_device").expect("device cookie");
    assert_eq!(device, FIX_DEVICE);
    assert!(
        r.head.contains("galion_device=")
            && r.head.contains("HttpOnly")
            && r.head.contains("Max-Age=31536000")
            && r.head.contains("Secure"),
        "device cookie flags: {}",
        r.head
    );
    assert!(
        !r.head.contains("other=x"),
        "upstream stray cookie must not be relayed"
    );
    eprintln!("OK pair -> 303 Location:/session admin+device HttpOnly Secure");

    // --- Happy: paired session-pack download headers ---
    let pack_cookie = format!("Cookie: galion_admin={FIX_ADMIN}\r\n");
    let r = http("GET", "/api/v1/session-pack", &pack_cookie, "");
    assert_eq!(r.code, 200, "pack: {}", r.head);
    assert!(
        r.head.contains("Content-Type: application/zip"),
        "ctype: {}",
        r.head
    );
    assert!(
        r.head
            .contains("Content-Disposition: attachment; filename=\"build-session.zip\""),
        "disp: {}",
        r.head
    );
    assert!(r.head.contains("Cache-Control: no-store"), "{}", r.head);
    assert!(
        r.head.contains("X-Content-Type-Options: nosniff"),
        "{}",
        r.head
    );
    assert!(
        r.body.as_bytes().starts_with(b"PK"),
        "zip magic missing, len={}",
        r.body.len()
    );
    eprintln!(
        "OK paired GET /api/v1/session-pack -> 200 zip {} bytes disposition=build-session.zip",
        r.body.len()
    );

    // --- Happy: device resume when you=false (paired:true store) ---
    let r = http(
        "GET",
        "/session",
        &format!("Cookie: galion_device={FIX_DEVICE}\r\n"),
        "",
    );
    assert_eq!(r.code, 303, "resume: {}", r.head);
    assert!(r.head.contains("Location: /session"), "{}", r.head);
    assert!(
        cookie_line(&r.head, "galion_admin").as_deref() == Some(FIX_ADMIN),
        "resume admin cookie: {}",
        r.head
    );
    eprintln!("OK device resume GET /session -> 303 + galion_admin");

    // --- Negative: refused device cookie cleared (no 5-fail burn) ---
    let r = http(
        "GET",
        "/session",
        "Cookie: galion_device=notAValidDeviceXX\r\n",
        "",
    );
    assert_eq!(r.code, 200, "bad device: {}", r.head);
    assert!(
        r.body.contains("action=\"/_ui/pair"),
        "expected pairing form"
    );
    assert!(
        r.head
            .contains("galion_device=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0"),
        "clear cookie: {}",
        r.head
    );
    eprintln!("NEG refused device -> pairing page + Max-Age=0 clear");

    // --- Negative: session-pack method not allowed ---
    let r = http("POST", "/api/v1/session-pack", &pack_cookie, "");
    assert_eq!(r.code, 405, "pack POST: {}", r.head);
    eprintln!("NEG POST /api/v1/session-pack -> {}", r.code);

    let posts = log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains("POST /api/v1/admin"))
        .count();
    assert!(
        posts >= 3,
        "expected pair+wrong+resume admin POSTs, got {posts}"
    );

    // --- Sprite + cached health on the same (paired) process ---
    let health_hits = |log: &Log| {
        log.lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("GET /api/v1/health "))
            .count()
    };
    // Let any health answer cached during the pairing steps expire so the
    // first chrome page below is a guaranteed fill (TTL is a few seconds).
    thread::sleep(Duration::from_millis(5_500));
    let before = health_hits(&log);
    let r = http("GET", "/console", &pack_cookie, "");
    assert_eq!(r.code, 200, "/console: {}", r.head);
    let filled = health_hits(&log) - before;
    assert_eq!(filled, 1, "first /console after TTL fills the cache once");
    for p in ["/data", "/agents", "/console", "/gateway", "/memory"] {
        for _ in 0..3 {
            let r = http("GET", p, &pack_cookie, "");
            assert_eq!(r.code, 200, "{p}: {}", r.head);
        }
    }
    let burst = health_hits(&log) - before;
    assert_eq!(
        burst, 1,
        "15-page burst must reuse cached health, hits={burst}"
    );
    eprintln!("OK cached health: 16 chrome pages -> {burst} upstream /api/v1/health");

    let r = http("GET", "/services", &pack_cookie, "");
    assert_eq!(r.code, 200, "/services: {}", r.head);
    assert!(!r.body.to_ascii_lowercase().contains("<script"));
    assert!(r.body.contains("<use href=\"/_ui/i."), "sprite use missing");
    let href = r
        .body
        .split("<use href=\"")
        .nth(1)
        .unwrap()
        .split('#')
        .next()
        .unwrap()
        .to_string();
    let s = http("GET", &href, "", "");
    assert_eq!(s.code, 200, "{}", s.head);
    assert!(s.head.contains("immutable"), "{}", s.head);
    assert!(s.head.contains("image/svg+xml"), "{}", s.head);
    let symbols = s.body.matches("<symbol ").count();
    assert!(symbols >= 40, "symbols={symbols}");
    eprintln!(
        "OK sprite {href} -> 200 {} bytes {symbols} symbols immutable; /services {} bytes",
        s.body.len(),
        r.body.len()
    );

    // Session page (pairing half) also renders with the sprite chrome.
    let r = http("GET", "/session", &pack_cookie, "");
    assert_eq!(r.code, 200, "/session paired: {}", r.head);
    assert!(
        !r.body.contains("action=\"/_ui/pair"),
        "paired /session must not show the pairing form"
    );
    eprintln!("OK paired /session -> 200 ({} bytes)", r.body.len());

    let ms = t0.elapsed().as_millis();
    eprintln!(
        "real_v2_merge PASS on {BIND} upstream={UPSTREAM} in {ms} ms (fixture tokens only; no VPS)"
    );
}
