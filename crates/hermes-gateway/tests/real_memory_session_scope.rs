// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 426 hermes-memory-session-scope: real-life test that memory recall
//! is scoped to the memory key (session_key, else the session id) unless
//! cross-session recall is opted in. Gated: `GALION_REAL=1`. All on
//! 127.0.0.1 with throwaway dirs; `--mock` provider (no network, no spend).
//!
//! | port  | process |
//! |-------|---------|
//! | 22965 | real release `openmind-memory` of this branch |
//! | 22966 | `hermes-gateway --mock`, memory on, default (session scope) |
//! | 22967 | `hermes-gateway --mock`, memory on, `HERMES_RS_MEMORY_CROSS_SESSION=1` |
//! | 22968 | older `openmind-memory` (`HMS_OLD_MEMORY_BIN`, e.g. 8887a0cb) |
//! | 22969 | `hermes-gateway --mock`, default scope, against the older server |
//!
//! Binaries: `HMS_GATEWAY_BIN` / `HMS_MEMORY_BIN` (default: this crate's
//! release `hermes-gateway` and the sibling `openmind-memory`; build with
//! `cargo build --release -p openmind-memory -p hermes-gateway`). Step 6
//! needs `HMS_OLD_MEMORY_BIN`; `HMS_REQUIRE_OLD=1` makes it mandatory.
//! Port base: `HMS_PORT_BASE` (default 22965).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const API_KEY: &str = "hms-gateway-key-0123456789";
const MEM_KEY: &str = "hms-memory-key-abcdef-426";
const WORD_A: &str = "heliotrope-22965";
const WORD_B: &str = "vermilion-22966";
const WORD_U: &str = "cerulean-22967";
const FENCE: &str = "<memory-context>";

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1");
    }
    on
}

fn base() -> u16 {
    std::env::var("HMS_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22965)
}

fn gateway_bin() -> PathBuf {
    std::env::var_os("HMS_GATEWAY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn memory_bin() -> PathBuf {
    let p = std::env::var_os("HMS_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")).with_file_name("openmind-memory")
        });
    assert!(
        p.exists(),
        "{} missing: cargo build --release -p openmind-memory (same CARGO_TARGET_DIR)",
        p.display()
    );
    p
}

fn zagreb_now() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S %:z")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

struct Proc(Option<Child>);
impl Drop for Proc {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// (status, lower-cased head, body)
fn http(
    port: u16,
    method: &str,
    path: &str,
    hdrs: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut req =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    for (k, v) in hdrs {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    ));
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (status, head.to_ascii_lowercase(), rest.to_string())
}

fn wait_health(port: u16, child: &mut Child) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(15) {
        if let Ok(Some(st)) = child.try_wait() {
            panic!("process on {port} exited early: {st}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok()
            && http(port, "GET", "/health", &[], None).0 == 200
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("no /health 200 on {port}");
}

fn start_memory(bin: &Path, port: u16, dir: &Path) -> Proc {
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "port {port} busy"
    );
    let mut c = Command::new(bin)
        .env_clear()
        .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", port.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .env("OPENMIND_MEMORY_INSTANCE_ID", "galion")
        .env("OPENMIND_MEMORY_API_KEY", MEM_KEY)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openmind-memory");
    wait_health(port, &mut c);
    Proc(Some(c))
}

fn start_gateway(port: u16, home: &Path, mem_port: u16, cross: bool) -> Proc {
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "port {port} busy"
    );
    std::fs::create_dir_all(home).unwrap();
    let log = std::fs::File::create(home.join("gateway.log")).unwrap();
    let mut c = Command::new(gateway_bin());
    c.arg("--mock")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .env("API_SERVER_KEY", API_KEY)
        .env("HERMES_RS_MEMORY", "1")
        .env(
            "HERMES_RS_MEMORY_URL",
            format!("http://127.0.0.1:{mem_port}"),
        )
        .env("TDAI_GATEWAY_API_KEY", MEM_KEY)
        .env("HERMES_RS_MEMORY_RECALL_MS", "1500");
    if cross {
        c.env("HERMES_RS_MEMORY_CROSS_SESSION", "1");
    }
    let mut c = c
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .expect("spawn hermes-gateway");
    wait_health(port, &mut c);
    Proc(Some(c))
}

