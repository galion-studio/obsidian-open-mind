// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life rehearsal for wave-4 slot 406 (legacy-vectors-archive-plan,
//! D-388-2): the archive runbook in docs/ops/legacy-vectors-archive.md,
//! step for step, on a BOX tree (never the VPS). Starts the real release
//! `openmind-memory` on 127.0.0.1 (slot ports 22760-22769, override with
//! `REAL_LEGACY_ARCHIVE_PORT`) and checks:
//! - before: `freshness` reports the legacy root `vectors.db` as
//!   `stray_root_db=yes`;
//! - archive (`tar -cpf` of vectors.db + -wal/-shm, `sha256sum`, extract
//!   and re-hash) gives a byte-identical copy that opens read-only with the
//!   same row count;
//! - after removing the three files the server (instance `galion`) is
//!   healthy, captures still land in the instance DB, and it never recreates
//!   the root file; `freshness` drops the stray line, instance counts same;
//! - hazard: the same server WITHOUT `OPENMIND_MEMORY_INSTANCE_ID` creates a
//!   new empty root DB (so the env line must stay);
//! - rollback: extracting the tar back restores the original bytes.
//!
//! Optional: `GALION_MEMORY_SNAPSHOT_DIR=<dir>` = a box COPY of the live VPS
//! data dir (root vectors.db + instances/galion/vectors.db + -wal, copied
//! read-only). The rehearsal then runs on a second throwaway copy of it,
//! reports only counts (never content or session keys), and checks the live
//! numbers: root 4 l0 rows, instance 2610 l0 / 216 l1 rows unchanged.
//! Gated: `GALION_REAL=1`.

use openmind_memory::store::{L0Record, Store};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TRIO: [&str; 3] = ["vectors.db", "vectors.db-wal", "vectors.db-shm"];
// Stored UTC wire rows (the DB keeps `...Z`; freshness prints Zagreb).
const LEGACY_AT: &str = "2026-10-13T09:19:53.385Z"; // repo-check: allow (stored UTC row under test)
const LIVE_AT: &str = "2026-10-18T21:57:27.000Z"; // repo-check: allow (stored UTC row under test)

fn port(off: u16) -> u16 {
    std::env::var("REAL_LEGACY_ARCHIVE_PORT")
        .ok()
        .map(|v| v.parse().expect("REAL_LEGACY_ARCHIVE_PORT must be a port"))
        .unwrap_or(22760)
        + off
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

struct Server(Option<Child>, u16);

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

fn start(dir: &Path, p: u16, instance: Option<&str>) -> Server {
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} already in use"
    );
    let mut cmd = Command::new(bin());
    clean(&mut cmd)
        .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", p.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(i) = instance {
        cmd.env("OPENMIND_MEMORY_INSTANCE_ID", i);
    }
    let srv = Server(Some(cmd.spawn().expect("spawn openmind-memory")), p);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        if let Ok((200, _)) = http(p, "GET", "/health", None) {
            println!(
                "server :{p} instance={instance:?} /health 200 after {:?}",
                t0.elapsed()
            );
            return srv;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("openmind-memory on {p} did not become healthy");
}

fn stop(srv: Server) {
    let p = srv.1;
    drop(srv);
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} still open after kill"
    );
}

fn freshness(dir: &Path) -> (i32, String) {
    let out = clean(&mut Command::new(bin()))
        .args(["freshness", &dir.display().to_string(), "galion", "24"])
        .output()
        .expect("run freshness");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    println!(
        "--- freshness galion (exit {:?}) ---\n{text}---",
        out.status.code()
    );
    (out.status.code().unwrap_or(-1), text)
}

/// `instance` block line `  l0_rows=N ...` (first one = the instance DB).
fn instance_rows(text: &str) -> (String, String) {
    let pick = |k: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with(k))
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or("")
            .to_string()
    };
    (pick("l0_rows="), pick("l1_rows="))
}

