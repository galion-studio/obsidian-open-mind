// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--hermes-api` (hermes-ui-rebuild, owner rulings t359u/t360u,
//! 2026-10-29 CET +01:00): the console's `/api/hermes/*` adapter end to end
//! over loopback against a mock hermes-rs gateway (ephemeral ports, no
//! network, no paid calls). Covers: the pairing gate (401 before any
//! gateway call), the gateway key never reaching a reply, the former 404s
//! answering, ask forwarding (provider, session key, reasoning), provider
//! gating (RunPod not set up, laptop offline, OpenRouter-only aliases),
//! the $20 cap / $16 warning, SSE streaming, stop, error JSON and the
//! `/hermes` page through the console's own adapter.

use console_static::admin::sha_hex;
use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Test-only gateway key (not a real secret); must never appear in a reply.
const KEY: &str = "test-only-gateway-key-7f3a9c0d2e1b";
/// Paired-device cookie token for the test admin store.
const TOKEN: &str = "paired-device-token-for-tests";

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    head: String,
    body: String,
}

type Log = Arc<Mutex<Vec<Seen>>>;

fn tmp(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "hermes-api-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn session_rows(cost: f64) -> String {
    format!(
        r#"{{"data":[{{"id":"api-1","source":"api_server","session_key":"console:hermes:abc","model":"anthropic/claude-opus-5.5","billing_provider":"openrouter","title":"First <q>","started_at":1792340000,"ended_at":1792340805,"message_count":3,"input_tokens":1000,"output_tokens":500,"estimated_cost_usd":{cost}}},{{"id":"tg-9","source":"telegram","session_key":"tg:9","title":"Not ours","started_at":1792330000,"message_count":2,"estimated_cost_usd":99}}]}}"#
    )
}

/// Mock hermes-rs: `/health` open, everything else needs the bearer key.
fn gateway(cost: f64, log: Log) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || serve_one(s, cost, log));
        }
    });
    addr
}

