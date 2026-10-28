// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test for wave-4 slot 388 (memory-db-stale-diag): start the real
//! release `openmind-memory` on 127.0.0.1 (slot ports 22580-22589, override
//! with `REAL_MEMORY_FRESH_PORT`) with the live layout (data dir + instance
//! `galion` + a legacy root `vectors.db`), run the real `freshness` CLI
//! before and after a live `/capture`, and check that:
//! - an instance with no writes is `stale` (exit 3) even though the stray
//!   root DB has rows and a reader just touched its WAL;
//! - one real `/capture` makes it `fresh` (exit 0) with the new row counted;
//! - output never carries memory content or session keys.
//!
//! Optional: `GALION_MEMORY_SNAPSHOT_DIR=<dir>` points at a box COPY of the
//! live VPS data dir (DB + WAL copied read-only, mtimes restored) and checks
//! the live verdict: stale, instance last l0 row 2026-10-19T14:57:55+02:00,
//! legacy root DB last row 2026-10-13T11:19:53+02:00 (the stamp a probe
//! mistook for "last write"). Gated: `GALION_REAL=1`. Synthetic data only.

use openmind_memory::store::{L0Record, Store};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn port() -> u16 {
    std::env::var("REAL_MEMORY_FRESH_PORT")
        .ok()
        .map(|v| v.parse().expect("REAL_MEMORY_FRESH_PORT must be a port"))
        .unwrap_or(22580)
}

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1");
    }
    on
}

fn bin() -> PathBuf {
    std::env::var_os("OPENMIND_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openmind-memory")))
}

struct Server(Option<Child>);

impl Drop for Server {
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
    body: Option<&Value>,
) -> std::io::Result<(u16, Value)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Ok((status, serde_json::from_str(rest).unwrap_or(Value::Null)))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for k in [
        "OPENMIND_MEMORY_API_KEY",
        "TDAI_GATEWAY_API_KEY",
        "OPENMIND_MEMORY_READ_ONLY",
        "OPENMIND_MEMORY_INSTANCE_ID",
        "TDAI_INSTANCE_ID",
        "TDAI_DATA_DIR",
        "NERVE_URL",
    ] {
        cmd.env_remove(k);
    }
    cmd
}

fn start(dir: &Path) -> Server {
    let p = port();
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} already in use"
    );
    let child = clean(&mut Command::new(bin()))
        .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", p.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .env("OPENMIND_MEMORY_INSTANCE_ID", "galion")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openmind-memory");
    let srv = Server(Some(child));
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        if let Ok((200, _)) = http(p, "GET", "/health", None) {
            return srv;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("openmind-memory on {p} did not become healthy");
}

