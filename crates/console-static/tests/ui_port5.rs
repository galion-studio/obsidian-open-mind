// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end tests for the console-port-5 rooms (`/node`, `/security`,
//! `/session`): the real server (`--ui rust`) in-process, a fake console
//! API on an ephemeral loopback port, raw HTTP/1.1. Covers rendering, the
//! gate, CSRF refusals, the fixed Scan/Heal posts, the Defender PIN header
//! (never echoed), the session file-name rule (bad names never reach the
//! API), the zip link through the `/api/` proxy, and that `/node` and
//! `/session` only GET.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    head: String,
    body: String,
}

type Log = Arc<Mutex<Vec<Seen>>>;

const HEALTH: &str = r#"{"status":"healthy","version":"3.8.3-board","target":{"provider":"cpu","pod":"ip50","podId":"ip50","sshUser":"root","sshHost":"203.0.113.7","sshDirect":"root@203.0.113.7","sshPort":22,"publicIp":"203.0.113.7"}}"#;
const SECURITY: &str = r#"{"ok":true,"denylist":true,"autoAccept":true,"floorAlways":true,"historyRedaction":true,"auth":false,"floor":["gateway stop, kill, or freeze of the watchdog","secret files and the OpenRouter key","editing the guard or its patterns","/etc, docker.sock, libsqlite, reboot"],"files":[{"path":"/root/galion-secrets/github.token","mode":"400","ok":true},{"path":"/root/galion-secrets/hermes.env","mode":"missing","ok":false}],"volume":[{"path":"/workspace/galion-light/secrets/hermes.key","mode":"sealed","ok":true}]}"#;
const GUARDIAN: &str = r#"{"status":"watching","health":"steady","score":97,"activeIncidents":1,"healingActions":4,"library":{"count":9},"checks":[{"service":"console","status":"healthy","response_time_ms":3},{"service":"tuner","status":"down","error":"timeout"}],"lastScan":"2026-10-21T03:30:05.000Z"}"#;
const INCIDENTS: &str =
    r#"[{"id":"i1","title":"Secret mode <loose>","severity":"high","status":"open"}]"#;
const LIST: &str = r#"{"ok":true,"items":[{"name":"00-summary.md","bytes":1234},{"name":"01-from-zero.md","bytes":88}]}"#;
const DOC: &str = "# Summary\n\n<script>alert(1)</script> & more\n";