/// One non-streaming chat. `key`/`sid` become X-Hermes-Session-Key/-Id.
/// Returns (status, assistant text, X-Hermes-Session-Id of the reply).
fn chat(port: u16, key: Option<&str>, sid: Option<&str>, text: &str) -> (u16, String, String) {
    let auth = format!("Bearer {API_KEY}");
    let mut h = vec![("Authorization", auth.as_str())];
    if let Some(k) = key {
        h.push(("X-Hermes-Session-Key", k));
    }
    if let Some(s) = sid {
        h.push(("X-Hermes-Session-Id", s));
    }
    let (s, head, body) = http(
        port,
        "POST",
        "/v1/chat/completions",
        &h,
        Some(&json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]})),
    );
    let content = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v["choices"][0]["message"]["content"]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_default();
    let sid = head
        .lines()
        .find_map(|l| l.strip_prefix("x-hermes-session-id:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    (s, content, sid)
}

fn detailed(port: u16) -> Value {
    let auth = format!("Bearer {API_KEY}");
    let (s, _, b) = http(
        port,
        "GET",
        "/health/detailed",
        &[("Authorization", &auth)],
        None,
    );
    assert_eq!(s, 200);
    serde_json::from_str(&b).unwrap()
}

fn wait_capture_ok(port: u16, n: u64) -> Value {
    let t0 = Instant::now();
    loop {
        let d = detailed(port);
        if d["memory"]["capture_ok"].as_u64().unwrap_or(0) >= n
            || t0.elapsed() > Duration::from_secs(10)
        {
            assert!(d["memory"]["capture_ok"].as_u64().unwrap_or(0) >= n, "{d}");
            return d;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn recalled(reply: &str, fact: &str) -> bool {
    reply.contains(FENCE) && reply.contains(fact)
}

#[test]
fn real_memory_session_scope() {
    if !real() {
        return;
    }
    let b = base();
    let (p_mem, p_gw, p_cross, p_old, p_gw_old) = (b, b + 1, b + 2, b + 3, b + 4);
    let tmp = std::env::temp_dir().join(format!("hms426-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let memdir = tmp.join("mem");
    eprintln!(
        "[{}] start openmind-memory :{p_mem} ({})",
        zagreb_now(),
        memory_bin().display()
    );
    let _mem = start_memory(&memory_bin(), p_mem, &memdir);
    let gw = start_gateway(p_gw, &tmp.join("gw"), p_mem, false);

    let fact_a = format!("my locker code word is {WORD_A}");
    let fact_b = format!("my bike lock word is {WORD_B}");
    let fact_u = format!("my unkeyed note word is {WORD_U}");

    // 1. Session A states a fact; session B states another.
    assert_eq!(chat(p_gw, Some("s426-a"), None, &fact_a).0, 200);
    wait_capture_ok(p_gw, 1);
    assert_eq!(chat(p_gw, Some("s426-b"), None, &fact_b).0, 200);
    wait_capture_ok(p_gw, 2);

    // 2. Own session recalls its own fact.
    let (s, a2, _) = chat(p_gw, Some("s426-a"), None, WORD_A);
    assert_eq!(s, 200);
    assert!(recalled(&a2, &fact_a), "A recalls A: {a2}");
    assert!(!a2.contains(WORD_B), "A must not see B: {a2}");
    let (_, b2, _) = chat(p_gw, Some("s426-b"), None, WORD_B);
    assert!(recalled(&b2, &fact_b), "B recalls B: {b2}");
    assert!(!b2.contains(WORD_A), "B must not see A: {b2}");

    // 3. No leak either way: B asks for A's word, A asks for B's word.
    let (s, b3, _) = chat(p_gw, Some("s426-b"), None, WORD_A);
    assert_eq!(s, 200);
    assert!(!b3.contains(FENCE), "LEAK A->B: {b3}");
    let (_, a3, _) = chat(p_gw, Some("s426-a"), None, WORD_B);
    assert!(!a3.contains(FENCE), "LEAK B->A: {a3}");
    // a brand-new key sees nothing either.
    let (_, c3, _) = chat(p_gw, Some("s426-c"), None, WORD_A);
    assert!(!c3.contains(FENCE), "LEAK A->C: {c3}");
    eprintln!(
        "[{}] scoped gateway: own-session recall ok, 0 leaks in 3 cross probes",
        zagreb_now()
    );
    // (checked after the leak probes so an older gateway fails on the leak)
    let d = detailed(p_gw);
    assert_eq!(d["memory"]["scope"], "session", "default scope: {d}");
    assert_eq!(d["memory"]["recall_unscoped"], 0, "{d}");

    // 4. No session key: the memory key is the session id (D-403-4).
    let (s, _, sid_u) = chat(p_gw, None, None, &fact_u);
    assert_eq!(s, 200);
    assert!(sid_u.starts_with("api-"), "session id header: {sid_u:?}");
    wait_capture_ok(p_gw, 8);
    let (_, u2, sid_u2) = chat(p_gw, None, None, WORD_U);
    assert_ne!(sid_u2, sid_u, "unkeyed chat opens a new session");
    assert!(
        !u2.contains(FENCE),
        "LEAK unkeyed session -> new session: {u2}"
    );
    let (_, u3, sid_u3) = chat(p_gw, None, Some(&sid_u), WORD_U);
    assert_eq!(sid_u3, sid_u);
    assert!(recalled(&u3, &fact_u), "same session id recalls: {u3}");
    let (_, a4, _) = chat(p_gw, Some("s426-a"), None, WORD_U);
    assert!(!a4.contains(FENCE), "LEAK unkeyed -> A: {a4}");

    // store side: captures are keyed by session_key / session id.
    let mut keys = std::collections::BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(memdir.join("conversations")) {
        for e in rd.flatten() {
            for l in std::fs::read_to_string(e.path())
                .unwrap_or_default()
                .lines()
            {
                if let Ok(v) = serde_json::from_str::<Value>(l) {
                    keys.insert(v["sessionKey"].as_str().unwrap_or("").to_string());
                }
            }
        }
    }
    assert!(
        keys.contains("s426-a") && keys.contains("s426-b") && keys.contains(&sid_u),
        "{keys:?}"
    );
    assert!(keys.iter().all(|k| !k.is_empty()), "{keys:?}");
    let d = detailed(p_gw);
    assert_eq!(d["memory"]["recall_err"], 0, "{d}");
    assert_eq!(d["memory"]["recall_unscoped"], 0, "{d}");
    eprintln!(
        "[{}] scoped gateway stats {} ; store keys {:?}",
        zagreb_now(),
        d["memory"],
        keys
    );

    // 5. Explicit opt-in: cross-session recall works and says so.
    let cross = start_gateway(p_cross, &tmp.join("gw-cross"), p_mem, true);
    let d = detailed(p_cross);
    assert_eq!(d["memory"]["scope"], "all", "{d}");
    let (s, x5, _) = chat(p_cross, Some("s426-x"), None, WORD_A);
    assert_eq!(s, 200);
    assert!(recalled(&x5, &fact_a), "opt-in crosses sessions: {x5}");
    drop(cross);

    // 6. Older server (ignores `scope`): the default client fails closed.
    match std::env::var_os("HMS_OLD_MEMORY_BIN").map(PathBuf::from) {
        Some(old_bin) => {
            let olddir = tmp.join("mem-old");
            let _old = start_memory(&old_bin, p_old, &olddir);
            let g = start_gateway(p_gw_old, &tmp.join("gw-old"), p_old, false);
            assert_eq!(chat(p_gw_old, Some("s426-a"), None, &fact_a).0, 200);
            wait_capture_ok(p_gw_old, 1);
            // prove the old server itself would leak: raw /recall from B.
            let auth = format!("Bearer {MEM_KEY}");
            let (s, _, raw) = http(
                p_old,
                "POST",
                "/recall",
                &[("Authorization", &auth)],
                Some(&json!({"query": WORD_A, "session_key": "s426-b", "scope": "session"})),
            );
            assert_eq!(s, 200);
            let raw: Value = serde_json::from_str(&raw).unwrap();
            assert!(
                raw["context"].as_str().unwrap_or("").contains(WORD_A)
                    && raw.get("scope").is_none(),
                "old server leaks and does not echo scope: {raw}"
            );
            let (_, o6, _) = chat(p_gw_old, Some("s426-b"), None, WORD_A);
            assert!(!o6.contains(FENCE), "client must drop unscoped reply: {o6}");
            let d = detailed(p_gw_old);
            assert!(d["memory"]["recall_unscoped"].as_u64().unwrap() >= 1, "{d}");
            assert_eq!(d["memory"]["recall_hit"], 0, "{d}");
            eprintln!(
                "[{}] old server: raw /recall leaks to B; client dropped it, stats {}",
                zagreb_now(),
                d["memory"]
            );
            drop(g);
        }
        None => {
            assert!(
                std::env::var("HMS_REQUIRE_OLD").as_deref() != Ok("1"),
                "HMS_OLD_MEMORY_BIN required"
            );
            eprintln!("step 6 not run: set HMS_OLD_MEMORY_BIN to an older openmind-memory");
        }
    }

    // 7. Keys never in gateway logs; start log names the scope.
    drop(gw);
    for (g, want) in [("gw", "scope=session"), ("gw-cross", "scope=all")] {
        let log = std::fs::read_to_string(tmp.join(g).join("gateway.log")).unwrap_or_default();
        assert!(
            !log.contains(MEM_KEY) && !log.contains(API_KEY),
            "{g} log leaks key"
        );
        assert!(log.contains(want), "{g}: {log}");
    }
    eprintln!("[{}] real_memory_session_scope PASS", zagreb_now());
    let _ = std::fs::remove_dir_all(&tmp);
}