fn serve_one(mut s: TcpStream, cost: f64, log: Log) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let len: usize = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse().ok())?
        })
        .unwrap_or(0);
    while buf.len() < head_end + len {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
    let mut first = head.split(' ');
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    log.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        head: head.clone(),
        body,
    });
    let authed = head
        .lines()
        .any(|l| l.eq_ignore_ascii_case(&format!("authorization: bearer {KEY}")));
    let reply = |s: &mut TcpStream, code: u16, body: &str| {
        let _ = write!(
            s,
            "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    };
    if path == "/health" {
        return reply(
            &mut s,
            200,
            r#"{"status":"ok","platform":"hermes-agent","implementation":"hermes-rs","implementation_version":"0.4.0"}"#,
        );
    }
    if !authed {
        return reply(&mut s, 401, r#"{"error":{"message":"Invalid API key"}}"#);
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/health/detailed") => reply(
            &mut s,
            200,
            r#"{"status":"ok","active_agents":0,"max_concurrent_runs":3}"#,
        ),
        ("GET", "/v1/models") => reply(
            &mut s,
            200,
            r#"{"object":"list","data":[{"id":"hermes-agent","root":"hermes-agent"},{"id":"galion","root":"anthropic/claude-haiku-5.5"}]}"#,
        ),
        ("GET", p) if p.starts_with("/api/sessions?limit=") => {
            reply(&mut s, 200, &session_rows(cost))
        }
        ("GET", "/api/sessions/api-1") => reply(
            &mut s,
            200,
            r#"{"id":"api-1","session_key":"console:hermes:abc","title":"First <q>","model":"m","started_at":1792340000,"message_count":3,"input_tokens":1,"output_tokens":2,"estimated_cost_usd":0.5}"#,
        ),
        ("GET", "/api/sessions/api-1/messages") => reply(
            &mut s,
            200,
            r#"{"data":[{"role":"user","content":"hello hermes"},{"role":"tool","name":"web_search","content":"RAW TOOL OUTPUT"},{"role":"assistant","content":"hi there"}]}"#,
        ),
        ("GET", "/api/spend") => reply(
            &mut s,
            200,
            r#"{"day":"2026-10-29","day_usd":1.5,"day_calls":4,"caps":{"session_usd":5,"daily_usd":7,"daily_calls":2000},"models":[]}"#,
        ),
        ("POST", "/v1/runs") => reply(&mut s, 202, r#"{"run_id":"run_abc123","status":"started"}"#),
        ("GET", "/v1/runs/run_abc123") => reply(
            &mut s,
            200,
            r#"{"run_id":"run_abc123","status":"completed","output":"done"}"#,
        ),
        ("POST", "/v1/runs/run_abc123/stop") => reply(
            &mut s,
            200,
            r#"{"run_id":"run_abc123","status":"stopping"}"#,
        ),
        ("GET", "/v1/runs/run_abc123/events") => {
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
            );
            for ev in [
                r#"{"event":"run.started","run_id":"run_abc123","session_id":"api-2"}"#,
                r#"{"event":"message.delta","run_id":"run_abc123","delta":"Hel"}"#,
                r#"{"event":"message.delta","run_id":"run_abc123","delta":"lo"}"#,
                r#"{"event":"run.completed","run_id":"run_abc123","output":"Hello"}"#,
            ] {
                let _ = write!(s, "data: {ev}\n\n");
                let _ = s.flush();
            }
        }
        _ => reply(&mut s, 404, r#"{"error":{"message":"not found"}}"#),
    }
}

struct Box {
    addr: SocketAddr,
    log: Log,
}

/// Console with `--hermes-api` against a mock gateway charging `cost` to the
/// console's sessions. `key`: write a key file. `extra`: more flags.
fn start(cost: f64, key: bool, extra: &[&str]) -> Box {
    let log: Log = Arc::default();
    let gw = gateway(cost, Arc::clone(&log));
    let dir = tmp("box");
    let store = dir.join("admin-device.json");
    std::fs::write(
        &store,
        format!(r#"{{"tokenHashes":["{}"]}}"#, sha_hex(TOKEN)),
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--local-api".into(),
        "--quiet".into(),
        "--bind".into(),
        addr.to_string(),
        "--hermes".into(),
        gw.to_string(),
        "--hermes-api".into(),
        "--admin-store".into(),
        store.display().to_string(),
    ];
    if key {
        let f = dir.join("hermes.env");
        std::fs::write(&f, format!("# test\nOTHER=1\nAPI_SERVER_KEY={KEY}\n")).unwrap();
        args.push("--hermes-key-file".into());
        args.push(f.display().to_string());
    }
    args.extend(extra.iter().map(|s| s.to_string()));
    let cfg = Config::from_args(args).unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    Box { addr, log }
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn send(a: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(a).unwrap();
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

fn post(a: SocketAddr, path: &str, host: &str, extra: &str, body: &str) -> Resp {
    send(
        a,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn ask(a: SocketAddr, body: &str) -> Resp {
    post(a, "/api/hermes/ask", "127.0.0.1", "", body)
}

fn no_key(r: &Resp, what: &str) {
    assert!(!r.body.contains(KEY), "{what}: key in body");
    assert!(!r.head.contains(KEY), "{what}: key in head");
    assert!(!r.body.contains("Bearer"), "{what}: auth header echoed");
}

const READS: &[&str] = &[
    "/api/hermes/health",
    "/api/hermes/guard",
    "/api/hermes/history",
    "/api/hermes/models",
    "/api/hermes/modes",
    "/api/hermes/skills",
    "/api/hermes/providers",
    "/api/hermes/spend",
];

#[test]
fn unpaired_requests_get_401_before_the_gateway() {
    let b = start(1.0, true, &[]);
    for p in READS.iter().chain(
        [
            "/api/hermes/history?id=api-1",
            "/api/hermes/runs/run_abc123/events",
        ]
        .iter(),
    ) {
        let r = get(b.addr, p, "console.galion.app", "");
        assert_eq!(r.code, 401, "{p}: {}", r.body);
        let r = get(
            b.addr,
            p,
            "console.galion.app",
            "Cookie: galion_admin=wrong\r\n",
        );
        assert_eq!(r.code, 401, "{p} bad cookie");
        // Not a console host at all.
        let r = get(b.addr, p, "evil.example", "");
        assert_eq!(r.code, 401, "{p} foreign host");
    }
    for p in [
        "/api/hermes/ask",
        "/api/hermes/run",
        "/api/hermes/runs/run_abc123/stop",
    ] {
        let r = post(b.addr, p, "console.galion.app", "", r#"{"input":"hi"}"#);
        assert_eq!(r.code, 401, "{p}");
    }
    assert!(
        b.log.lock().unwrap().is_empty(),
        "nothing reached the gateway"
    );
    // Paired cookie on the public host passes.
    let r = get(
        b.addr,
        "/api/hermes/health",
        "console.galion.app",
        &format!("Cookie: galion_admin={TOKEN}\r\n"),
    );
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains("\"ok\":true"));
    no_key(&r, "paired health");
}

#[test]
fn every_read_answers_and_never_leaks_the_key() {
    let b = start(1.0, true, &[]);
    for p in READS {
        let r = get(b.addr, p, "127.0.0.1", "");
        assert_eq!(r.code, 200, "{p}: {}", r.body);
        assert!(r.head.contains("Content-Type: application/json"), "{p}");
        assert!(r.head.contains("Cache-Control: no-store"), "{p}");
        assert!(r.body.starts_with("{\"ok\":true"), "{p}: {}", r.body);
        no_key(&r, p);
    }
    let h = get(b.addr, "/api/hermes/health", "127.0.0.1", "").body;
    for n in [
        "\"implementation\":\"hermes-rs\"",
        "\"version\":\"0.4.0\"",
        "\"key\":true",
        "\"auth_ok\":true",
    ] {
        assert!(h.contains(n), "health {n}: {h}");
    }
    assert!(
        !h.contains("\"bind\""),
        "health does not advertise the bind"
    );
    let g = get(b.addr, "/api/hermes/guard", "127.0.0.1", "").body;
    for n in [
        "\"on\":true",
        "\"provider\":\"river\"",
        "\"cap_usd\":20",
        "\"warn_usd\":16",
        "\"level\":\"ok\"",
        "\"spent_usd\":1",
    ] {
        assert!(g.contains(n), "guard {n}: {g}");
    }
    // Only console-keyed sessions count toward the research cap.
    assert!(!g.contains("\"spent_usd\":100"), "{g}");
    let hist = get(b.addr, "/api/hermes/history", "127.0.0.1", "").body;
    assert!(hist.contains("\"id\":\"api-1\"") && hist.contains("\"console\":true"));
    assert!(hist.contains("\"id\":\"tg-9\"") && hist.contains("\"console\":false"));
    let t = get(b.addr, "/api/hermes/history?id=api-1", "127.0.0.1", "").body;
    assert!(t.contains("hello hermes") && t.contains("hi there"), "{t}");
    assert!(t.contains("\"name\":\"web_search\""));
    assert!(!t.contains("RAW TOOL OUTPUT"), "tool output stays hidden");
    let r = get(b.addr, "/api/hermes/history?id=..%2Fx", "127.0.0.1", "");
    assert_eq!(r.code, 400);
    let pv = get(b.addr, "/api/hermes/providers", "127.0.0.1", "").body;
    assert!(pv.contains("\"default\":\"openrouter\""), "{pv}");
    // Every gateway call except /health carried the bearer key.
    let log = b.log.lock().unwrap();
    assert!(!log.is_empty());
    for s in log.iter().filter(|s| s.path != "/health") {
        assert!(
            s.head.contains(&format!("Authorization: Bearer {KEY}")),
            "{s:?}"
        );
    }
    assert!(log.iter().all(|s| s.method == "GET"), "reads only GET");
}

#[test]
fn ask_forwards_with_openrouter_default_and_a_console_session_key() {
    let b = start(1.0, true, &["--hermes-providers", "openrouter,river"]);
    let r = ask(b.addr, r#"{"input":"  What does an H200 cost?  "}"#);
    assert_eq!(r.code, 202, "{}", r.body);
    no_key(&r, "ask");
    for n in [
        "\"ok\":true",
        "\"run_id\":\"run_abc123\"",
        "\"provider\":\"openrouter\"",
        "\"mode\":\"deep\"",
        "\"warn\":false",
    ] {
        assert!(r.body.contains(n), "{n}: {}", r.body);
    }
    let log = b.log.lock().unwrap().clone();
    let run = log
        .iter()
        .find(|s| s.path == "/v1/runs")
        .expect("POST /v1/runs");
    assert_eq!(run.method, "POST");
    assert!(run.head.contains(&format!("Authorization: Bearer {KEY}")));
    assert!(run.head.contains("X-Hermes-Session-Key: console:hermes:"));
    assert!(
        run.body.contains("\"input\":\"What does an H200 cost?\""),
        "{}",
        run.body
    );
    assert!(run.body.contains("\"provider\":\"openrouter\""));
    assert!(run.body.contains("\"effort\":\"high\""));
    assert!(run.body.contains("You are Hermes Research"));
    assert!(
        !run.body.contains("\"model\""),
        "no model: the gateway default (as before)"
    );
    // Continue a thread: no new session key, session id passed.
    let r = ask(
        b.addr,
        r#"{"input":"more","session_id":"api-1","mode":"quick","provider":"river"}"#,
    );
    assert_eq!(r.code, 202, "{}", r.body);
    let log = b.log.lock().unwrap().clone();
    let run = log.iter().rev().find(|s| s.path == "/v1/runs").unwrap();
    assert!(!run.head.contains("X-Hermes-Session-Key"));
    assert!(run.body.contains("\"session_id\":\"api-1\""));
    assert!(run.body.contains("\"provider\":\"river\""));
    assert!(run.body.contains("\"effort\":\"low\""));
    // OpenRouter alias from /v1/models.
    let r = ask(
        b.addr,
        r#"{"input":"x","provider":"openrouter","model":"galion"}"#,
    );
    assert_eq!(r.code, 202, "{}", r.body);
    let log = b.log.lock().unwrap().clone();
    let run = log.iter().rev().find(|s| s.path == "/v1/runs").unwrap();
    assert!(run.body.contains("\"model\":\"galion\""));
}

#[test]
fn provider_gating_and_validation_never_reach_the_gateway() {
    // Laptop probe points at a closed loopback port (offline).
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let b = start(
        1.0,
        true,
        &[
            "--hermes-providers",
            "openrouter,river,laptop",
            "--hermes-laptop",
            &closed.to_string(),
        ],
    );
    let pv = get(b.addr, "/api/hermes/providers", "127.0.0.1", "").body;
    for n in [
        r#""id":"laptop","label":"Local laptop""#,
        r#""id":"runpod","label":"RunPod""#,
        r#""id":"openrouter","label":"OpenRouter""#,
        r#""id":"river","label":"River AI""#,
    ] {
        assert!(pv.contains(n), "{n}: {pv}");
    }
    assert!(pv.contains("offline") && pv.contains("not set up"), "{pv}");
    let before = b
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.path == "/v1/runs")
        .count();
    for (body, code) in [
        (r#"{"input":"x","provider":"runpod"}"#, 409),
        (r#"{"input":"x","provider":"laptop"}"#, 503),
        (r#"{"input":"x","provider":"anthropic"}"#, 400),
        (r#"{"input":"x","provider":"river","model":"galion"}"#, 400),
        (r#"{"input":"x","model":"not-offered"}"#, 400),
        (r#"{"input":"x","session_id":"../etc"}"#, 400),
        (r#"{"input":"   "}"#, 400),
        ("not json", 400),
    ] {
        let r = ask(b.addr, body);
        assert_eq!(r.code, code, "{body}: {}", r.body);
        assert!(r.body.starts_with("{\"ok\":false,\"error\":"), "{}", r.body);
        assert!(r.body.contains("\"route\":\"/api/hermes/ask\""));
    }
    let long = format!(r#"{{"input":"{}"}}"#, "a".repeat(12_001));
    assert_eq!(ask(b.addr, &long).code, 413);
    let after = b
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.path == "/v1/runs")
        .count();
    assert_eq!(before, after, "refused asks never start a run");
    // Without --hermes-providers only OpenRouter is enabled.
    let c = start(1.0, true, &[]);
    assert_eq!(ask(c.addr, r#"{"input":"x","provider":"river"}"#).code, 409);
}

#[test]
fn spend_cap_blocks_at_20_and_warns_at_16() {
    let warn = start(16.5, true, &[]);
    let g = get(warn.addr, "/api/hermes/guard", "127.0.0.1", "").body;
    assert!(g.contains("\"level\":\"warn\""), "{g}");
    let r = ask(warn.addr, r#"{"input":"x"}"#);
    assert_eq!(r.code, 202);
    assert!(r.body.contains("\"warn\":true"), "{}", r.body);

    let blocked = start(20.0, true, &[]);
    let g = get(blocked.addr, "/api/hermes/guard", "127.0.0.1", "").body;
    assert!(g.contains("\"level\":\"blocked\""), "{g}");
    let r = ask(blocked.addr, r#"{"input":"x"}"#);
    assert_eq!(r.code, 429, "{}", r.body);
    assert!(r.body.contains("research cap reached"));
    assert!(r.body.contains("\"spent_usd\":20"));
    assert!(
        !blocked
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.path == "/v1/runs"),
        "a blocked ask never starts a run"
    );
}

#[test]
fn streams_events_and_stops_runs() {
    let b = start(1.0, true, &[]);
    let r = get(
        b.addr,
        "/api/hermes/runs/run_abc123/events",
        "127.0.0.1",
        "",
    );
    assert_eq!(r.code, 200, "{}", r.head);
    assert!(r.head.contains("text/event-stream"), "{}", r.head);
    no_key(&r, "events");
    assert!(r.body.contains(r#""event":"message.delta""#), "{}", r.body);
    assert!(r.body.contains(r#""event":"run.completed""#));
    let log = b.log.lock().unwrap().clone();
    let ev = log
        .iter()
        .find(|s| s.path == "/v1/runs/run_abc123/events")
        .unwrap();
    assert!(ev.head.contains(&format!("Authorization: Bearer {KEY}")));
    assert!(
        !ev.head.contains("galion_admin"),
        "browser cookie not forwarded"
    );
    let r = get(b.addr, "/api/hermes/runs/run_abc123", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("\"status\":\"completed\""));
    let r = post(
        b.addr,
        "/api/hermes/runs/run_abc123/stop",
        "127.0.0.1",
        "",
        "{}",
    );
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains("stopping"));
    no_key(&r, "stop");
    assert_eq!(
        get(b.addr, "/api/hermes/runs/bad$id", "127.0.0.1", "").code,
        400
    );
}

#[test]
fn clear_error_json() {
    let b = start(1.0, true, &[]);
    let r = post(
        b.addr,
        "/api/hermes/guard",
        "127.0.0.1",
        "",
        r#"{"on":false}"#,
    );
    assert_eq!(r.code, 405);
    assert!(
        r.body.contains("guard toggles are not supported"),
        "{}",
        r.body
    );
    let r = get(b.addr, "/api/hermes/ask", "127.0.0.1", "");
    assert_eq!(r.code, 405);
    let r = post(b.addr, "/api/hermes/health", "127.0.0.1", "", "{}");
    assert_eq!(r.code, 405);
    let r = get(b.addr, "/api/hermes/nope", "127.0.0.1", "");
    assert_eq!(r.code, 404);
    assert!(r.body.contains("\"route\":\"/api/hermes/nope\""));

    // No key: reads that need it say so (503), health still answers.
    let nk = start(1.0, false, &[]);
    let h = get(nk.addr, "/api/hermes/health", "127.0.0.1", "");
    assert_eq!(h.code, 200);
    assert!(h.body.contains("\"key\":false"), "{}", h.body);
    let r = ask(nk.addr, r#"{"input":"x"}"#);
    assert_eq!(r.code, 503, "{}", r.body);

    // Gateway down: 502 JSON with the route, never a hang or HTML.
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let dir = tmp("down");
    let f = dir.join("hermes.env");
    std::fs::write(&f, format!("API_SERVER_KEY={KEY}\n")).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let cfg = Config::from_args(
        [
            "--ui",
            "rust",
            "--local-api",
            "--quiet",
            "--bind",
            &addr.to_string(),
            "--hermes",
            &dead.to_string(),
            "--hermes-api",
            "--hermes-key-file",
            &f.display().to_string(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>(),
    )
    .unwrap();
    let st = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, st));
    let r = get(addr, "/api/hermes/history", "127.0.0.1", "");
    assert_eq!(r.code, 502, "{}", r.body);
    assert!(r.body.starts_with("{\"ok\":false,\"error\":"));
    let h = get(addr, "/api/hermes/health", "127.0.0.1", "");
    assert_eq!(h.code, 200);
    assert!(h.body.contains("\"ok\":false") && h.body.contains("\"mode\":\"down\""));
    no_key(&r, "down");
}

#[test]
fn hermes_page_renders_through_the_adapter() {
    let b = start(1.0, true, &["--hermes-providers", "openrouter,river"]);
    let r = get(b.addr, "/hermes", "127.0.0.1", "");
    assert_eq!(r.code, 200, "{}", r.body);
    no_key(&r, "page");
    for n in [
        "Hermes Research",
        "hermes-rs 0.4.0",
        "on · River AI",
        "$1.00 of $20.00",
        "value=\"openrouter\" checked>",
        "value=\"river\">",
        "value=\"runpod\" disabled>",
        ">not set up</span>",
        "<option value=\"galion\">galion · anthropic/claude-haiku-5.5</option>",
        "href=\"/hermes?id=api-1\"",
        "<script src=\"/_ui/hermes.",
    ] {
        assert!(r.body.contains(n), "missing {n}");
    }
    assert!(r.head.contains("script-src 'self'; connect-src 'self'"));
    let r = get(b.addr, "/hermes?id=api-1", "127.0.0.1", "");
    assert!(
        r.body.contains("<div class=\"hm-text\">hello hermes</div>"),
        "{}",
        r.body
    );
    assert!(!r.body.contains("RAW TOOL OUTPUT"));
    // The page itself is gated like every room.
    let r = get(b.addr, "/hermes", "console.galion.app", "");
    assert!(
        !r.body.contains("hm-composer"),
        "unpaired sees the pairing page"
    );
}

#[test]
fn flags_are_checked() {
    let base = ["--ui", "rust", "--local-api", "--bind", "127.0.0.1:0"];
    let parse = |extra: &[&str]| {
        Config::from_args(
            base.iter()
                .chain(extra.iter())
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
        )
    };
    assert!(parse(&["--hermes-api"]).is_err(), "needs --hermes");
    assert!(
        parse(&["--hermes", "127.0.0.1:8642"]).is_err(),
        "needs --hermes-api or owner routes"
    );
    let c = parse(&["--hermes", "127.0.0.1:8642", "--hermes-api"]).unwrap();
    assert_eq!(c.hermes_providers, vec!["openrouter".to_string()]);
    assert!(parse(&[
        "--hermes",
        "127.0.0.1:8642",
        "--hermes-api",
        "--hermes-providers",
        "openrouter,bogus"
    ])
    .is_err());
    assert!(parse(&[
        "--hermes",
        "127.0.0.1:8642",
        "--hermes-api",
        "--hermes-laptop",
        "10.0.0.5:11434"
    ])
    .is_err());
    assert!(parse(&[
        "--hermes",
        "127.0.0.1:8642",
        "--hermes-api",
        "--hermes-key-file",
        "relative.env"
    ])
    .is_err());
    assert!(
        parse(&["--hermes-providers", "openrouter"]).is_err(),
        "needs --hermes-api"
    );
    let c = parse(&[
        "--hermes",
        "127.0.0.1:8642",
        "--hermes-api",
        "--hermes-providers",
        "river,openrouter,river",
        "--hermes-laptop",
        "127.0.0.1:11434",
    ])
    .unwrap();
    assert_eq!(
        c.hermes_providers,
        vec!["river".to_string(), "openrouter".to_string()]
    );
    assert_eq!(c.hermes_laptop, Some("127.0.0.1:11434".parse().unwrap()));
}