fn fake(log: Log) -> SocketAddr {
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
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("content-length").then(|| v.trim())
                })
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
    let method = first.split(' ').next().unwrap_or("").to_string();
    let path = first.split(' ').nth(1).unwrap_or("").to_string();
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Cookie: "))
        .unwrap_or("")
        .to_string();
    log.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        head: head.clone(),
        body: body.clone(),
    });
    let paired = cookie.contains("galion_admin=good");
    let refuse = cookie.contains("galion_admin=bad");
    let (code, ctype, out): (u16, &str, String) = match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/admin") => (
            200,
            "application/json",
            format!("{{\"ok\":true,\"you\":{paired}}}"),
        ),
        ("GET", "/api/v1/health") => (200, "application/json", HEALTH.into()),
        (_, p)
            if refuse
                && (p.starts_with("/api/v1/session")
                    || p.starts_with("/api/v1/security")
                    || p.starts_with("/api/v3/guardian")) =>
        {
            (
                401,
                "application/json",
                r#"{"error":"phone required"}"#.into(),
            )
        }
        ("GET", "/api/v1/security") => (200, "application/json", SECURITY.into()),
        ("GET", "/api/v3/guardian/status") => (200, "application/json", GUARDIAN.into()),
        ("GET", "/api/v3/guardian/incidents") => (200, "application/json", INCIDENTS.into()),
        ("GET", "/api/v1/session") => (200, "application/json", LIST.into()),
        ("GET", "/api/v1/session?name=00-summary.md") => {
            (200, "text/markdown; charset=utf-8", DOC.into())
        }
        ("GET", "/api/v1/session?name=01-from-zero.md") => {
            (404, "application/json", r#"{"error":"not found"}"#.into())
        }
        ("GET", "/api/v1/session-pack") => (200, "application/zip", "PK\u{3}\u{4}zip".into()),
        ("POST", "/api/v3/guardian/scan") => (200, "application/json", r#"{"ok":true}"#.into()),
        ("POST", "/api/v3/guardian/heal") if head.contains("\r\nX-Galion-Pin: 4242") => {
            (200, "application/json", r#"{"ok":true}"#.into())
        }
        ("POST", "/api/v3/guardian/heal") => {
            (403, "application/json", r#"{"error":"bad pin"}"#.into())
        }
        _ => (404, "application/json", "{}".into()),
    };
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
}

fn start() -> (SocketAddr, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let api = fake(Arc::clone(&log));
    let args = vec![
        "--ui".to_string(),
        "rust".into(),
        "--upstream".into(),
        api.to_string(),
        "--quiet".into(),
    ];
    let cfg = Config::from_args(args).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /\n").unwrap(),
        cfg,
    ));
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

fn get(a: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        a,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn post_at(a: SocketAddr, path: &str, host: &str, extra: &str, body: &str) -> Resp {
    send(
        a,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn post(a: SocketAddr, path: &str, extra: &str, body: &str) -> Resp {
    post_at(a, path, "127.0.0.1", extra, body)
}

const SAME: &str = "Sec-Fetch-Site: same-origin\r\n";

fn posts(log: &Log) -> Vec<Seen> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|s| s.method != "GET")
        .cloned()
        .collect()
}

fn clean(r: &Resp, p: &str) {
    assert!(!r.body.to_ascii_lowercase().contains("<script"), "{p}");
    assert!(
        !r.body.contains("style=\""),
        "{p}: inline style breaks the CSP"
    );
    assert!(!r.body.contains("Not in Rust yet"), "{p}");
    assert!(
        r.head
            .contains("Content-Security-Policy: default-src 'none'"),
        "{p}"
    );
}

#[test]
fn three_rooms_are_rust_and_plain_views_only_get() {
    let (a, log) = start();
    for p in [
        "/node",
        "/security",
        "/session",
        "/session?name=00-summary.md",
    ] {
        let r = get(a, p, "127.0.0.1", "");
        assert_eq!(r.code, 200, "{p}");
        clean(&r, p);
        assert!(!r.body.contains("did not load"), "{p}: {}", r.body);
    }
    assert!(posts(&log).is_empty(), "plain GETs must not POST upstream");
    for p in ["/node", "/security", "/session"] {
        assert!(console_ui::PORTED.contains(&p));
        // HEAD answers like GET, without a body.
        let r = send(
            a,
            &format!("HEAD {p} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
        );
        assert_eq!(r.code, 200, "HEAD {p}");
    }
    // /node and /session take no POST.
    for p in ["/node", "/session"] {
        assert_eq!(post(a, p, SAME, "act=scan").code, 405, "{p}");
    }
    assert!(posts(&log).is_empty());
}

#[test]
fn node_room() {
    let (a, _) = start();
    let r = get(a, "/node", "127.0.0.1", "");
    for n in [
        "ssh root@203.0.113.7 -p 22 -i ~/.ssh/id_ed25519",
        "<span>Last IP</span><span class=\"muted mono\">203.0.113.7</span>",
        "8888,80,8080,19123",
        "galion-initiative-web/cpu-light/galion-light/flash.sh | bash; exec /start.sh",
        "Container start command",
        "Flash onto the pod",
        "<title>Galion Console · Pod</title>",
    ] {
        assert!(r.body.contains(n), "missing {n}");
    }
}

#[test]
fn security_room_renders_the_fort() {
    let (a, log) = start();
    let r = get(a, "/security", "127.0.0.1", "");
    for n in [
        "Watch · 24/7 · steady · 05:30:05 CEST",
        "<li><span>Score</span><span class=\"muted\">97</span></li>",
        "<li><span>Denylist</span><span class=\"muted\">on</span></li>",
        "<li><span>Incidents</span><span class=\"muted\">1</span></li>",
        "<li><span>Notes</span><span class=\"muted\">9</span></li>",
        "Auto-accept is on",
        "<li><span>tuner</span><span class=\"err\">down</span></li>",
        "<li><span>galion-secrets/github.token</span><span class=\"muted\">400</span></li>",
        "<li><span>galion-secrets/hermes.env</span><span class=\"err\">missing</span></li>",
        "<li><span>secrets/hermes.key</span><span class=\"muted\">sealed</span></li>",
        "Secret mode &lt;loose&gt;",
        "Heals logged 4.",
        "/etc, docker.sock, libsqlite, reboot",
    ] {
        assert!(r.body.contains(n), "missing {n}");
    }
    let gets: Vec<String> = log.lock().unwrap().iter().map(|s| s.path.clone()).collect();
    for p in [
        "/api/v1/security",
        "/api/v3/guardian/status",
        "/api/v3/guardian/incidents",
    ] {
        assert!(gets.iter().any(|g| g == p), "{p} not read");
    }
}

#[test]
fn security_posts_are_csrf_checked_and_fixed() {
    let (a, log) = start();
    // No same-origin proof: 403, nothing upstream.
    assert_eq!(post(a, "/security", "", "act=scan").code, 403);
    assert_eq!(
        post(a, "/security", "Sec-Fetch-Site: cross-site\r\n", "act=scan").code,
        403
    );
    assert_eq!(
        post(
            a,
            "/security",
            "Origin: https://evil.example\r\n",
            "act=scan"
        )
        .code,
        403
    );
    assert!(posts(&log).is_empty());
    // Same-origin by Origin header works too.
    let r = post(a, "/security", "Origin: http://127.0.0.1\r\n", "act=scan");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("scan: HTTP 200"));
    // Scan with extra fields: path and body stay the binary's.
    let r = post(
        a,
        "/security",
        SAME,
        "act=scan&path=/api/hermes/guard&body=%7B%22on%22%3Afalse%7D",
    );
    assert_eq!(r.code, 200);
    let p = posts(&log);
    assert_eq!(p.len(), 2, "{p:?}");
    for s in &p {
        assert_eq!(s.path, "/api/v3/guardian/scan");
        assert_eq!(s.body, "{}");
        assert!(s.head.contains("Content-Type: application/json"));
        assert!(!s.head.contains("X-Galion-Pin"));
    }
    // Unknown actions, including the denylist switch, send nothing.
    for body in ["act=denylist&on=false", "act=guard", "act=", "x=1"] {
        let r = post(a, "/security", SAME, body);
        assert_eq!(r.code, 400, "{body}");
        assert!(r.body.contains("Unknown action (nothing was sent)."));
    }
    assert_eq!(posts(&log).len(), 2);
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .all(|s| !s.path.starts_with("/api/hermes")));
}

#[test]
fn heal_pin_only_in_the_header_never_in_the_page() {
    let (a, log) = start();
    // Empty / whitespace / header-injection PINs: 400, nothing sent.
    for body in [
        "act=heal",
        "act=heal&pin=",
        "act=heal&pin=+++",
        "act=heal&pin=42%0D%0AX-Evil%3A+1",
        "act=heal&pin=12+34",
    ] {
        let r = post(a, "/security", SAME, body);
        assert_eq!(r.code, 400, "{body}");
        assert!(r.body.contains("enter the Defender PIN (nothing was sent)"));
    }
    assert!(posts(&log).is_empty());
    // Right PIN: sent as X-Galion-Pin, body {}, never echoed.
    let r = post(a, "/security", SAME, "act=heal&pin=4242");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("heal: HTTP 200"));
    assert!(!r.body.contains("4242"), "PIN echoed");
    // Wrong PIN: the refusal is shown, the PIN is not.
    let r = post(a, "/security", SAME, "act=heal&pin=s3cr%21t");
    assert!(r.body.contains("heal: refused, HTTP 403 (bad pin)"));
    assert!(!r.body.contains("s3cr"), "PIN echoed");
    let p = posts(&log);
    assert_eq!(p.len(), 2);
    assert!(p
        .iter()
        .all(|s| s.path == "/api/v3/guardian/heal" && s.body == "{}"));
    assert!(p[0].head.contains("\r\nX-Galion-Pin: 4242"));
    assert!(p[1].head.contains("\r\nX-Galion-Pin: s3cr!t"));
    assert!(!p[1].body.contains("s3cr"), "PIN must not be in the body");
}

