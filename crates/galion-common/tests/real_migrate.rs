// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the crates moved onto galion-common (wave 2 slot 28,
//! ports 19280-19289). It is gated twice, by `GALION_REAL=1` and by `#[ignore]`:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-common --test real_migrate \
//!     -- --ignored --nocapture
//! ```
//!
//! The test does all of the work itself; it replaced
//! `scripts/real/common-migrate-2.sh`.
//!
//! 1. It builds the migrated release binaries, and also the same packages
//!    at `GALION_REAL_OLD_REV` (default 26d98b0, which is swarm/common
//!    before this migration) in a throw-away git worktree. Nested builds
//!    go to `$CARGO_TARGET_DIR/real-new` and `real-old`, never to the
//!    directory the outer `cargo test` uses.
//! 2. Valid traffic, old vs new. hermes-gateway --mock, openmind-memory,
//!    galion-desk and room run side by side, each on its own synthetic
//!    state. The same requests go to both, and the raw reply bytes are
//!    compared. Fields that change from run to run (ids, clocks, pids) are
//!    masked, but only when the raw bytes differ. The client binaries run
//!    against a recording fixture: guardian-tick (openmind-survival),
//!    link-watch, galion-desk's Ollama calls and `openmind-memory parity`.
//!    Old and new must send the same request bytes and print the same
//!    output.
//! 3. Abuse. Chunked-upload abuse goes to both hermes builds. Then the
//!    galion-common hammer (`real_hammer.rs`: latency, malformed,
//!    oversized, slowloris) runs against all 8 servers, while memory and
//!    threads are sampled. Every process must still be alive at the end;
//!    all of them are killed on drop.
//!
//! Ports come from `GALION_REAL_PORT_BASE` (default 19280): base+0..7 are
//! the servers (old/new pairs), base+8 is the fixture, and base+9 is kept
//! closed. Nothing here touches a live service; all data is synthetic.

#[path = "real_hammer.rs"]
mod real_hammer;
mod realsupport;

use realsupport::{addr, now, Proc};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KEY: &str = "real-test-key-not-a-secret-0123";

struct Env {
    old: PathBuf,
    new: PathBuf,
    scratch: PathBuf,
    base: u16,
}

fn start(bin: &Path, args: &[&str], envs: &[(&str, String)], port: u16, log: &Path) -> Proc {
    let name = bin.file_name().unwrap_or_default().to_string_lossy();
    realsupport::start(&name, bin, args, envs, port, log)
}

