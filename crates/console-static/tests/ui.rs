// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end tests for `--ui rust`: the real server with a fake console API
//! upstream and a fake desk pulse, raw HTTP/1.1 over loopback (ephemeral
//! ports). Covers the gate (loopback, cookie, Soon, pairing), the three
//! ported pages, the no-JS form posts and CSRF refusals.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const SERVICES: &str = r#"{"count":4,"items":[
{"id":"console","name":"Control console","kind":"console","weight":"light","summary":"This operator desk.","repo":"galion-initiative-web","hosts":["dashboard.galion.studio"],"port":8080,"defaultOn":true},
{"id":"hermes","name":"Hermes","kind":"api","weight":"light","summary":"The mind.","hosts":[],"port":8642,"defaultOn":false},
{"id":"whisper","name":"Whisper","kind":"api","weight":"light","summary":"Speech to text.","hosts":[],"port":9000,"defaultOn":true},
{"id":"gen","name":"Generation","kind":"held","weight":"heavy","summary":"GPU <b>only</b>.","hosts":[],"port":9100,"defaultOn":false}]}"#;
const DOMAINS: &str = r#"{"count":2,"items":[
{"host":"console.galion.app","role":"Console IDE — operator control","serviceId":"console"},
{"host":"voice.galion.app","role":"Voice <script>","serviceId":"whisper"}]}"#;
const HEALTH: &str = r#"{"status":"healthy","version":"3.8.3-board","target":{"pod":"ip50"}}"#;
const AWAKE: &str = r#"{"services":[{"id":"console","ok":true},{"id":"whisper","ok":false},{"id":"vision","ok":false}]}"#;

type Log = Arc<Mutex<Vec<String>>>;

/// Fake console API. Records "METHOD path host cookie body" per request.
fn fake_api(log: Log) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || answer(s, log));
        }
    });
    addr
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
            break (head, String::from_utf8_lossy(&buf[p + 4..]).to_string());
        }
    };
    let first = head.lines().next().unwrap_or("").to_string();
    let hdr = |n: &str| {
        head.lines()
            .find_map(|l| l.strip_prefix(n))
            .unwrap_or("-")
            .to_string()
    };
    let host = hdr("Host: ");
    let cookie = hdr("Cookie: ");
    log.lock()
        .unwrap()
        .push(format!("{first} {host} {cookie} {body}"));
    let path = first.split(' ').nth(1).unwrap_or("");
    let paired = cookie.contains("galion_admin=good");
    let (code, ctype, extra, out): (u16, &str, String, String) = match (first.split(' ').next().unwrap_or(""), path) {
        ("GET", "/api/v1/admin") => (200, "application/json", String::new(), format!("{{\"ok\":true,\"paired\":true,\"you\":{paired}}}")),
        ("POST", "/api/v1/admin") if body.contains("\"code\":\"RIGHT\"") => (
            200,
            "application/json",
            "Set-Cookie: galion_admin=good; HttpOnly; Path=/; SameSite=Lax\r\nSet-Cookie: other=x\r\n".into(),
            "{\"ok\":true,\"you\":true,\"device\":\"\"}".into(),
        ),
        ("POST", "/api/v1/admin") => (401, "application/json", String::new(), "{\"error\":\"wrong code\"}".into()),
        ("GET", "/api/v1/health") => (200, "application/json", String::new(), HEALTH.into()),
        // Chunked on purpose: the UI client must decode it.
        ("GET", "/api/v1/services") => {
            let mut c = String::new();
            for part in SERVICES.as_bytes().chunks(100) {
                c.push_str(&format!("{:x}\r\n{}\r\n", part.len(), String::from_utf8_lossy(part)));
            }
            c.push_str("0\r\n\r\n");
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{c}");
            let _ = s.write_all(resp.as_bytes());
            return;
        }
        ("GET", "/api/v1/domains") => (200, "application/json", String::new(), DOMAINS.into()),
        ("GET", "/api/v1/awake") => (200, "application/json", String::new(), AWAKE.into()),
        ("POST", "/api/v1/command") => (200, "application/json", String::new(), "{\"ok\":true,\"data\":{\"cmd\":\"health\",\"note\":\"undefined fine\"}}".into()),
        _ => (404, "text/plain", String::new(), "nope".into()),
    };
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: {ctype}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
}