#[test]
fn session_room_list_doc_and_name_rule() {
    let (a, log) = start();
    let r = get(a, "/session", "127.0.0.1", "");
    assert!(r
        .body
        .contains("Notes from this pod. 2 files. No secrets in the pack."));
    assert!(r.body.contains("href=\"/session?name=00-summary.md\""));
    assert!(r.body.contains("href=\"/api/v1/session-pack\""));
    assert!(r.body.contains(">1234</span>"));
    let r = get(a, "/session?name=00-summary.md", "127.0.0.1", "");
    assert!(r.body.contains(
        "<pre class=\"code\"># Summary\n\n&lt;script&gt;alert(1)&lt;/script&gt; &amp; more\n</pre>"
    ));
    let r = get(a, "/session?name=01-from-zero.md", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r
        .body
        .contains("The file did not load: HTTP 404 (not found)."));
    log.lock().unwrap().clear();
    for bad in [
        "../../etc/passwd",
        "..%2F..%2Fetc%2Fpasswd.md",
        "a%2Fb.md",
        ".env.md",
        "x.txt",
        "00-summary.md%00",
        "a%20b.md",
    ] {
        let r = get(a, &format!("/session?name={bad}"), "127.0.0.1", "");
        assert_eq!(r.code, 400, "{bad}");
        assert!(r.body.contains("Nothing was asked."), "{bad}");
    }
    let named: Vec<String> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.path.contains("name="))
        .map(|s| s.path.clone())
        .collect();
    assert!(named.is_empty(), "bad names reached the API: {named:?}");
    // The zip link goes through the /api/ proxy to the console API.
    let r = get(a, "/api/v1/session-pack", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.head.to_ascii_lowercase().contains("application/zip"));
}

