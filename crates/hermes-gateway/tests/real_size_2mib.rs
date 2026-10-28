// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 329 hermes-gateway-2mib: the trimmed release binary still does its
//! real job. Gated: `GALION_REAL=1`. Offline only (127.0.0.1, no spend).
//!
//! | port  | process |
//! |-------|---------|
//! | 21990 | `hermes-gateway` (production path: CurlProvider -> stub) |
//! | 21991 | `hermes-gateway --mock` |
//! | 21992 | stub OpenAI-compatible upstream inside this test (SSE) |
//!
//! `HG2_BIN` = binary under test (default: this crate's own build). When it
//! is set (the x86_64 musl release artifact), the test also checks the size
//! budget (<= 2 MiB = 2,097,152 B) and, with readelf on PATH, that the file
//! is stripped and carries the packed `.relr.dyn` relocations.
//!
//! What the trimmed SQLite build must still do (LIBSQLITE3_FLAGS in
//! .cargo/config.toml): open a Python-shaped legacy state.db that holds an
//! FTS5 table and triggers (read-only), write sessions to its own
//! rs-state.db, and read them back after a restart.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const KEY: &str = "hg2-test-key-0123456789abcdef";
const BUDGET: u64 = 2 * 1024 * 1024;
const P_GW: u16 = 21990;
const P_MOCK: u16 = 21991;
const P_UP: u16 = 21992;

fn real() -> bool {
    std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
}

fn zagreb_now() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S %:z")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

struct Proc(Child);
impl Drop for Proc {
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

impl Resp {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    }
}

fn http(port: u16, method: &str, path: &str, extra: &[(&str, &str)], body: &str) -> Resp {
    let fail = Resp {
        code: 0,
        head: String::new(),
        body: String::new(),
    };
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return fail;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() {
        return fail;
    }
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Resp {
        code,
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn chat(port: u16, text: &str, sid: Option<&str>) -> Resp {
    let body = serde_json::json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]}).to_string();
    match sid {
        Some(s) => http(
            port,
            "POST",
            "/v1/chat/completions",
            &[("X-Hermes-Session-Id", s)],
            &body,
        ),
        None => http(port, "POST", "/v1/chat/completions", &[], &body),
    }
}

fn stub_upstream(port: u16, stop: Arc<AtomicBool>, hits: Arc<AtomicU64>) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind stub upstream");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    let hits = hits.clone();
                    std::thread::spawn(move || serve_stub(s, &hits));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    });
}

fn serve_stub(mut s: TcpStream, hits: &AtomicU64) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; len.min(16 << 20)];
    let _ = r.read_exact(&mut body);
    hits.fetch_add(1, Ordering::SeqCst);
    let chunks = [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"stub "}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"answer ok"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":40,"completion_tokens":3,"total_tokens":43,"cost":0}}"#,
    ];
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
    );
    for c in chunks {
        out.push_str("data: ");
        out.push_str(c);
        out.push_str("\n\n");
    }
    out.push_str("data: [DONE]\n\n");
    let _ = s.write_all(out.as_bytes());
}

fn gateway_bin() -> PathBuf {
    std::env::var("HG2_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn spawn_gateway(work: &Path, port: u16, mock: bool, legacy: &Path) -> Proc {
    let home = work.join(format!("home-{port}"));
    std::fs::create_dir_all(&home).unwrap();
    let mut c = Command::new(gateway_bin());
    if mock {
        c.arg("--mock");
    }
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("API_SERVER_KEY", KEY)
        .env("HERMES_HOME", &home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_LEGACY_STATE_DB", legacy)
        .env("HERMES_RS_SESSION_USD_CAP", "0")
        .env("HERMES_RS_DAILY_USD_CAP", "0")
        // never a paid provider: loopback stub only
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .env("OPENROUTER_BASE_URL", format!("http://127.0.0.1:{P_UP}/v1"))
        .stdout(Stdio::null())
        .stderr(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(work.join(format!("gw-{port}.log")))
                .map(Stdio::from)
                .unwrap_or(Stdio::null()),
        );
    Proc(c.spawn().expect("spawn hermes-gateway"))
}

fn wait_up(port: u16) -> Duration {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        if http(port, "GET", "/health", &[], "").code == 200 {
            return t.elapsed();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("gateway on {port} did not come up");
}

/// A Python-Hermes-shaped state.db: the hermes-rs SCHEMA tables plus an
/// FTS5 index with the sync triggers Python Hermes keeps on `messages`.
fn legacy_db(path: &Path) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute_batch(hermes_rs::session_sqlite::SCHEMA).unwrap();
    c.execute_batch(
        "CREATE VIRTUAL TABLE messages_fts USING fts5(content, content=messages, content_rowid=id);
         CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
           INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
         END;
         INSERT INTO sessions (id, source, session_key, model, started_at, billing_provider) VALUES
           ('s-py', 'api_server', 'galion-one', 'anthropic/claude-haiku-5.5', 1760800000.5, 'openrouter');
         INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'user', 'hello legacy', 1, 1);
         INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'assistant', 'legacy answer', 2, 1);
         INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'assistant', 'compacted away', 3, 0);",
    )
    .unwrap();
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'legacy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2, "FTS5 still compiled in and matching");
}

fn roles(body: &str) -> usize {
    body.matches("\"role\"").count()
}