/// Write `bytes`, read until the server closes (or 15 s).
fn exchange(port: u16, bytes: &[u8]) -> Vec<u8> {
    let Ok(mut s) = TcpStream::connect_timeout(&addr(port), Duration::from_secs(2)) else {
        return b"<connect failed>".to_vec();
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = s.write_all(bytes);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    out
}

/// Mask what legitimately differs between two runs: numbers after keys like
/// `"pid":` or `"..._ms":`, long digit runs (clocks, sizes), long ids with
/// digits in them, and the Content-Length that follows from those.
fn mask(b: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(b).into_owned();
    for key in [
        "\"pid\":",
        "\"ms\":",
        "_ms\":",
        "\"uptime_s\":",
        "\"age_s\":",
        "\"ts\":",
    ] {
        let mut res = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find(key) {
            res.push_str(&rest[..i + key.len()]);
            let after = rest[i + key.len()..].trim_start_matches(' ');
            let n = after
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(after.len());
            res.push_str("<v>");
            rest = &after[n..];
        }
        res.push_str(rest);
        s = res;
    }
    // Host load and memory that desk reports live.
    for key in ["\"load\":\"", "\"pod_ram\":\"", "\"laptop_ram\":\""] {
        let mut res = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find(key) {
            res.push_str(&rest[..i + key.len()]);
            let after = &rest[i + key.len()..];
            res.push_str("<v>");
            rest = &after[after.find('"').unwrap_or(after.len())..];
        }
        res.push_str(rest);
        s = res;
    }
    let mut out = String::with_capacity(s.len());
    let mut tok = String::new();
    let flush = |tok: &mut String, out: &mut String| {
        let digits = tok.chars().filter(|c| c.is_ascii_digit()).count();
        if tok.len() >= 12 && digits >= 3 {
            out.push_str("<id>");
        } else if tok.len() >= 4 && digits >= 3 && digits + 1 >= tok.len() {
            out.push_str("<n>");
        } else {
            out.push_str(tok);
        }
        tok.clear();
    };
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            tok.push(c);
        } else {
            flush(&mut tok, &mut out);
            out.push(c);
        }
    }
    flush(&mut tok, &mut out);
    out.split("\r\n")
        .map(|line| {
            if line.to_ascii_lowercase().starts_with("content-length:") {
                "content-length: <len>"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn show(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let s: String = s.chars().take(700).collect();
    s.replace('\r', "\\r").replace('\n', "\\n\n    ")
}

#[derive(Default)]
struct Tally {
    identical: usize,
    masked: usize,
    diff: Vec<String>,
}

impl Tally {
    fn compare(&mut self, what: &str, old: &[u8], new: &[u8]) {
        if old == new {
            self.identical += 1;
            println!("  same   {what} ({} bytes)", old.len());
        } else if mask(old) == mask(new) {
            self.masked += 1;
            println!(
                "  masked {what} (old {} / new {} bytes)",
                old.len(),
                new.len()
            );
        } else {
            let (mo, mn) = (mask(old), mask(new));
            let at = mo
                .bytes()
                .zip(mn.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(mo.len().min(mn.len()));
            let from = at.saturating_sub(120);
            let win = |s: &str| {
                s.get(from..(at + 200).min(s.len()))
                    .unwrap_or("")
                    .to_string()
            };
            println!(
                "  DIFF   {what} (masked, from byte {from})\n  old: {}\n  new: {}",
                show(win(&mo).as_bytes()),
                show(win(&mn).as_bytes())
            );
            self.diff.push(what.to_string());
        }
    }
}

fn get(path: &str, extra: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra}Connection: close\r\n\r\n")
        .into_bytes()
}

fn post(path: &str, extra: &str, body: &str) -> Vec<u8> {
    format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn chunked(path: &str, extra: &str, body: &str) -> Vec<u8> {
    let (a, b) = body.split_at(body.len() / 2);
    format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n{extra}\r\n{:x}\r\n{a}\r\n{:x};ext=1\r\n{b}\r\n0\r\n\r\n",
        a.len(),
        b.len()
    )
    .into_bytes()
}

fn hermes_requests() -> Vec<(&'static str, Vec<u8>)> {
    let auth = format!("Authorization: Bearer {KEY}\r\n");
    let chat = r#"{"model":"hermes-agent","messages":[{"role":"user","content":"hello from the real test"}]}"#;
    let chat_stream = r#"{"model":"hermes-agent","stream":true,"messages":[{"role":"user","content":"stream please"}]}"#;
    vec![
        ("GET /health", get("/health", "")),
        (
            "GET /v1/health HTTP/1.0 (Galion One)",
            b"GET /v1/health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n".to_vec(),
        ),
        ("GET /health/detailed", get("/health/detailed", &auth)),
        ("GET /v1/models", get("/v1/models", &auth)),
        ("GET /v1/capabilities", get("/v1/capabilities", &auth)),
        ("GET /v1/models no key (401)", get("/v1/models", "")),
        (
            "GET /v1/models wrong key (401)",
            get("/v1/models", "Authorization: Bearer wrong-key-0123456789\r\n"),
        ),
        ("POST /v1/chat/completions", post("/v1/chat/completions", &auth, chat)),
        (
            "POST /v1/chat/completions HTTP/1.0 (Galion One)",
            format!(
                "POST /v1/chat/completions HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}\r\n{chat}",
                chat.len()
            )
            .into_bytes(),
        ),
        (
            "POST /v1/chat/completions stream (SSE)",
            post("/v1/chat/completions", &auth, chat_stream),
        ),
        (
            "POST /v1/chat/completions chunked upload",
            chunked("/v1/chat/completions", &auth, chat),
        ),
        (
            "POST /v1/chat/completions bad JSON",
            post("/v1/chat/completions", &auth, "{not json"),
        ),
        ("GET /api/sessions", get("/api/sessions?limit=5", &auth)),
        ("GET /api/jobs", get("/api/jobs", &auth)),
        ("GET /api/spend", get("/api/spend", &auth)),
        ("GET /p/default/health", get("/p/default/health", "")),
        ("GET /p/other/health (404)", get("/p/other/health", "")),
        ("GET /nope (404)", get("/nope", &auth)),
        ("GET /v1/runs/missing", get("/v1/runs/missing", &auth)),
        (
            "HEAD /health",
            b"HEAD /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_vec(),
        ),
        (
            "LF-only GET /health",
            b"GET /health HTTP/1.1\nHost: 127.0.0.1\n\n".to_vec(),
        ),
        (
            "Expect 100-continue with body sent at once",
            post(
                "/v1/chat/completions",
                &format!("{auth}Expect: 100-continue\r\n"),
                chat,
            ),
        ),
    ]
}

fn memory_requests() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("GET /health", get("/health", "")),
        ("GET /health?x=1", get("/health?x=1", "")),
        (
            "GET absolute-form",
            b"GET http://127.0.0.1/health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_vec(),
        ),
        (
            "lower-case method",
            b"get /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_vec(),
        ),
        (
            "POST /capture",
            post(
                "/capture",
                "",
                r#"{"user_content":"synthetic: we sail on Sunday","assistant_content":"noted","session_key":"real-p"}"#,
            ),
        ),
        (
            "POST /recall",
            post("/recall", "", r#"{"query":"sail","session_key":"real-p"}"#),
        ),
        (
            "POST /search/memories",
            post("/search/memories", "", r#"{"query":"sail","limit":5}"#),
        ),
        (
            "POST /search/conversations",
            post(
                "/search/conversations",
                "",
                r#"{"query":"Sunday","limit":5}"#,
            ),
        ),
        (
            "POST /session/end",
            post("/session/end", "", r#"{"session_key":"real-p"}"#),
        ),
        ("POST /recall bad JSON", post("/recall", "", "{oops")),
        (
            "POST /recall chunked (refused, 400)",
            chunked("/recall", "", r#"{"query":"sail"}"#),
        ),
        (
            "POST /v2/pipeline/status",
            post("/v2/pipeline/status", "x-tdai-service-id: real\r\n", "{}"),
        ),
        ("GET /nope (404)", get("/nope", "")),
        ("GET /recall (wrong method)", get("/recall", "")),
    ]
}

fn desk_requests() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("GET /health", get("/health", "")),
        (
            "GET /health HTTP/1.0",
            b"GET /health HTTP/1.0\r\n\r\n".to_vec(),
        ),
        ("GET /desk", get("/desk", "")),
        ("GET /desk?x=1", get("/desk?x=1", "")),
        (
            "POST /desk note",
            post("/desk", "", r#"{"act":"note","text":"real test note"}"#),
        ),
        ("GET /desk after note", get("/desk", "")),
        ("POST /health", post("/health", "", "{}")),
        ("GET /nope (404)", get("/nope", "")),
        (
            "HEAD /health",
            b"HEAD /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_vec(),
        ),
        (
            "LF-only GET /desk",
            b"GET /desk HTTP/1.1\nHost: 127.0.0.1\n\n".to_vec(),
        ),
    ]
}

fn room_requests() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("GET /health", get("/health", "")),
        ("GET /", get("/", "")),
        ("GET /index.html", get("/index.html", "")),
        ("GET /api/objects", get("/api/objects", "")),
        (
            "GET /api/read held",
            get(
                "/api/read?path=%2Fworkspace%2Fgalion-light%2Fdocs%2Fnone.md",
                "",
            ),
        ),
        (
            "GET /api/read traversal",
            get("/api/read?path=/workspace/galion-src/../etc/passwd", ""),
        ),
        ("POST / (405)", post("/", "", "{}")),
        (
            "HEAD /health (405)",
            b"HEAD /health HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n".to_vec(),
        ),
        ("GET /nope (404)", get("/nope", "")),
        (
            "GET /health HTTP/1.0",
            b"GET /health HTTP/1.0\r\n\r\n".to_vec(),
        ),
    ]
}

// ------------------------------------------------------------ fixture

type Log = Arc<Mutex<Vec<Vec<u8>>>>;

fn fixture_reply(head: &str) -> Vec<u8> {
    let path = head.split(' ').nth(1).unwrap_or("/");
    let json = |code: &str, body: &str| {
        format!(
            "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    };
    let ps = r#"{"models":[{"name":"galion:latest","size_vram":4096,"context_length":8192}]}"#;
    let tags = r#"{"models":[{"name":"other:1b"},{"name":"galion:latest"}]}"#;
    let chunk = |body: &str| {
        let (a, b) = body.split_at(body.len() / 2);
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
            a.len(),
            b.len()
        )
        .into_bytes()
    };
    match path {
        "/api/ps" => json("200 OK", ps),
        "/api/tags" => json("200 OK", tags),
        "/chunk/api/ps" => chunk(r#"{"models":[]}"#),
        "/chunk/api/tags" => chunk(tags),
        "/chunk/health" | "/chunk/console" => chunk(r#"{"ok":true}"#),
        "/ok" | "/health" | "/console" => json("200 OK", r#"{"ok":true}"#),
        "/redirect" => {
            b"HTTP/1.1 302 Found\r\nLocation: /ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_vec()
        }
        "/missing" => json("404 Not Found", r#"{"error":"missing"}"#),
        "/bigheaders" => {
            let pad = "p".repeat(1000);
            format!("HTTP/1.1 200 OK\r\nX-Pad: {pad}\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").into_bytes()
        }
        "/noclose" => b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
        "/api/generate" => json("200 OK", r#"{"done":true}"#),
        "/recall" => json(
            "200 OK",
            r#"{"context":"fixture context","memory_count":1,"strategy":"keyword"}"#,
        ),
        "/search/memories" | "/search/conversations" => {
            json("200 OK", r#"{"results":"fixture results","total":1}"#)
        }
        _ => json("404 Not Found", r#"{"error":"no route"}"#),
    }
}

fn fixture(port: u16) -> Log {
    let log: Log = Arc::default();
    let l = TcpListener::bind(addr(port)).expect("fixture bind");
    let log2 = log.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = log2.clone();
            std::thread::spawn(move || serve_fixture(s, &log));
        }
    });
    log
}

fn serve_fixture(mut s: TcpStream, log: &Log) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let cl = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while buf.len() < head_end + cl {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    log.lock().unwrap().push(buf.clone());
    let reply = fixture_reply(&head);
    if head.starts_with("POST /api/generate ") {
        // Like a model load: long enough that desk's waking/resting flag is
        // reliably still set when it builds the reply, in old and new alike.
        std::thread::sleep(Duration::from_millis(300));
    }
    let _ = s.write_all(&reply);
    if head.starts_with("GET /noclose ") {
        // Keep the socket open past the client's deadline.
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn take(log: &Log) -> Vec<Vec<u8>> {
    let mut v = std::mem::take(&mut *log.lock().unwrap());
    v.sort();
    v
}

fn run(bin: &Path, args: &[&str], envs: &[(&str, String)]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .env("OPENMIND_CONFIG", "/nonexistent/openmind.toml");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("{}: {e}", bin.display()));
    (out.status.code().unwrap_or(-1), out.stdout, out.stderr)
}

fn compare_requests(t: &mut Tally, what: &str, old: &[Vec<u8>], new: &[Vec<u8>]) {
    if old.len() != new.len() {
        println!("  DIFF   {what}: {} vs {} requests", old.len(), new.len());
        t.diff.push(what.to_string());
        return;
    }
    for (i, (o, n)) in old.iter().zip(new).enumerate() {
        // Request bytes must be byte-identical; nothing is masked here.
        if o == n {
            t.identical += 1;
            println!("  same   {what} request {} ({} bytes)", i + 1, o.len());
        } else {
            println!(
                "  DIFF   {what} request {}\n  old: {}\n  new: {}",
                i + 1,
                show(o),
                show(n)
            );
            t.diff.push(format!("{what} request {}", i + 1));
        }
    }
}

/// Phase 2: valid traffic and client captures, old vs new.
fn valid_traffic(e: &Env) {
    let b = e.base;
    let sc = |p: &str| e.scratch.join(p);
    for d in [
        "hermes-old",
        "hermes-new",
        "memory-old",
        "memory-new",
        "desk-old/state",
        "desk-new/state",
        "desk-old/one",
        "desk-new/one",
        "lw-old",
        "lw-new",
    ] {
        std::fs::create_dir_all(sc(d)).unwrap();
    }
    let fix = fixture(b + 8);
    let mut t = Tally::default();

    // ------------------------------------------------------------ servers
    let hermes = |dir: &Path, home: &str, port: u16| {
        start(
            &dir.join("hermes-gateway"),
            &["--mock"],
            &[
                ("HERMES_HOME", sc(home).display().to_string()),
                ("HERMES_RS_HOST", "127.0.0.1".into()),
                ("HERMES_RS_PORT", port.to_string()),
                ("HERMES_RS_MCP", "0".into()),
                ("API_SERVER_KEY", KEY.into()),
            ],
            port,
            &sc(&format!("{home}.log")),
        )
    };
    let memory = |dir: &Path, data: &str, port: u16| {
        start(
            &dir.join("openmind-memory"),
            &[],
            &[
                ("OPENMIND_MEMORY_DATA_DIR", sc(data).display().to_string()),
                ("OPENMIND_MEMORY_PORT", port.to_string()),
            ],
            port,
            &sc(&format!("{data}.log")),
        )
    };
    let desk = |dir: &Path, root: &str, port: u16| {
        std::fs::write(sc(&format!("{root}/one/voice")), "jarvis\n").unwrap();
        start(
            &dir.join("galion-desk"),
            &[],
            &[
                ("DESK_BIND", format!("127.0.0.1:{port}")),
                (
                    "DESK_STATE_DIR",
                    sc(&format!("{root}/state")).display().to_string(),
                ),
                (
                    "DESK_ONE_DIR",
                    sc(&format!("{root}/one")).display().to_string(),
                ),
                (
                    "DESK_HERMES_DIR",
                    sc(&format!("{root}/hermes")).display().to_string(),
                ),
                ("DESK_OLLAMA", format!("127.0.0.1:{}", b + 8)),
                ("DESK_HERMES_BIN", "/bin/false".into()),
            ],
            port,
            &sc(&format!("{root}.log")),
        )
    };
    let room = |dir: &Path, port: u16, name: &str| {
        start(
            &dir.join("room"),
            &["--listen"],
            &[("ROOM_BIND", format!("127.0.0.1:{port}"))],
            port,
            &sc(&format!("{name}.log")),
        )
    };

    let mut procs = vec![
        hermes(&e.old, "hermes-old", b),
        hermes(&e.new, "hermes-new", b + 1),
        memory(&e.old, "memory-old", b + 2),
        memory(&e.new, "memory-new", b + 3),
        desk(&e.old, "desk-old", b + 4),
        desk(&e.new, "desk-new", b + 5),
        room(&e.old, b + 6, "room-old"),
        room(&e.new, b + 7, "room-new"),
    ];

    type Reqs = fn() -> Vec<(&'static str, Vec<u8>)>;
    let services: [(&str, u16, Reqs); 4] = [
        ("hermes", b, hermes_requests),
        ("memory", b + 2, memory_requests),
        ("desk", b + 4, desk_requests),
        ("room", b + 6, room_requests),
    ];
    for (name, old_port, reqs) in services {
        println!(
            "== {name}: old 127.0.0.1:{old_port} vs new 127.0.0.1:{}",
            old_port + 1
        );
        for (label, bytes) in reqs() {
            let o = exchange(old_port, &bytes);
            let old_calls = take(&fix);
            let n = exchange(old_port + 1, &bytes);
            let new_calls = take(&fix);
            t.compare(&format!("{name} {label}"), &o, &n);
            if !old_calls.is_empty() || !new_calls.is_empty() {
                compare_requests(
                    &mut t,
                    &format!("{name} {label} -> fixture"),
                    &old_calls,
                    &new_calls,
                );
            }
        }
    }
    // desk wake/rest go to Ollama from a background thread.
    for act in ["wake", "rest"] {
        let body = format!(r#"{{"act":"{act}"}}"#);
        let o = exchange(b + 4, &post("/desk", "", &body));
        std::thread::sleep(Duration::from_millis(600));
        let old_calls = take(&fix);
        let n = exchange(b + 5, &post("/desk", "", &body));
        std::thread::sleep(Duration::from_millis(600));
        let new_calls = take(&fix);
        t.compare(&format!("desk POST /desk {act}"), &o, &n);
        compare_requests(
            &mut t,
            &format!("desk {act} -> Ollama fixture"),
            &old_calls,
            &new_calls,
        );
    }

    // ------------------------------------------------------------ clients
    println!("== guardian-tick (openmind-survival check_http)");
    let probes = format!(
        "[[probe]]\nkind = \"http\"\nname = \"ok\"\nport = {f}\npath = \"/ok\"\n\n\
         [[probe]]\nkind = \"http\"\nname = \"redirect\"\nport = {f}\npath = \"/redirect\"\n\n\
         [[probe]]\nkind = \"http\"\nname = \"missing\"\nport = {f}\npath = \"/missing\"\n\n\
         [[probe]]\nkind = \"http\"\nname = \"big-headers\"\nport = {f}\npath = \"/bigheaders\"\n\n\
         [[probe]]\nkind = \"http\"\nname = \"chunked\"\nport = {f}\npath = \"/chunk/health\"\n\n\
         [[probe]]\nkind = \"http\"\nname = \"closed\"\nport = {c}\npath = \"/\"\n",
        f = b + 8,
        c = b + 9
    );
    std::fs::write(sc("probes.toml"), probes).unwrap();
    let mut outs = Vec::new();
    for (tag, dir) in [("old", &e.old), ("new", &e.new)] {
        let out = sc(&format!("guardian-{tag}.json"));
        let r = run(
            &dir.join("guardian-tick"),
            &[
                "--probes",
                &sc("probes.toml").display().to_string(),
                "--out",
                &out.display().to_string(),
            ],
            &[],
        );
        outs.push((r, std::fs::read(&out).unwrap_or_default(), take(&fix)));
    }
    let ((ro, fo, qo), (rn, fn_, qn)) = (&outs[0], &outs[1]);
    println!("  exit old {} new {}", ro.0, rn.0);
    t.compare("guardian-tick report", fo, fn_);
    t.compare("guardian-tick stdout", &ro.1, &rn.1);
    compare_requests(&mut t, "guardian-tick", qo, qn);

    println!("== link-watch (LINK_WATCH_ONCE)");
    for (scenario, prefix) in [("plain", ""), ("chunked", "/chunk")] {
        let mut outs = Vec::new();
        for (tag, dir) in [("old", &e.old), ("new", &e.new)] {
            let d = sc(&format!("lw-{tag}"));
            let base = format!("http://127.0.0.1:{}{prefix}", b + 8);
            let r = run(
                &dir.join("link-watch"),
                &[],
                &[
                    ("LINK_WATCH_ONCE", "1".into()),
                    ("LINK_WATCH_OUT", d.join("link.json").display().to_string()),
                    (
                        "LINK_WATCH_LOG",
                        d.join("link-log.json").display().to_string(),
                    ),
                    (
                        "LINK_WATCH_LAPTOP",
                        d.join("laptop.json").display().to_string(),
                    ),
                    ("LINK_WATCH_OLLAMA_URL", base.clone()),
                    ("LINK_WATCH_HERMES_URL", base.clone()),
                    ("LINK_WATCH_FRANCE_URL", base),
                ],
            );
            outs.push((
                r,
                std::fs::read(d.join("link.json")).unwrap_or_default(),
                take(&fix),
            ));
        }
        let ((ro, fo, qo), (rn, fn_, qn)) = (&outs[0], &outs[1]);
        println!("  {scenario}: exit old {} new {}", ro.0, rn.0);
        t.compare(&format!("link-watch {scenario} link.json"), fo, fn_);
        compare_requests(&mut t, &format!("link-watch {scenario}"), qo, qn);
    }

    println!("== openmind-memory parity (post_json client)");
    std::fs::write(sc("queries.txt"), "sail\nSunday\n").unwrap();
    let fixture_base = format!("http://127.0.0.1:{}", b + 8);
    let mut outs = Vec::new();
    for (tag, dir) in [("old", &e.old), ("new", &e.new)] {
        let r = run(
            &dir.join("openmind-memory"),
            &[
                "parity",
                &fixture_base,
                &fixture_base,
                &sc("queries.txt").display().to_string(),
                "real-p",
            ],
            &[],
        );
        let reqs = take(&fix);
        // And the real thing: old server vs new server, through this binary.
        let live = run(
            &dir.join("openmind-memory"),
            &[
                "parity",
                &format!("http://127.0.0.1:{}", b + 2),
                &format!("http://127.0.0.1:{}", b + 3),
                &sc("queries.txt").display().to_string(),
                "real-p",
            ],
            &[],
        );
        println!(
            "  {tag} parity old-server vs new-server: exit {} {}",
            live.0,
            String::from_utf8_lossy(&live.1)
                .lines()
                .last()
                .unwrap_or("")
        );
        outs.push((r, reqs, live.1));
    }
    let ((ro, qo, lo), (rn, qn, ln)) = (&outs[0], &outs[1]);
    t.compare("memory parity stdout (fixture)", &ro.1, &rn.1);
    // Old and new servers hold the same synthetic data but stamp it with
    // their own clocks, so parity between them is informational; the two
    // parity binaries must print the same verdicts though.
    t.compare("memory parity stdout (old vs new server)", lo, ln);
    compare_requests(&mut t, "memory parity", qo, qn);

    for p in procs.iter_mut() {
        if let Ok(Some(st)) = p.child.try_wait() {
            println!("  a server exited early: {st}");
            t.diff.push("server died".into());
        }
    }
    drop(procs);
    println!(
        "== summary: {} byte-identical, {} identical after masking, {} different",
        t.identical,
        t.masked,
        t.diff.len()
    );
    for d in &t.diff {
        println!("   different: {d}");
    }
    assert!(t.diff.is_empty(), "old and new differ: {:?}", t.diff);
}

/// Chunked-upload abuse for servers that accept chunked bodies (hermes).
/// Each case prints the status, or how the connection ended. The server
/// must answer `/health` afterwards.
fn chunked_abuse(targets: &[(String, SocketAddr)]) {
    let head = |extra: &str| {
        format!("POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\n{extra}\r\n")
    };
    let te = head("Transfer-Encoding: chunked\r\n");
    let mut nine_mb = te.clone().into_bytes();
    for _ in 0..9 {
        nine_mb.extend_from_slice(b"100000\r\n");
        nine_mb.extend(std::iter::repeat_n(b'x', 0x10_0000));
        nine_mb.extend_from_slice(b"\r\n");
    }
    nine_mb.extend_from_slice(b"0\r\n\r\n");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "size ffffffffffffffff",
            format!("{te}ffffffffffffffff\r\n").into_bytes(),
        ),
        (
            "size line 2000 bytes",
            format!("{te}{}\r\n", "1".repeat(2000)).into_bytes(),
        ),
        (
            "non-hex size",
            format!("{te}zz\r\nab\r\n0\r\n\r\n").into_bytes(),
        ),
        ("negative size", format!("{te}-1\r\n\r\n").into_bytes()),
        (
            "missing CRLF after data",
            format!("{te}2\r\nabXY0\r\n\r\n").into_bytes(),
        ),
        (
            "trailer 20 KB",
            format!("{te}0\r\nX-T: {}\r\n\r\n", "t".repeat(20_000)).into_bytes(),
        ),
        ("9 MiB chunked body", nine_mb),
        (
            "TE gzip",
            head("Transfer-Encoding: gzip\r\nContent-Length: 2\r\n").into_bytes(),
        ),
        (
            "TE chunked + CL",
            head("Transfer-Encoding: chunked\r\nContent-Length: 5\r\n").into_bytes(),
        ),
        (
            "TE twice",
            head("Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n").into_bytes(),
        ),
        ("truncated chunked", format!("{te}10\r\nabc").into_bytes()),
    ];
    for (name, a) in targets {
        let a = *a;
        for (label, bytes) in &cases {
            let t = Instant::now();
            let reply = (|| {
                let mut s = TcpStream::connect_timeout(&a, Duration::from_secs(2)).ok()?;
                let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(20)));
                let _ = s.write_all(bytes);
                let _ = s.shutdown(std::net::Shutdown::Write);
                let mut out = Vec::new();
                let _ = s.read_to_end(&mut out);
                Some(out)
            })();
            let what = match reply {
                None => "connect failed".to_string(),
                Some(o) if o.is_empty() => "closed, no reply".to_string(),
                Some(o) => String::from_utf8_lossy(&o[..o.len().min(12)]).into_owned(),
            };
            println!(
                "{name} chunked {label}: {what} in {:.1}ms",
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        // Slow drip: one byte of a chunked body every 300 ms for 4 s.
        let t = Instant::now();
        let drip = (|| {
            let mut s = TcpStream::connect_timeout(&a, Duration::from_secs(2)).ok()?;
            s.write_all(te.as_bytes()).ok()?;
            s.write_all(b"400\r\n").ok()?;
            for _ in 0..13 {
                if s.write_all(b"x").is_err() {
                    return Some("write cut".to_string());
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            Some("dripped 4 s, client gave up".to_string())
        })();
        println!(
            "{name} chunked slow drip: {drip:?} after {:.1}s",
            t.elapsed().as_secs_f64()
        );
        let ok = exchange(a.port(), &get("/health", ""));
        println!(
            "{name} after chunked abuse: /health -> {}",
            String::from_utf8_lossy(&ok[..ok.len().min(12)])
        );
        assert!(
            ok.starts_with(b"HTTP/1.1 200"),
            "{name} not healthy after abuse"
        );
    }
}

/// The packages built for the test (each has a binary of the same name).
const PACKAGES: [&str; 6] = [
    "hermes-gateway",
    "openmind-memory",
    "galion-desk",
    "room",
    "link-watch",
    "guardian-tick",
];

#[test]
#[ignore = "real-life test: GALION_REAL=1, builds and runs release binaries on 19280-19289"]
fn real_migrate() {
    if !realsupport::enabled("real_migrate") {
        return;
    }
    let base = realsupport::port_base(19280);
    let rev = std::env::var("GALION_REAL_OLD_REV")
        .unwrap_or_else(|_| "26d98b055bd980045e718e3ab63a0d26888f0256".into());
    let scratch = realsupport::Scratch::new("common-migrate-2-real");

    // ------------------------------------------------------------ 1. build
    println!("== {} build migrated tree and {rev}", now());
    let new = realsupport::build(
        &realsupport::workspace(),
        &realsupport::target_dir("new"),
        &PACKAGES,
    );
    let old = realsupport::build_old(&rev, &scratch, &PACKAGES, &PACKAGES);

    // ------------------------------------------- 2. valid traffic, old vs new
    std::fs::create_dir_all(scratch.join("diff")).unwrap();
    println!("== {} valid traffic, old vs new", now());
    valid_traffic(&Env {
        old: old.clone(),
        new: new.clone(),
        scratch: scratch.join("diff"),
        base,
    });

    // ------------------------------------------------------------ 3. abuse
    let mut specs: Vec<Spec> = Vec::new();
    for (side, dir, o) in [("new", &new, 1u16), ("old", &old, 0)] {
        let (hp, mp, dp, rp) = (base + o, base + 2 + o, base + 4 + o, base + 6 + o);
        let d = |p: &str| scratch.join(&format!("{side}/{p}"));
        for p in ["hermes", "memory", "desk/state", "desk/one"] {
            std::fs::create_dir_all(d(p)).unwrap();
        }
        let s = |p: PathBuf| p.display().to_string();
        let spec = |name: &str, bin: &str, prefix, args: &[&'static str], envs| Spec {
            name: format!("{side}-{name}"),
            bin: dir.join(bin),
            prefix,
            args: args.to_vec(),
            envs,
            port: 0,
        };
        specs.push(Spec {
            port: hp,
            ..spec(
                "hermes",
                "hermes-gateway",
                "HERMES_RS",
                &["--mock"],
                vec![
                    ("HERMES_HOME", s(d("hermes"))),
                    ("HERMES_RS_HOST", "127.0.0.1".into()),
                    ("HERMES_RS_PORT", hp.to_string()),
                    ("HERMES_RS_MCP", "0".into()),
                    ("API_SERVER_KEY", KEY.into()),
                ],
            )
        });
        specs.push(Spec {
            port: mp,
            ..spec(
                "memory",
                "openmind-memory",
                "OPENMIND_MEMORY",
                &[],
                vec![
                    ("OPENMIND_MEMORY_DATA_DIR", s(d("memory"))),
                    ("OPENMIND_MEMORY_PORT", mp.to_string()),
                ],
            )
        });
        specs.push(Spec {
            port: dp,
            ..spec(
                "desk",
                "galion-desk",
                "DESK",
                &[],
                vec![
                    ("DESK_BIND", format!("127.0.0.1:{dp}")),
                    ("DESK_STATE_DIR", s(d("desk/state"))),
                    ("DESK_ONE_DIR", s(d("desk/one"))),
                    ("DESK_HERMES_DIR", s(d("desk/hermes"))),
                    ("DESK_OLLAMA", format!("127.0.0.1:{}", base + 9)),
                    ("DESK_HERMES_BIN", "/bin/false".into()),
                ],
            )
        });
        specs.push(Spec {
            port: rp,
            ..spec(
                "room",
                "room",
                "ROOM",
                &["--listen"],
                vec![("ROOM_BIND", format!("127.0.0.1:{rp}"))],
            )
        });
    }
    let mut procs: Vec<Proc> = specs.iter().map(|s| s.start(&scratch, &[])).collect();
    let targets: Vec<(String, SocketAddr)> = specs
        .iter()
        .map(|s| (s.name.clone(), addr(s.port)))
        .collect();
    println!("== {} abuse", now());
    realsupport::mem("before", &procs);
    let sampler = realsupport::Sampler::start(&procs);
    let hermes: Vec<_> = targets
        .iter()
        .filter(|(n, _)| n.ends_with("-hermes"))
        .cloned()
        .collect();
    chunked_abuse(&hermes);
    for (name, a) in &targets {
        real_hammer::hammer(name, *a);
    }
    std::thread::sleep(Duration::from_secs(1));
    println!("== {} done", now());
    realsupport::mem("after", &procs);
    sampler.finish();
    let dead = realsupport::alive_report(&mut procs);
    assert!(dead.is_empty(), "died under abuse: {dead:?}");
    drop(procs);

    // ------------------------------------------- 4. limits from environment
    // Every migrated server takes <PREFIX>_HTTP_* overrides, so a stricter
    // limit can be relaxed (or a revert approximated) without a rebuild.
    // Default vs override, measured on the migrated binaries.
    println!("== {} limits from environment (migrated binaries)", now());
    for spec in specs.iter().filter(|s| s.name.starts_with("new-")) {
        let (head_default, _) = {
            let _p = spec.start(&scratch, &[]);
            slow_head(spec.port)
        };
        let head_ms = format!("{}_HTTP_HEAD_TIMEOUT_MS", spec.prefix);
        let conns = format!("{}_HTTP_MAX_CONNECTIONS", spec.prefix);
        let _p = spec.start(&scratch, &[(&head_ms, "1000".into()), (&conns, "2".into())]);
        let (head_override, reply) = slow_head(spec.port);
        // Let the server notice that client close and free its slot first.
        std::thread::sleep(Duration::from_millis(300));
        let busy = over_cap(spec.port, 2);
        println!(
            "{} stalled head closed after {:.1}s by default, {:.1}s with {head_ms}=1000 ({reply}); {conns}=2: 3rd connection -> {busy}",
            spec.name,
            head_default.as_secs_f64(),
            head_override.as_secs_f64()
        );
        assert!(
            head_override < Duration::from_millis(2500),
            "{head_ms} ignored"
        );
        assert!(
            head_default > Duration::from_millis(2500),
            "default head deadline"
        );
        assert!(busy.contains("503"), "{conns} ignored: {busy}");
    }
}

/// How to start one server (the abuse and environment phases share it).
struct Spec {
    name: String,
    bin: PathBuf,
    /// Environment prefix for `Limits::with_env` (`<PREFIX>_HTTP_*`).
    prefix: &'static str,
    args: Vec<&'static str>,
    envs: Vec<(&'static str, String)>,
    port: u16,
}

impl Spec {
    fn start(&self, scratch: &realsupport::Scratch, extra: &[(&str, String)]) -> Proc {
        let mut envs: Vec<(&str, String)> =
            self.envs.iter().map(|(k, v)| (*k, v.clone())).collect();
        envs.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
        let log = scratch.join(&format!("{}.log", self.name));
        realsupport::start(&self.name, &self.bin, &self.args, &envs, self.port, &log)
    }
}

/// Send half a request head and wait for the server to give up; returns the
/// time it took and the first bytes of the reply.
fn slow_head(port: u16) -> (Duration, String) {
    let t = Instant::now();
    let Ok(mut s) = TcpStream::connect_timeout(&addr(port), Duration::from_secs(2)) else {
        return (t.elapsed(), "connect failed".into());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = s.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\n");
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let reply = if out.is_empty() {
        "closed".to_string()
    } else {
        String::from_utf8_lossy(&out[..out.len().min(12)]).into_owned()
    };
    (t.elapsed(), reply)
}

/// Hold `held` stalled connections, then send a full GET on one more.
fn over_cap(port: u16, held: usize) -> String {
    let hold: Vec<TcpStream> = (0..held)
        .filter_map(|_| {
            let mut s = TcpStream::connect_timeout(&addr(port), Duration::from_secs(2)).ok()?;
            let _ = s.write_all(b"GET /health HTTP/1.1\r\n");
            Some(s)
        })
        .collect();
    std::thread::sleep(Duration::from_millis(200));
    let early: Vec<String> = hold
        .iter()
        .map(|s| {
            let _ = s.set_read_timeout(Some(Duration::from_millis(50)));
            let mut b = [0u8; 12];
            match s.peek(&mut b) {
                Ok(0) => "closed".to_string(),
                Ok(n) => String::from_utf8_lossy(&b[..n]).into_owned(),
                Err(_) => "held".to_string(),
            }
        })
        .collect();
    let r = exchange(port, &get("/health", ""));
    drop(hold);
    format!(
        "{} (held: {early:?})",
        String::from_utf8_lossy(&r[..r.len().min(12)])
    )
}