fn start(extra: &[&str]) -> (SocketAddr, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let api = fake_api(Arc::clone(&log));
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--upstream".into(),
        api.to_string(),
        "--pulse".into(),
        api.to_string(),
        "--quiet".into(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let cfg = Config::from_args(args).unwrap();
    let routes = Routes::parse("page /\n").unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(Site::default(), routes, cfg));
    thread::spawn(move || server::serve(l, state));
    (addr, log)
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn send(addr: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Resp {
        code: head
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn get(addr: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn post(addr: SocketAddr, path: &str, host: &str, extra: &str, body: &str) -> Resp {
    send(
        addr,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn loopback_pages_render_without_javascript() {
    let (a, log) = start(&[]);
    for p in ["/status", "/services", "/agents"] {
        let r = get(a, p, "127.0.0.1", "");
        assert_eq!(r.code, 200, "{p}: {}", r.head);
        assert!(
            r.head.contains("Content-Type: text/html; charset=utf-8"),
            "{p}"
        );
        assert!(
            r.head
                .contains("Content-Security-Policy: default-src 'none'"),
            "{p}"
        );
        assert!(!r.body.contains("<script"), "{p} has a script");
        assert!(r.body.contains("/_ui/console."), "{p} links the sheet");
    }
    // The gate did not need the upstream for a loopback peer + loopback Host.
    assert!(!log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.starts_with("GET /api/v1/admin")));
}

#[test]
fn status_page_content() {
    let (a, _) = start(&[]);
    let r = get(a, "/status", "127.0.0.1", "");
    assert!(r.body.contains("<h1>Light stack is up.</h1>"), "{}", r.body);
    // console online (catalog + pulse ok); whisper probed down -> offline; gen held.
    assert!(
        r.body.contains("1 subsystems online · 1 held"),
        "{}",
        r.body
    );
    assert!(r.body.contains("console.galion.app"));
    assert!(r.body.contains("Voice &lt;script&gt;"), "escaped");
    assert!(r
        .body
        .contains("<span class=\"dot offline\"></span>offline"));
}

#[test]
fn services_page_groups_like_the_tsx() {
    let (a, _) = start(&[]);
    let r = get(a, "/services", "127.0.0.1", "");
    let b = &r.body;
    let up = b.find("<h2>Up</h2>").unwrap();
    let down = b.find("<h2>Down</h2>").unwrap();
    let console = b.find("Control console").unwrap();
    let hermes = b.find(">Hermes<").unwrap();
    let whisper = b.find(">Whisper<").unwrap();
    let vision = b.find(">Vision<").unwrap();
    assert!(
        up < console && console < hermes && hermes < down,
        "online first, then offline, in Up"
    );
    assert!(
        whisper > down && vision > down,
        "cold/probed-down go to Down"
    );
    assert!(b.contains("GPU &lt;b&gt;only&lt;/b&gt;."));
    assert!(b.contains(":8080 · dashboard.galion.studio · galion-initiative-web"));
    assert!(b.contains("Desk pulse: 3 services reported."));
    assert!(!b.contains(">Stop<") && !b.contains(">Start<"));
    assert!(b.contains("aria-current=\"page\""));
}

#[test]
fn agents_page_and_command_post() {
    let (a, log) = start(&[]);
    let r = get(a, "/agents", "127.0.0.1", "");
    assert!(r.body.contains("3.8.3-board"));
    assert!(r.body.contains(">ip50<"), "pod from target.pod");
    assert!(r.body.contains("not in /api/v1/health"));
    assert!(r.body.contains(">idle</pre>"));
    // Same-origin form post runs the command upstream as JSON.
    let r = post(
        a,
        "/agents",
        "127.0.0.1",
        "Sec-Fetch-Site: same-origin\r\n",
        "cmd=health",
    );
    assert_eq!(r.code, 200, "{}", r.head);
    assert!(
        r.body
            .contains("{&quot;cmd&quot;:&quot;health&quot;,&quot;note&quot;:&quot; fine&quot;}"),
        "{}",
        r.body
    );
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.starts_with("POST /api/v1/command") && l.ends_with("{\"cmd\":\"health\"}")));
    // Unknown command never leaves the box.
    let r = post(
        a,
        "/agents",
        "127.0.0.1",
        "Sec-Fetch-Site: same-origin\r\n",
        "cmd=rm",
    );
    assert_eq!(r.code, 400);
    // Cross-site and header-less posts are refused.
    assert_eq!(
        post(
            a,
            "/agents",
            "127.0.0.1",
            "Sec-Fetch-Site: cross-site\r\n",
            "cmd=health"
        )
        .code,
        403
    );
    assert_eq!(post(a, "/agents", "127.0.0.1", "", "cmd=health").code, 403);
    assert_eq!(
        post(
            a,
            "/agents",
            "127.0.0.1",
            "Origin: http://evil.example\r\n",
            "cmd=health"
        )
        .code,
        403
    );
    assert_eq!(
        post(
            a,
            "/agents",
            "127.0.0.1:80",
            "Origin: http://127.0.0.1:80\r\n",
            "cmd=boot"
        )
        .code,
        200
    );
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("POST /api/v1/command"))
            .count(),
        2
    );
    // Other pages refuse POST.
    assert_eq!(
        post(
            a,
            "/services",
            "127.0.0.1",
            "Sec-Fetch-Site: same-origin\r\n",
            "x=1"
        )
        .code,
        405
    );
    // Oversized form.
    let big = "x".repeat(console_ui::MAX_FORM + 1);
    assert_eq!(
        post(
            a,
            "/agents",
            "127.0.0.1",
            "Sec-Fetch-Site: same-origin\r\n",
            &big
        )
        .code,
        413
    );
}

#[test]
fn gate_soon_pair_and_cookie() {
    let (a, log) = start(&[]);
    // Not a console host: quiet page, no data fetched.
    let r = get(a, "/agents", "galion.app", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("Soon online."));
    assert!(!r.body.contains("3.8.3-board"));
    let r = get(a, "/status", "developer.galion.app", "");
    assert!(r
        .body
        .contains("github.com/galion-studio/community/discussions"));
    // Console host without cookie: pairing page, no data.
    let r = get(a, "/agents", "console.galion.app", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("This phone"));
    assert!(r.body.contains("action=\"/_ui/pair?back=/agents\""));
    assert!(!r.body.contains("3.8.3-board"));
    // `/` redirects to /console on console hosts.
    let r = get(a, "/", "console.galion.app", "");
    assert_eq!(r.code, 302);
    assert!(r.head.contains("Location: /console"));
    // With the admin cookie the page renders; only galion_admin is forwarded.
    let r = get(
        a,
        "/agents",
        "console.galion.app",
        "Cookie: track=1; galion_admin=good\r\n",
    );
    assert!(r.body.contains("3.8.3-board"), "{}", r.body);
    let l = log.lock().unwrap().clone();
    assert!(l
        .iter()
        .any(|x| x.starts_with("GET /api/v1/admin HTTP/1.1 console.galion.app galion_admin=good")));
    assert!(!l.iter().any(|x| x.contains("track=1")));
}

#[test]
fn pairing_form_flow() {
    let (a, log) = start(&[]);
    let so = "Sec-Fetch-Site: same-origin\r\n";
    let r = post(
        a,
        "/_ui/pair?back=/services",
        "console.galion.app",
        so,
        "code=WRONG",
    );
    assert_eq!(r.code, 401);
    assert!(r.body.contains("That code was refused."));
    let r = post(
        a,
        "/_ui/pair?back=/services",
        "console.galion.app",
        so,
        "code=RIGHT",
    );
    assert_eq!(r.code, 303, "{}", r.head);
    assert!(r.head.contains("Location: /services"));
    assert!(r
        .head
        .contains("Set-Cookie: galion_admin=good; HttpOnly; Path=/; SameSite=Lax"));
    assert!(
        !r.head.contains("other=x"),
        "only the admin cookie is relayed"
    );
    // Open redirect refused.
    let r = post(
        a,
        "/_ui/pair?back=https://evil.example",
        "console.galion.app",
        so,
        "code=RIGHT",
    );
    assert!(r.head.contains("Location: /console"));
    // CSRF.
    assert_eq!(
        post(
            a,
            "/_ui/pair",
            "console.galion.app",
            "Sec-Fetch-Site: cross-site\r\n",
            "code=RIGHT"
        )
        .code,
        403
    );
    // Not on non-console hosts.
    assert_eq!(
        post(a, "/_ui/pair", "galion.app", so, "code=RIGHT").code,
        404
    );
    let posts = log
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("POST /api/v1/admin"))
        .count();
    assert_eq!(posts, 3);
}