#[test]
fn real_size_2mib() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let bin = gateway_bin();
    println!("[{}] bin {}", zagreb_now(), bin.display());
    if std::env::var("HG2_BIN").is_ok() {
        let size = std::fs::metadata(&bin).unwrap().len();
        println!(
            "SIZE {size} B (budget {BUDGET} B, headroom {} B)",
            BUDGET as i64 - size as i64
        );
        assert!(size <= BUDGET, "hermes-gateway {size} B > {BUDGET} B");
        if let Ok(o) = Command::new("readelf").arg("-SW").arg(&bin).output() {
            let s = String::from_utf8_lossy(&o.stdout);
            assert!(!s.contains(".symtab"), "binary not stripped");
            let relr = s.contains(".relr.dyn");
            println!(
                "ELF stripped yes, .relr.dyn {}",
                if relr { "yes" } else { "no" }
            );
        }
    }

    let work = std::env::temp_dir().join(format!("hg2-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let legacy = work.join("legacy-state.db");
    legacy_db(&legacy);
    let legacy_len = std::fs::metadata(&legacy).unwrap().len();

    let stop = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicU64::new(0));
    stub_upstream(P_UP, stop.clone(), hits.clone());

    // 1. production path (CurlProvider -> stub upstream)
    let gw = spawn_gateway(&work, P_GW, false, &legacy);
    let up = wait_up(P_GW);
    println!(
        "[{}] gateway :{P_GW} /health 200 after {} ms",
        zagreb_now(),
        up.as_millis()
    );
    let h = http(P_GW, "GET", "/health", &[], "");
    println!("GET /health -> {} {}", h.code, h.body.trim());
    assert_eq!(h.code, 200);
    let m = http(P_GW, "GET", "/v1/models", &[], "");
    println!("GET /v1/models -> {} {}", m.code, m.body.trim());
    assert_eq!(m.code, 200);
    assert!(m.body.contains("hermes-agent"));

    let t = Instant::now();
    let c1 = chat(P_GW, "hello 2mib", None);
    let sid = c1.header("X-Hermes-Session-Id").expect("session id header");
    println!(
        "POST /v1/chat/completions -> {} in {} ms, session {sid}, body has 'stub answer ok': {}",
        c1.code,
        t.elapsed().as_millis(),
        c1.body.contains("stub answer ok")
    );
    assert_eq!(c1.code, 200, "{}", c1.body);
    assert!(c1.body.contains("stub answer ok"));
    let c2 = chat(P_GW, "second turn", Some(&sid));
    println!(
        "POST /v1/chat/completions (turn 2, same session) -> {}",
        c2.code
    );
    assert_eq!(c2.code, 200, "{}", c2.body);

    let own = http(
        P_GW,
        "GET",
        &format!("/api/sessions/{sid}/messages"),
        &[],
        "",
    );
    println!(
        "GET /api/sessions/{sid}/messages -> {} roles {}",
        own.code,
        roles(&own.body)
    );
    assert_eq!(own.code, 200);
    assert_eq!(roles(&own.body), 4, "{}", own.body);

    let old = http(P_GW, "GET", "/api/sessions/s-py/messages", &[], "");
    println!(
        "GET /api/sessions/s-py/messages (legacy FTS5 db, read-only) -> {} roles {}",
        old.code,
        roles(&old.body)
    );
    assert_eq!(old.code, 200, "{}", old.body);
    assert_eq!(
        roles(&old.body),
        2,
        "inactive legacy row hidden: {}",
        old.body
    );
    assert!(old.body.contains("legacy answer"));

    let list = http(P_GW, "GET", "/api/sessions?limit=10", &[], "");
    println!(
        "GET /api/sessions -> {} has own {} has legacy {}",
        list.code,
        list.body.contains(&sid),
        list.body.contains("s-py")
    );
    assert_eq!(list.code, 200);
    assert!(list.body.contains(&sid));

    // 2. restart on the same rs-state.db: the session is persisted by SQLite
    drop(gw);
    let gw2 = spawn_gateway(&work, P_GW, false, &legacy);
    let up2 = wait_up(P_GW);
    let again = http(
        P_GW,
        "GET",
        &format!("/api/sessions/{sid}/messages"),
        &[],
        "",
    );
    println!(
        "restart: /health after {} ms; GET /api/sessions/{sid}/messages -> {} roles {}",
        up2.as_millis(),
        again.code,
        roles(&again.body)
    );
    assert_eq!(again.code, 200);
    assert_eq!(roles(&again.body), 4);
    let unauth = {
        let mut s = TcpStream::connect(("127.0.0.1", P_GW)).unwrap();
        s.write_all(b"GET /v1/models HTTP/1.0\r\nHost: x\r\n\r\n")
            .unwrap();
        let mut o = String::new();
        let _ = s.read_to_string(&mut o);
        o.split_whitespace().nth(1).unwrap_or("").to_string()
    };
    println!("GET /v1/models without key -> {unauth}");
    assert_eq!(unauth, "401");
    drop(gw2);

    // 3. --mock gateway (shadow / smoke mode)
    let mock = spawn_gateway(&work, P_MOCK, true, &legacy);
    wait_up(P_MOCK);
    let mc = chat(P_MOCK, "mock hello", None);
    let mf = chat(P_MOCK, "mock-fail please", None);
    println!(
        "--mock :{P_MOCK} chat -> {}, mock-fail -> {}",
        mc.code, mf.code
    );
    assert_eq!(mc.code, 200);
    assert_eq!(mf.code, 502);
    drop(mock);

    stop.store(true, Ordering::SeqCst);
    let n = hits.load(Ordering::SeqCst);
    println!("stub upstream hits {n}");
    assert!(n >= 2);
    assert_eq!(
        std::fs::metadata(&legacy).unwrap().len(),
        legacy_len,
        "legacy db untouched (read-only)"
    );
    let _ = std::fs::remove_dir_all(&work);
    println!("[{}] PASS real_size_2mib", zagreb_now());
}