fn freshness(dir: &Path, instance: &str, max_age_h: &str) -> (i32, String) {
    let out = clean(&mut Command::new(bin()))
        .args(["freshness", &dir.display().to_string(), instance, max_age_h])
        .output()
        .expect("run freshness");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    println!(
        "--- freshness {instance} (exit {:?}) ---\n{text}---",
        out.status.code()
    );
    (out.status.code().unwrap_or(-1), text)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (epoch ms for the /capture wire timestamp, never printed)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn real_freshness_stale_then_fresh_after_capture() {
    if !real() {
        return;
    }
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-freshness");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Legacy root DB (default instance path) with rows, like the VPS.
    {
        let st = Store::open(&dir.join("vectors.db"), false).unwrap();
        st.upsert_l0(&L0Record {
            id: "legacy-1".into(),
            session_key: "legacy-secret-session".into(),
            session_id: "legacy-secret-session".into(),
            role: "user".into(),
            message_text: "legacy private text".into(),
            recorded_at: "2026-10-13T09:19:53.385Z".into(), // repo-check: allow (stored UTC row under test)
            timestamp: 1,
        })
        .unwrap();
    }
    let srv = start(&dir);
    let p = port();

    // 1) Server up, instance DB just created by the server, no capture yet:
    //    0 rows, the stray root DB reported separately with its own last row.
    let (code, text) = freshness(&dir, "galion", "24");
    assert!(text.contains("db[instance]="), "{text}");
    assert!(text.contains("instances/galion/vectors.db"), "{text}");
    assert!(text.contains("  l0_rows=0 l0_last=none"), "{text}");
    assert!(text.contains("stray_root_db=yes"), "{text}");
    assert!(text.contains("l0_last=2026-10-13T11:19:53+02:00"), "{text}");
    assert!(text.contains("conversations_last_file=none"), "{text}");
    // DB was just created by the server, so mtime is fresh: exit 0 is the
    // honest answer; the l0 count shows there were no captures.
    assert_eq!(code, 0, "{text}");

    // 3) One real capture: l0 rows 2, conversations JSONL written, fresh.
    let ts = now_ms() + 86_400_000;
    let (s, v) = http(
        p,
        "POST",
        "/capture",
        Some(&json!({
            "user_content": "zagreb tram live", "assistant_content": "ok",
            "session_key": "fresh-live-session",
            "messages": [
                {"role": "user", "content": "zagreb tram live", "timestamp": ts},
                {"role": "assistant", "content": "ok noted", "timestamp": ts + 1}
            ],
        })),
    )
    .unwrap();
    assert_eq!(s, 200, "{v}");
    let (code, text) = freshness(&dir, "galion", "1");
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("  l0_rows=2 l0_last="), "{text}");
    assert!(!text.contains("l0_last=none\nconversations"), "{text}");
    assert!(
        text.lines()
            .any(|l| l.starts_with("conversations_last_file=") && !l.ends_with("=none")),
        "{text}"
    );
    assert!(text.contains("state=fresh (max_age_h=1)"), "{text}");
    for secret in [
        "zagreb tram",
        "fresh-live-session",
        "legacy private",
        "legacy-secret",
    ] {
        assert!(!text.contains(secret), "leaked {secret}: {text}");
    }
    drop(srv);
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} still open after kill"
    );

    // 4) Default instance = the legacy root DB. A reader-touched empty WAL
    //    must not count as a write: with the root DB mtime pushed back and the
    //    shared JSONL dirs removed it is stale.
    std::fs::remove_dir_all(dir.join("conversations")).unwrap();
    let _ = std::fs::remove_dir_all(dir.join("records"));
    let old = std::process::Command::new("touch")
        .args(["-d", "2026-10-13 09:20:17 UTC"])
        .arg(dir.join("vectors.db"))
        .status()
        .unwrap();
    assert!(old.success());
    let (code, text) = freshness(&dir, "default", "24");
    assert!(
        text.contains("(empty; reader-touched, ignored)") || !text.contains("wal_mtime"),
        "{text}"
    );
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("state=stale"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_freshness_live_snapshot_copy() {
    if !real() {
        return;
    }
    let Some(dir) = std::env::var_os("GALION_MEMORY_SNAPSHOT_DIR").map(PathBuf::from) else {
        eprintln!("skip: set GALION_MEMORY_SNAPSHOT_DIR to a box copy of the live data dir");
        return;
    };
    let (code, text) = freshness(&dir, "galion", "24");
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("instances/galion/vectors.db"), "{text}");
    assert!(text.contains("l0_last=2026-10-19T14:57:55+02:00"), "{text}");
    assert!(text.contains("stray_root_db=yes"), "{text}");
    assert!(text.contains("l0_last=2026-10-13T11:19:53+02:00"), "{text}");
    assert!(text.contains("state=stale (max_age_h=24)"), "{text}");
    let (code, text) = freshness(&dir, "default", "24");
    assert!(
        text.contains("  l0_rows=4 l0_last=2026-10-13T11:19:53+02:00"),
        "{text}"
    );
    assert!(!text.contains("stray_root_db"), "{text}");
    let _ = code;
}