#[test]
fn theme_css_and_fail_closed() {
    let (a, _) = start(&[]);
    let r = get(a, "/_ui/theme?to=light&back=/agents", "127.0.0.1", "");
    assert_eq!(r.code, 303);
    assert!(r.head.contains("Set-Cookie: galion_theme=light;"));
    assert!(r.head.contains("Location: /agents"));
    let r = get(a, "/agents", "127.0.0.1", "Cookie: galion_theme=light\r\n");
    assert!(r.body.contains("data-theme=\"light\""));
    let href = r
        .body
        .split("rel=\"stylesheet\" href=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let css = get(a, &href, "127.0.0.1", "");
    assert_eq!(css.code, 200);
    assert!(css.head.contains("immutable"));
    assert!(css.body.contains("--c-bg:#1e1e1e"));
    assert_eq!(
        get(a, "/_ui/console.0000000000000000.css", "127.0.0.1", "").code,
        404
    );
    assert_eq!(get(a, "/_ui/favicon.svg", "127.0.0.1", "").code, 200);
    // Unported room: "not in Rust yet" page (no SPA fallback configured).
    let r = get(a, "/hermes", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("Not in Rust yet"));
    assert_eq!(get(a, "/nope", "127.0.0.1", "").code, 404);

    // API down: console hosts fail closed (503, no page data).
    let cfg = Config::from_args(
        ["--ui", "rust", "--upstream", "127.0.0.1:9", "--quiet"]
            .iter()
            .map(|s| s.to_string()),
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let b = l.local_addr().unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /\n").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    let r = get(
        b,
        "/agents",
        "console.galion.app",
        "Cookie: galion_admin=good\r\n",
    );
    assert_eq!(r.code, 503);
    assert!(r.body.contains("not answering"));
    // Loopback still renders, with the origin shown as down.
    let r = get(b, "/agents", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("no answer"));
}