fn sha256(p: &Path) -> String {
    let o = Command::new("sha256sum").arg(p).output().unwrap();
    assert!(o.status.success());
    String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn count_ro(p: &Path, table: &str) -> i64 {
    let c = rusqlite::Connection::open_with_flags(
        p,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn sh(args: &[&str], cwd: Option<&Path>) {
    let mut c = Command::new(args[0]);
    c.args(&args[1..]);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

struct Archived {
    tar: PathBuf,
    db_sha: String,
    members: Vec<String>,
}

/// Runbook step 3+4: tar the root trio (only files that exist), hash, verify
/// by extracting into a scratch dir and re-hashing; then step 5: remove.
fn archive_and_remove(dir: &Path, arch: &Path) -> Archived {
    std::fs::create_dir_all(arch).unwrap();
    let members: Vec<String> = TRIO
        .iter()
        .filter(|n| dir.join(n).is_file())
        .map(|n| n.to_string())
        .collect();
    assert!(members.contains(&"vectors.db".to_string()));
    let db_sha = sha256(&dir.join("vectors.db"));
    let rows = count_ro(&dir.join("vectors.db"), "l0_conversations");
    let tar = arch.join("legacy-root-vectors.tar");
    let mut args = vec![
        "tar",
        "-cpf",
        tar.to_str().unwrap(),
        "-C",
        dir.to_str().unwrap(),
    ];
    args.extend(members.iter().map(String::as_str));
    sh(&args, None);
    let verify = arch.join("verify");
    std::fs::create_dir_all(&verify).unwrap();
    sh(
        &[
            "tar",
            "-xpf",
            tar.to_str().unwrap(),
            "-C",
            verify.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        sha256(&verify.join("vectors.db")),
        db_sha,
        "archived copy byte-identical"
    );
    assert_eq!(
        count_ro(&verify.join("vectors.db"), "l0_conversations"),
        rows
    );
    println!("archive ok: members={members:?} sha256={db_sha} l0_rows={rows}");
    std::fs::remove_dir_all(&verify).unwrap();
    for m in &members {
        std::fs::remove_file(dir.join(m)).unwrap();
    }
    for n in TRIO {
        assert!(!dir.join(n).exists(), "{n} removed");
    }
    Archived {
        tar,
        db_sha,
        members,
    }
}

fn rollback(dir: &Path, a: &Archived) {
    sh(
        &[
            "tar",
            "-xpf",
            a.tar.to_str().unwrap(),
            "-C",
            dir.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(
        sha256(&dir.join("vectors.db")),
        a.db_sha,
        "rollback restores bytes"
    );
    for m in &a.members {
        assert!(dir.join(m).is_file(), "{m} back");
    }
    println!("rollback ok: sha256={}", a.db_sha);
}

fn rec(id: &str, sk: &str, at: &str) -> L0Record {
    L0Record {
        id: id.into(),
        session_key: sk.into(),
        session_id: sk.into(),
        role: "user".into(),
        message_text: format!("private text {id}"),
        recorded_at: at.into(),
        timestamp: 1,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (epoch ms for the /capture wire timestamp, never printed)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn real_legacy_root_archive_rehearsal() {
    if !real() {
        return;
    }
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-legacy-root");
    let _ = std::fs::remove_dir_all(&base);
    let dir = base.join("data");
    std::fs::create_dir_all(dir.join("instances/galion")).unwrap();
    {
        let st = Store::open(&dir.join("vectors.db"), false).unwrap();
        for i in 0..4 {
            st.upsert_l0(&rec(
                &format!("legacy-{i}"),
                "legacy-secret-session",
                LEGACY_AT,
            ))
            .unwrap();
        }
        let st = Store::open(&dir.join("instances/galion/vectors.db"), false).unwrap();
        for i in 0..3 {
            st.upsert_l0(&rec(&format!("live-{i}"), "live-session", LIVE_AT))
                .unwrap();
        }
    }
    // Step 1 pre-check.
    let (c0, t0) = freshness(&dir);
    assert!(t0.contains("stray_root_db=yes"), "{t0}");
    assert_eq!(instance_rows(&t0).0, "l0_rows=3", "{t0}");

    // Steps 3-5 archive + remove.
    let a = archive_and_remove(&dir, &base.join("archive"));
    let (c1, t1) = freshness(&dir);
    assert!(!t1.contains("stray_root_db"), "{t1}");
    assert!(!t1.contains("db[legacy-root]"), "{t1}");
    assert_eq!(instance_rows(&t1), instance_rows(&t0), "instance unchanged");
    assert_eq!(c1, c0, "same verdict for the instance");

    // Step 6 post-check: real server, real capture, root never recreated.
    let srv = start(&dir, port(0), Some("galion"));
    let ts = now_ms();
    let (s, v) = http(
        port(0),
        "POST",
        "/capture",
        Some(&json!({
            "user_content": "after archive", "assistant_content": "ok",
            "session_key": "post-archive-session",
            "messages": [
                {"role": "user", "content": "after archive", "timestamp": ts},
                {"role": "assistant", "content": "ok noted", "timestamp": ts + 1}
            ],
        })),
    )
    .unwrap();
    assert_eq!(s, 200, "{v}");
    stop(srv);
    for n in TRIO {
        assert!(!dir.join(n).exists(), "server recreated {n}");
    }
    let (c2, t2) = freshness(&dir);
    assert_eq!(c2, 0, "{t2}");
    let n: i64 = instance_rows(&t2)
        .0
        .trim_start_matches("l0_rows=")
        .parse()
        .unwrap();
    assert!(n > 3, "capture landed in the instance DB: {t2}");
    assert!(!t2.contains("stray_root_db"), "{t2}");
    for secret in [
        "private text",
        "legacy-secret",
        "post-archive",
        "after archive",
    ] {
        assert!(!t2.contains(secret), "leaked {secret}");
    }

    // Hazard: no instance id -> default instance = a NEW empty root DB.
    let srv = start(&dir, port(1), None);
    stop(srv);
    assert!(
        dir.join("vectors.db").is_file(),
        "default instance creates root DB"
    );
    assert_eq!(count_ro(&dir.join("vectors.db"), "l0_conversations"), 0);
    println!("hazard confirmed: server without OPENMIND_MEMORY_INSTANCE_ID created an empty root vectors.db");
    for n in TRIO {
        let _ = std::fs::remove_file(dir.join(n));
    }

    // Rollback.
    rollback(&dir, &a);
    let (_, t3) = freshness(&dir);
    assert!(t3.contains("stray_root_db=yes"), "{t3}");
    assert!(
        t3.contains("  l0_rows=4 l0_last=2026-10-13T11:19:53+02:00"),
        "{t3}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn real_legacy_root_archive_live_snapshot_copy() {
    if !real() {
        return;
    }
    let Some(src) = std::env::var_os("GALION_MEMORY_SNAPSHOT_DIR").map(PathBuf::from) else {
        eprintln!("skip: set GALION_MEMORY_SNAPSHOT_DIR to a box copy of the live data dir");
        return;
    };
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("real-legacy-root-live");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let dir = base.join("data");
    sh(
        &["cp", "-a", src.to_str().unwrap(), dir.to_str().unwrap()],
        None,
    );

    // Is anything in the root DB that the live instance lacks? Counts only.
    let root_db = dir.join("vectors.db");
    let inst_db = dir.join("instances/galion/vectors.db");
    {
        let c = rusqlite::Connection::open_with_flags(
            &root_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let tables: Vec<String> = c
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '%fts%' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        let mut counts = Vec::new();
        for t in &tables {
            let n: i64 = c
                .query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))
                .unwrap_or(-1);
            counts.push(format!("{t}={n}"));
        }
        println!("root tables (non-fts): {}", counts.join(" "));
        let ids: Vec<(String, String, String)> = c
            .prepare("SELECT record_id, session_key, recorded_at FROM l0_conversations")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(ids.len(), 4, "live root DB has 4 l0 rows");
        let i = rusqlite::Connection::open_with_flags(
            &inst_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut same_id = 0;
        let mut same_key_time = 0;
        let mut same_key = 0;
        for (id, sk, at) in &ids {
            let n: i64 = i
                .query_row(
                    "SELECT count(*) FROM l0_conversations WHERE record_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            same_id += (n > 0) as i32;
            let n: i64 = i
                .query_row(
                    "SELECT count(*) FROM l0_conversations WHERE session_key=?1 AND recorded_at=?2",
                    [sk, at],
                    |r| r.get(0),
                )
                .unwrap();
            same_key_time += (n > 0) as i32;
            let n: i64 = i
                .query_row(
                    "SELECT count(*) FROM l0_conversations WHERE session_key=?1",
                    [sk],
                    |r| r.get(0),
                )
                .unwrap();
            same_key += (n > 0) as i32;
        }
        println!("root l0 rows also in live instance: by record_id {same_id}/4, by session_key+recorded_at {same_key_time}/4, session_key seen {same_key}/4");
    }

    let (c0, t0) = freshness(&dir);
    assert_eq!(c0, 3, "{t0}");
    assert!(t0.contains("stray_root_db=yes"), "{t0}");
    assert_eq!(
        instance_rows(&t0),
        ("l0_rows=2610".to_string(), "l1_rows=216".to_string()),
        "{t0}"
    );
    let a = archive_and_remove(&dir, &base.join("archive"));
    assert_eq!(
        a.db_sha, "cb0a3a5a0eba869a5aa2159ceda6f1355199c31c45209d42087e263f2e6e748d",
        "raw live root DB hash (2026-10-28 17:23 CET copy)"
    );
    let (c1, t1) = freshness(&dir);
    assert_eq!(
        c1, 3,
        "still stale: archiving does not change the instance verdict"
    );
    assert!(!t1.contains("stray_root_db"), "{t1}");
    assert_eq!(instance_rows(&t1), instance_rows(&t0), "{t1}");
    // Real server on the archived copy, read-only so nothing is written.
    let p = port(2);
    let mut cmd = Command::new(bin());
    clean(&mut cmd)
        .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", p.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", &dir)
        .env("OPENMIND_MEMORY_INSTANCE_ID", "galion")
        .env("OPENMIND_MEMORY_READ_ONLY", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let srv = Server(Some(cmd.spawn().unwrap()), p);
    let t = Instant::now();
    let mut health = Value::Null;
    while t.elapsed() < Duration::from_secs(10) {
        if let Ok((200, v)) = http(p, "GET", "/health", None) {
            health = v;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("live-copy server :{p} /health {health}");
    assert!(health.is_object(), "server healthy on archived live copy");
    stop(srv);
    for n in TRIO {
        assert!(!dir.join(n).exists(), "server recreated {n}");
    }
    let (_, t2) = freshness(&dir);
    assert_eq!(instance_rows(&t2), instance_rows(&t0), "{t2}");
    rollback(&dir, &a);
    let (_, t3) = freshness(&dir);
    assert!(
        t3.contains("  l0_rows=4 l0_last=2026-10-13T11:19:53+02:00"),
        "{t3}"
    );
    let _ = std::fs::remove_dir_all(&base);
}