#[test]
fn refusals_are_stated_not_hidden() {
    let (a, log) = start();
    let bad = "Cookie: galion_admin=bad\r\n";
    let r = get(a, "/security", "127.0.0.1", bad);
    assert_eq!(r.code, 200);
    assert!(r
        .body
        .contains("Security did not load: HTTP 401 (phone required)."));
    assert!(r
        .body
        .contains("Guardian status did not load: HTTP 401 (phone required)."));
    assert!(r
        .body
        .contains("<li><span>Denylist</span><span class=\"muted\">—</span></li>"));
    assert!(!r.body.contains("None open."));
    let r = get(a, "/session", "127.0.0.1", bad);
    assert!(r
        .body
        .contains("The file list did not load: HTTP 401 (phone required)."));
    // The admin cookie is forwarded, other cookies are not.
    let _ = get(
        a,
        "/session",
        "127.0.0.1",
        "Cookie: track=1; galion_admin=bad\r\n",
    );
    let seen = log.lock().unwrap().clone();
    let s = seen
        .iter()
        .rev()
        .find(|s| s.path == "/api/v1/session")
        .unwrap();
    assert!(s.head.contains("Cookie: galion_admin=bad"));
    assert!(!s.head.contains("track=1"));
}

#[test]
fn gate_applies_to_the_three_rooms() {
    let (a, log) = start();
    for p in ["/node", "/security", "/session"] {
        // Public console host, unpaired: pairing page, no room data read.
        let r = get(a, p, "console.galion.app", "");
        assert_eq!(r.code, 200, "{p}");
        assert!(r.body.contains("name=\"code\""), "{p}: pairing form");
        assert!(!r.body.contains("Build session") && !r.body.contains("The fort"));
        // A public non-console host: the Soon page.
        let r = get(a, p, "galion.app", "");
        assert_eq!(r.code, 200);
        assert!(!r.body.contains("The fort"));
    }
    // Unpaired POST from the public host: nothing upstream.
    let _ = post_at(a, "/security", "console.galion.app", SAME, "act=scan");
    assert!(posts(&log).is_empty());
    let read: Vec<String> = log
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.path.clone())
        .filter(|p| p != "/api/v1/admin")
        .collect();
    assert!(read.is_empty(), "unpaired reads: {read:?}");
    // Paired phone on the public host: the room renders.
    let r = get(
        a,
        "/security",
        "console.galion.app",
        "Cookie: galion_admin=good\r\n",
    );
    assert!(r.body.contains("The fort"));
    let r = post_at(
        a,
        "/security",
        "console.galion.app",
        &format!("{SAME}Cookie: galion_admin=good\r\n"),
        "act=scan",
    );
    assert!(r.body.contains("scan: HTTP 200"));
    assert_eq!(posts(&log).len(), 1);
}

#[test]
fn every_emitted_class_is_styled() {
    let (a, _) = start();
    let defined = console_ui::css::defined_classes();
    let mut missing = Vec::new();
    for p in [
        "/node",
        "/security",
        "/session",
        "/session?name=00-summary.md",
        "/session?name=x.txt",
    ] {
        let r = get(a, p, "127.0.0.1", "");
        for cap in r.body.split("class=\"").skip(1) {
            for c in cap.split('"').next().unwrap_or("").split_whitespace() {
                let compound = [
                    "primary", "ghost", "online", "offline", "held", "neutral", "i-open", "i-close",
                ];
                if !defined.iter().any(|d| d == c) && !compound.contains(&c) {
                    missing.push(format!("{p}: {c}"));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "unstyled: {missing:?}");
}
