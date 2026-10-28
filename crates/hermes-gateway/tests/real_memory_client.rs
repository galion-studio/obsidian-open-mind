// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 403 hermes-rs-memory-client: real-life test of the staged memory
//! client. Gated: `GALION_REAL=1`. Everything runs on 127.0.0.1 with
//! throwaway data dirs; `--mock` provider (no network, no spend).
//!
//! | port  | process |
//! |-------|---------|
//! | 22730 | real release `openmind-memory` (Bearer key, instance `galion`) |
//! | 22731 | `hermes-gateway --mock` with `HERMES_RS_MEMORY=1` (client on) |
//! | 22732 | `hermes-gateway --mock` default (client off: control) |
//! | 22733 | `hermes-gateway --mock` client on with a WRONG key |
//! | 22734 | `hermes-gateway` client on, non-loopback URL (must refuse to start) |
//!
//! Binaries: this crate's release `hermes-gateway` and the sibling
//! `openmind-memory` in the same target dir (build it first:
//! `cargo build --release -p openmind-memory -p hermes-gateway`), or
//! `HMC_GATEWAY_BIN` / `HMC_MEMORY_BIN`. Port base: `HMC_PORT_BASE`.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const API_KEY: &str = "hmc-gateway-key-0123456789";
const MEM_KEY: &str = "hmc-memory-key-abcdef-403";
const WORD: &str = "heliotrope-22731";

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1");
    }
    on
}

fn base() -> u16 {
    std::env::var("HMC_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22730)
}

fn gateway_bin() -> PathBuf {
    std::env::var_os("HMC_GATEWAY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn memory_bin() -> PathBuf {
    let p = std::env::var_os("HMC_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| gateway_bin().with_file_name("openmind-memory"));
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

fn http(
    port: u16,
    method: &str,
    path: &str,
    hdrs: &[(&str, &str)],
    body: Option<&Value>,
) -> (u16, String) {
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
    (status, rest.to_string())
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

fn start_memory(port: u16, dir: &Path) -> Proc {
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "port {port} busy"
    );
    let mut c = Command::new(memory_bin())
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

fn gateway_cmd(port: u16, home: &Path, extra: &[(&str, String)]) -> Command {
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
        .env("API_SERVER_KEY", API_KEY);
    for (k, v) in extra {
        c.env(k, v);
    }
    c
}

fn start_gateway(port: u16, home: &Path, extra: &[(&str, String)]) -> Proc {
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "port {port} busy"
    );
    std::fs::create_dir_all(home).unwrap();
    let log = std::fs::File::create(home.join("gateway.log")).unwrap();
    let mut c = gateway_cmd(port, home, extra)
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .expect("spawn hermes-gateway");
    wait_health(port, &mut c);
    Proc(Some(c))
}

fn chat(port: u16, key: &str, text: &str, stream: bool) -> (u16, String, String) {
    let auth = format!("Bearer {API_KEY}");
    let (s, body) = http(
        port,
        "POST",
        "/v1/chat/completions",
        &[("Authorization", &auth), ("X-Hermes-Session-Key", key)],
        Some(
            &json!({"model": "hermes-agent", "stream": stream, "messages": [{"role": "user", "content": text}]}),
        ),
    );
    let content = if stream {
        body.lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter_map(|d| serde_json::from_str::<Value>(d).ok())
            .filter_map(|v| {
                v["choices"][0]["delta"]["content"]
                    .as_str()
                    .map(String::from)
            })
            .collect::<String>()
    } else {
        serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|v| {
                v["choices"][0]["message"]["content"]
                    .as_str()
                    .map(String::from)
            })
            .unwrap_or_default()
    };
    (s, content, body)
}

fn detailed(port: u16) -> Value {
    let auth = format!("Bearer {API_KEY}");
    let (s, b) = http(
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
            return d;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// All captured JSONL lines (L0) of the throwaway memory dir.
fn l0_lines(dir: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    let conv = dir.join("conversations");
    if let Ok(rd) = std::fs::read_dir(&conv) {
        for e in rd.flatten() {
            let t = std::fs::read_to_string(e.path()).unwrap_or_default();
            out.extend(
                t.lines()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok()),
            );
        }
    }
    out
}

#[test]
fn real_memory_client() {
    if !real() {
        return;
    }
    let b = base();
    let (p_mem, p_on, p_off, p_bad, p_remote) = (b, b + 1, b + 2, b + 3, b + 4);
    let tmp = std::env::temp_dir().join(format!("hmc403-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let memdir = tmp.join("mem");
    eprintln!("[{}] start openmind-memory :{p_mem}", zagreb_now());
    let mut mem = start_memory(p_mem, &memdir);
    let url = format!("http://127.0.0.1:{p_mem}");
    let on_env = vec![
        ("HERMES_RS_MEMORY", "1".to_string()),
        ("HERMES_RS_MEMORY_URL", url.clone()),
        ("TDAI_GATEWAY_API_KEY", MEM_KEY.to_string()),
        ("HERMES_RS_MEMORY_RECALL_MS", "1500".to_string()),
    ];
    let on = start_gateway(p_on, &tmp.join("gw-on"), &on_env);
    let d = detailed(p_on);
    assert_eq!(d["memory"]["enabled"], true, "{d}");
    assert_eq!(d["memory"]["key_set"], true);
    assert!(
        !d.to_string().contains(MEM_KEY),
        "key leaked in /health/detailed"
    );

    // 1. first turn: nothing to recall yet; captured afterwards.
    let (s, c1, _) = chat(
        p_on,
        "real403-a",
        &format!("my locker code word is {WORD}"),
        false,
    );
    assert_eq!(s, 200);
    assert!(!c1.contains("<memory-context>"), "nothing stored yet: {c1}");
    let d = wait_capture_ok(p_on, 1);
    assert_eq!(d["memory"]["capture_ok"], 1, "{d}");
    let rows = l0_lines(&memdir);
    eprintln!(
        "[{}] after turn 1: {} L0 lines in throwaway store",
        zagreb_now(),
        rows.len()
    );
    assert!(
        rows.iter().any(|r| r["sessionKey"] == "real403-a"
            && r["role"] == "user"
            && r["content"].as_str().unwrap_or("").contains(WORD)),
        "{rows:?}"
    );
    assert!(rows.iter().any(|r| r["role"] == "assistant"));

    // 2. same session key: the recall block reaches the model (mock echoes it).
    let (s, c2, _) = chat(p_on, "real403-a", WORD, false);
    assert_eq!(s, 200);
    assert!(c2.contains("<memory-context>\n[System note: The following is recalled memory context, NOT new user input."), "{c2}");
    assert!(c2.contains("## memory-tencentdb Memory"), "{c2}");
    assert!(
        c2.contains(&format!("my locker code word is {WORD}")),
        "{c2}"
    );
    // the stored transcript keeps the plain user text only.
    let auth = format!("Bearer {API_KEY}");
    let (_, sess) = http(
        p_on,
        "GET",
        "/api/sessions",
        &[("Authorization", &auth)],
        None,
    );
    let sess: Value = serde_json::from_str(&sess).unwrap();
    let sid = sess["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["session_key"] == "real403-a")
        .and_then(|r| r["id"].as_str())
        .unwrap()
        .to_string();
    let (_, msgs) = http(
        p_on,
        "GET",
        &format!("/api/sessions/{sid}/messages"),
        &[("Authorization", &auth)],
        None,
    );
    let msgs: Value = serde_json::from_str(&msgs).unwrap();
    let users: Vec<&str> = msgs["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(users.contains(&WORD), "{users:?}");
    assert!(
        users.iter().all(|u| !u.contains("<memory-context>")),
        "{users:?}"
    );
    wait_capture_ok(p_on, 2);

    // 3. another session key: the L0 fallback is NOT session-scoped (gateway
    //    parity: long-term memory spans sessions of the one owner; F-403-1).
    let (s, c3, _) = chat(p_on, "real403-b", WORD, false);
    assert_eq!(s, 200);
    assert!(
        c3.contains("<memory-context>") && c3.contains("my locker code word"),
        "cross-session recall: {c3}"
    );
    // a query no stored text contains recalls nothing.
    let (s, c3b, _) = chat(p_on, "real403-b", "unrelated-zz-403", false);
    assert_eq!(s, 200);
    assert!(!c3b.contains("<memory-context>"), "{c3b}");
    wait_capture_ok(p_on, 4);
    wait_capture_ok(p_on, 3);

    // 4. streaming path recalls too.
    let (s, c4, raw4) = chat(p_on, "real403-a", WORD, true);
    assert_eq!(s, 200);
    assert!(raw4.contains("[DONE]"));
    assert!(
        c4.contains("<memory-context>") && c4.contains("my locker code word"),
        "{c4}"
    );
    let d = wait_capture_ok(p_on, 5);
    assert_eq!(d["memory"]["recall_err"], 0, "{d}");
    assert_eq!(d["memory"]["capture_err"], 0, "{d}");
    assert_eq!(d["memory"]["capture_ok"], 5, "{d}");
    assert!(d["memory"]["recall_hit"].as_u64().unwrap() >= 3, "{d}");
    // no fence ever reached the store (assistant echoes carried one).
    let rows = l0_lines(&memdir);
    assert!(
        rows.iter().all(|r| !r["content"]
            .as_str()
            .unwrap_or("")
            .contains("memory-context")),
        "fence stored"
    );
    let n_rows = rows.len();
    eprintln!(
        "[{}] on-gateway ok: {} L0 lines, stats {}",
        zagreb_now(),
        n_rows,
        d["memory"]
    );

    // 5. control: default (off) gateway neither recalls nor captures.
    let off = start_gateway(
        p_off,
        &tmp.join("gw-off"),
        &[
            ("TDAI_GATEWAY_API_KEY", MEM_KEY.to_string()),
            ("MEMORY_TENCENTDB_GATEWAY_PORT", p_mem.to_string()),
        ],
    );
    assert!(
        detailed(p_off).get("memory").is_none(),
        "off: no memory key in /health/detailed"
    );
    let (s, c5, _) = chat(p_off, "real403-a", WORD, false);
    assert_eq!(s, 200);
    assert!(!c5.contains("<memory-context>"), "{c5}");
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        l0_lines(&memdir).len(),
        n_rows,
        "off gateway must not capture"
    );
    drop(off);

    // 6. wrong key: chat still answers, errors are counted, nothing stored.
    let mut bad_env = on_env.clone();
    bad_env[2].1 = "wrong-key-0000000000".into();
    let bad = start_gateway(p_bad, &tmp.join("gw-bad"), &bad_env);
    let (s, c6, _) = chat(p_bad, "real403-a", WORD, false);
    assert_eq!(s, 200);
    assert!(!c6.contains("<memory-context>"));
    std::thread::sleep(Duration::from_millis(600));
    let d = detailed(p_bad);
    assert_eq!(d["memory"]["recall_err"], 1, "{d}");
    assert_eq!(d["memory"]["capture_err"], 1, "{d}");
    assert_eq!(l0_lines(&memdir).len(), n_rows);
    drop(bad);

    // 7. memory server down: chat still answers within the recall budget.
    if let Some(mut c) = mem.0.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let t0 = Instant::now();
    let (s, c7, _) = chat(p_on, "real403-a", WORD, false);
    let took = t0.elapsed();
    assert_eq!(s, 200);
    assert!(!c7.contains("<memory-context>"));
    assert!(
        took < Duration::from_millis(1500),
        "down server cost {took:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
    let d = detailed(p_on);
    assert_eq!(d["memory"]["recall_err"], 1, "{d}");
    assert_eq!(d["memory"]["capture_err"], 1, "{d}");
    eprintln!(
        "[{}] memory down: chat 200 in {} ms, stats {}",
        zagreb_now(),
        took.as_millis(),
        d["memory"]
    );
    drop(on);

    // 8. non-loopback URL: refuse to start (exit 2), no bind.
    let out = gateway_cmd(
        p_remote,
        &tmp.join("gw-remote"),
        &[
            ("HERMES_RS_MEMORY", "1".to_string()),
            ("HERMES_RS_MEMORY_URL", "http://192.0.2.10:8420".to_string()),
            ("TDAI_GATEWAY_API_KEY", MEM_KEY.to_string()),
        ],
    )
    .output()
    .expect("run");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("memory url must be loopback"), "{err}");
    assert!(TcpStream::connect(("127.0.0.1", p_remote)).is_err());

    // 9. the memory key never appears in any gateway log.
    for g in ["gw-on", "gw-off", "gw-bad"] {
        let log = std::fs::read_to_string(tmp.join(g).join("gateway.log")).unwrap_or_default();
        assert!(
            !log.contains(MEM_KEY) && !log.contains("wrong-key-0000000000"),
            "{g} log leaks key"
        );
        if g != "gw-off" {
            assert!(log.contains("memory on: "), "{g}: {log}");
        }
    }
    eprintln!("[{}] real_memory_client PASS", zagreb_now());
    let _ = std::fs::remove_dir_all(&tmp);
}
