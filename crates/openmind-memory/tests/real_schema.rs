// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life tests for swarm/memory-schema (slot 36, ports 19366-19369; the
//! memory-real suite reuses 19360-19365 via OPENMIND_REAL_PORT_BASE). Each
//! test starts the release `openmind-memory` binary on 127.0.0.1 with a
//! FIXTURE database and talks to it over real sockets:
//! - kill -9 during the schema migration, in a loop, on a large pre-v3
//!   gateway-style DB (user_version 0, no timestamp column, no FTS tables);
//! - CJK / emoji round trip through /capture, SQLite, JSONL and FTS search;
//! - opt-in import mode: replay old timestamps, idempotent, concurrent;
//! - (GALION_REAL_VPS=1) ONE read-only GET /health of live 8420 over ssh.
//!
//! Gated with GALION_REAL=1; run via scripts/real/memory-schema.sh.

use rusqlite::{params, Connection, OpenFlags};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT_KILL: u16 = 19366;
const PORT_CJK: u16 = 19367;
const PORT_IMPORT: u16 = 19368;
const PORT_DEFAULT: u16 = 19369;
const SCHEMA_VERSION: i64 = openmind_memory::store::SCHEMA_VERSION;

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1 (see scripts/real/memory-schema.sh)");
    }
    on
}

fn bin() -> PathBuf {
    std::env::var_os("OPENMIND_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openmind-memory")))
}

fn fresh_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("schema-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn spawn(port: u16, dir: &Path, env: &[(&str, &str)]) -> Child {
    let mut cmd = Command::new(bin());
    for k in [
        "OPENMIND_MEMORY_API_KEY",
        "TDAI_GATEWAY_API_KEY",
        "OPENMIND_MEMORY_READ_ONLY",
        "OPENMIND_MEMORY_ALLOW_IMPORT",
        "TDAI_INSTANCE_ID",
        "OPENMIND_MEMORY_INSTANCE_ID",
        "TDAI_DATA_DIR",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", port.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.spawn().expect("spawn openmind-memory")
}

struct Server {
    child: Option<Child>,
    port: u16,
}

impl Server {
    fn start(port: u16, dir: &Path, env: &[(&str, &str)]) -> Server {
        let s = Server {
            child: Some(spawn(port, dir, env)),
            port,
        };
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(60) {
            if let Ok((200, _)) = http(port, "GET", "/health", None) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("server on {port} did not become healthy");
    }
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let b = body.map(Value::to_string);
        let (s, text) = http(self.port, method, path, b.as_deref()).unwrap();
        (
            s,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill(); // SIGKILL
            let _ = c.wait();
        }
    }
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> std::io::Result<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let text = String::from_utf8(raw).expect("response is valid UTF-8");
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Ok((status, rest.to_string()))
}

fn ro(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn q_i64(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn has_table(c: &Connection, t: &str) -> bool {
    c.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
        [t],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

fn has_col(c: &Connection, t: &str, col: &str) -> bool {
    c.query_row(
        &format!("SELECT COUNT(*) FROM pragma_table_info('{t}') WHERE name=?"),
        [col],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

/// Tiny deterministic PRNG (no rand crate).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

const MIXED: [&str; 12] = [
    "萨格勒布",
    "咖啡馆",
    "東京タワー",
    "서울특별시",
    "zagreb",
    "kiwi",
    "🦀",
    "👨‍👩‍👧",
    "👍🏽",
    "🇭🇷",
    "𠮷野家",
    "café",
];

fn mixed_sentence(i: u64) -> String {
    let mut g = Lcg(i ^ 0x9E37_79B9);
    let w: Vec<&str> = (0..6).map(|_| MIXED[(g.next() % 12) as usize]).collect();
    format!("fixture {i} {}", w.join(" "))
}

/// A gateway-style pre-v3 database exactly as an old tdai gateway left it:
/// user_version 0, l0_conversations WITHOUT `timestamp`, l1_records, and no
/// FTS tables at all. Big enough that the migration (column + indexes + FTS
/// rebuild of every row) takes a measurable time to kill -9 into.
fn pre_v3_fixture(path: &Path, l0: u64, l1: u64) {
    let mut c = Connection::open(path).unwrap();
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE embedding_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE l0_conversations (record_id TEXT PRIMARY KEY, session_key TEXT NOT NULL,
           session_id TEXT DEFAULT '', role TEXT NOT NULL DEFAULT '', message_text TEXT NOT NULL,
           recorded_at TEXT DEFAULT '');
         CREATE INDEX idx_l0_session ON l0_conversations(session_key);
         CREATE TABLE l1_records (record_id TEXT PRIMARY KEY, content TEXT NOT NULL, type TEXT DEFAULT '',
           priority INTEGER DEFAULT 50, scene_name TEXT DEFAULT '', session_key TEXT DEFAULT '',
           session_id TEXT DEFAULT '', timestamp_str TEXT DEFAULT '', timestamp_start TEXT DEFAULT '',
           timestamp_end TEXT DEFAULT '', created_time TEXT DEFAULT '', updated_time TEXT DEFAULT '',
           metadata_json TEXT DEFAULT '{}');",
    )
    .unwrap();
    let tx = c.transaction().unwrap();
    {
        let mut a = tx
            .prepare(
                "INSERT INTO l0_conversations VALUES (?, ?, ?, ?, ?, '2026-10-01T10:00:00.000Z')",
            )
            .unwrap();
        for i in 0..l0 {
            let sk = format!("old-{}", i % 50);
            let role = if i % 2 == 0 { "user" } else { "assistant" };
            a.execute(params![
                format!("old-{i:06}"),
                sk,
                sk,
                role,
                mixed_sentence(i)
            ])
            .unwrap();
        }
        let mut b = tx
            .prepare("INSERT INTO l1_records (record_id, content, type, scene_name, session_key) VALUES (?, ?, 'episodic', 'Scena', 'old-0')")
            .unwrap();
        for i in 0..l1 {
            b.execute(params![format!("m-{i:06}"), mixed_sentence(1_000_000 + i)])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Check the invariant after a kill: the DB is either fully old or fully new.
/// Returns true when the kill landed before the migration committed.
fn assert_atomic(db: &Path, l0: i64, l1: i64) -> bool {
    let c = ro(db);
    let ic: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ic, "ok");
    let uv = q_i64(&c, "PRAGMA user_version");
    assert_eq!(q_i64(&c, "SELECT COUNT(*) FROM l0_conversations"), l0);
    assert_eq!(q_i64(&c, "SELECT COUNT(*) FROM l1_records"), l1);
    match uv {
        0 => {
            assert!(
                !has_col(&c, "l0_conversations", "timestamp"),
                "half-migrated: column without version"
            );
            assert!(
                !has_table(&c, "l0_fts") && !has_table(&c, "l1_fts"),
                "half-migrated: FTS without version"
            );
            true
        }
        v if v == SCHEMA_VERSION => {
            assert!(has_col(&c, "l0_conversations", "timestamp"));
            assert_eq!(
                q_i64(&c, "SELECT COUNT(*) FROM l0_fts"),
                l0,
                "partial FTS rebuild"
            );
            assert_eq!(
                q_i64(&c, "SELECT COUNT(*) FROM l1_fts"),
                l1,
                "partial FTS rebuild"
            );
            false
        }
        v => panic!("unexpected user_version {v}"),
    }
}

#[test]
fn real_kill9_during_migration_loop() {
    if !real() {
        return;
    }
    let (l0, l1) = (60_000u64, 20_000u64);
    let base = fresh_dir("kill-fixture");
    let pristine = base.join("pristine.db");
    let t = Instant::now();
    pre_v3_fixture(&pristine, l0, l1);
    println!(
        "fixture: pre-v3 gateway DB, {l0} l0 + {l1} l1 rows, {} bytes, built in {:.0} ms",
        file_len(&pristine),
        t.elapsed().as_secs_f64() * 1000.0
    );

    // How long does a full migration take (spawn -> /health 200)?
    let dir = fresh_dir("kill-measure");
    std::fs::copy(&pristine, dir.join("vectors.db")).unwrap();
    let t = Instant::now();
    let srv = Server::start(PORT_KILL, &dir, &[]);
    let full_ms = t.elapsed().as_secs_f64() * 1000.0;
    drop(srv);
    assert!(!assert_atomic(
        &dir.join("vectors.db"),
        l0 as i64,
        l1 as i64
    ));
    println!("full migration (spawn -> healthy): {full_ms:.0} ms");

    // Phase A: fresh pristine copy every round, kill -9 at a spread of
    // delays across the migration window.
    let rounds = 16u64;
    let (mut mid, mut after) = (0, 0);
    let mut g = Lcg(42);
    for r in 0..rounds {
        let dir = fresh_dir(&format!("kill-a-{r}"));
        let db = dir.join("vectors.db");
        std::fs::copy(&pristine, &db).unwrap();
        let delay = 5 + (g.next() % (full_ms * 1.3) as u64);
        let mut child = spawn(PORT_KILL, &dir, &[]);
        std::thread::sleep(Duration::from_millis(delay));
        let wal = file_len(&dir.join("vectors.db-wal"));
        child.kill().unwrap(); // SIGKILL = kill -9
        child.wait().unwrap();
        let rolled_back = assert_atomic(&db, l0 as i64, l1 as i64);
        if rolled_back {
            mid += 1;
        } else {
            after += 1;
        }
        println!(
            "A round {r:02}: kill -9 after {delay:4} ms, wal {wal:9} bytes -> {}",
            if rolled_back {
                "old schema intact (user_version 0, rolled back)"
            } else {
                "fully migrated (user_version 4)"
            }
        );
        // and it always comes up afterwards, fully migrated
        if r % 4 == 0 {
            let srv = Server::start(PORT_KILL, &dir, &[]);
            drop(srv);
            assert!(!assert_atomic(&db, l0 as i64, l1 as i64));
        }
    }
    println!("phase A: {mid} kills mid-migration (rolled back), {after} after commit");
    assert!(mid >= 4, "too few kills landed mid-migration ({mid})");

    // Phase B: the SAME database killed again and again, each attempt a bit
    // later, until a start finally completes: retries must converge.
    let dir = fresh_dir("kill-b");
    let db = dir.join("vectors.db");
    std::fs::copy(&pristine, &db).unwrap();
    let mut attempt = 0u64;
    loop {
        let delay = 20 + attempt * (full_ms as u64 / 8).max(10);
        let mut child = spawn(PORT_KILL, &dir, &[]);
        std::thread::sleep(Duration::from_millis(delay));
        child.kill().unwrap();
        child.wait().unwrap();
        let rolled_back = assert_atomic(&db, l0 as i64, l1 as i64);
        println!("B attempt {attempt}: kill -9 after {delay} ms -> rolled_back={rolled_back}");
        attempt += 1;
        if !rolled_back {
            break;
        }
        assert!(attempt < 40, "migration never completed");
    }

    // Finally: serve on the migrated DB; old CJK/emoji rows are searchable.
    let srv = Server::start(PORT_KILL, &dir, &[]);
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "萨格勒布", "limit": 3})),
    );
    assert_eq!(v["total"], 3, "{v}");
    let text = v["results"].as_str().unwrap();
    assert!(text.contains("萨格勒布"));
    let c = ro(&db);
    let want: String = c
        .query_row(
            "SELECT message_text FROM l0_conversations WHERE record_id='old-000007'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        want,
        mixed_sentence(7),
        "legacy row bytes changed by migration"
    );
    let (_, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "서울특별시", "limit": 2})),
    );
    assert_eq!(
        (v["total"].clone(), v["strategy"].clone()),
        (json!(2), json!("fts"))
    );
    let out = Command::new(bin())
        .args(["schema", db.to_str().unwrap()])
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("schema report after phase B:\n{report}");
    assert!(report.contains(&format!("user_version={SCHEMA_VERSION}")));
    println!("real_kill9_during_migration_loop: ok (port {PORT_KILL})");
}

/// Text that has bitten Unicode handling before: astral CJK, kana, hangul,
/// ZWJ families, skin tones, flags, combining marks, RTL, a lone keycap.
const ROUND_TRIP: [&str; 8] = [
    "我们明天去萨格勒布吗？🚆",
    "東京タワーで会いましょう 🗼",
    "서울특별시 날씨 좋네요 ☀️",
    "家族 👨‍👩‍👧‍👦 and 👍🏽 at the 🇭🇷 coast",
    "𠮷野家の牛丼 (astral U+20BB7)",
    "Ćevapi u Sarajevu, e\u{301} combining, שלום",
    "keycap 1️⃣ crab 🦀 rust",
    "纯中文没有空格的一句话用于测试分词效果",
];

#[test]
fn real_cjk_emoji_roundtrip_and_fts() {
    if !real() {
        return;
    }
    let dir = fresh_dir("cjk");
    let db = dir.join("vectors.db");
    let srv = Server::start(PORT_CJK, &dir, &[]);
    let base = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 86_400_000;
    for (i, t) in ROUND_TRIP.iter().enumerate() {
        let body = json!({
            "user_content": t, "assistant_content": "ok", "session_key": "cjk",
            "messages": [{"role": "user", "content": t, "timestamp": base + i as i64}],
        });
        let (s, v) = srv.call("POST", "/capture", Some(&body));
        assert_eq!((s, v["l0_recorded"].clone()), (200, json!(1)), "{t}");
    }
    // SQLite holds the exact bytes, in both the table and the FTS original column
    let c = ro(&db);
    let mut st = c
        .prepare("SELECT message_text FROM l0_conversations ORDER BY timestamp")
        .unwrap();
    let stored: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(stored, ROUND_TRIP.to_vec());
    let mut st = c
        .prepare("SELECT message_text_original FROM l0_fts ORDER BY CAST(timestamp AS INTEGER)")
        .unwrap();
    let fts: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(fts, ROUND_TRIP.to_vec());
    // JSONL shard holds the exact text too
    let jsonl: String = std::fs::read_dir(dir.join("conversations"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    for t in ROUND_TRIP {
        let found = jsonl
            .lines()
            .any(|l| serde_json::from_str::<Value>(l).unwrap()["content"] == json!(t));
        assert!(found, "JSONL lost {t}");
    }
    // FTS search: query -> expected original text in the result
    let cases: [(&str, usize); 9] = [
        ("萨格勒布", 0),
        ("格勒", 0),   // 2-gram inside a run
        ("タワー", 1), // katakana
        ("서울", 2),   // hangul 2-gram
        ("날씨", 2),
        ("𠮷野家", 4), // astral ideograph
        ("Ćevapi", 5),
        ("分词", 7), // inside an unspaced sentence
        ("rust", 6),
    ];
    for (q, idx) in cases {
        let (s, v) = srv.call(
            "POST",
            "/search/conversations",
            Some(&json!({"query": q, "session_key": "cjk"})),
        );
        let text = v["results"].as_str().unwrap_or("");
        assert_eq!(s, 200);
        assert!(
            text.contains(ROUND_TRIP[idx]),
            "query {q} did not return {:?}: {v}",
            ROUND_TRIP[idx]
        );
        println!(
            "FTS {q:>8} -> total {} incl. {:?}",
            v["total"], ROUND_TRIP[idx]
        );
    }
    // recall's exact-substring L0 path also returns the emoji text unchanged
    let (_, v) = srv.call(
        "POST",
        "/recall",
        Some(&json!({"query": "👨‍👩‍👧‍👦", "session_key": "cjk"})),
    );
    println!("recall emoji-only -> context {:?}", v["context"]);
    // emoji-only FTS query: no word characters -> no FTS query (gateway's
    // fallback tokenizer behaves the same); must be a clean 200, not an error
    let (s, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "🦀"})),
    );
    assert_eq!((s, v["total"].clone()), (200, json!(0)));
    // restart: still identical (WAL replay)
    drop(srv);
    let srv = Server::start(PORT_CJK, &dir, &[]);
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "家族", "session_key": "cjk"})),
    );
    assert!(v["results"].as_str().unwrap().contains(ROUND_TRIP[3]));
    println!("real_cjk_emoji_roundtrip_and_fts: ok (port {PORT_CJK})");
}

fn replay_body(session: &str, n: i64) -> Value {
    let t0 = 1_759_312_800_000i64; // 2025-10-01T12:00:00+02:00
    let msgs: Vec<Value> = (0..n)
        .map(|i| {
            json!({"role": if i % 2 == 0 {"user"} else {"assistant"},
                   "content": format!("replayed {i} {}", MIXED[(i % 12) as usize]),
                   "timestamp": t0 + i * 1000})
        })
        .collect();
    json!({"user_content": "x", "assistant_content": "y", "session_key": session,
           "import": true, "messages": msgs})
}

#[test]
fn real_import_replay_idempotent() {
    if !real() {
        return;
    }
    // default server: same payload, old timestamps dropped (unchanged)
    let dir = fresh_dir("import-default");
    let srv = Server::start(PORT_DEFAULT, &dir, &[]);
    let (s, v) = srv.call("POST", "/capture", Some(&replay_body("replay", 40)));
    assert_eq!((s, v["l0_recorded"].clone()), (200, json!(0)));
    assert!(v.get("import").is_none());
    drop(srv);
    println!("default server (port {PORT_DEFAULT}): 40 old-timestamp messages -> l0_recorded 0 (unchanged)");

    // import-enabled server on a pre-v3 fixture DB
    let dir = fresh_dir("import");
    let db = dir.join("vectors.db");
    pre_v3_fixture(&db, 500, 50);
    let srv = Server::start(PORT_IMPORT, &dir, &[("OPENMIND_MEMORY_ALLOW_IMPORT", "1")]);
    let (s, v) = srv.call("POST", "/capture", Some(&replay_body("replay", 40)));
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        v["import"],
        json!({"inserted": 40, "duplicates": 0, "skipped_no_timestamp": 0})
    );
    for round in 0..3 {
        let (_, v) = srv.call("POST", "/capture", Some(&replay_body("replay", 40)));
        assert_eq!(
            v["import"],
            json!({"inserted": 0, "duplicates": 40, "skipped_no_timestamp": 0}),
            "round {round}"
        );
    }
    // 8 concurrent replays of a longer overlapping batch: 20 new rows total
    let hs: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let (s, b) = http(
                    PORT_IMPORT,
                    "POST",
                    "/capture",
                    Some(&replay_body("replay", 60).to_string()),
                )
                .unwrap();
                assert_eq!(s, 200);
                serde_json::from_str::<Value>(&b).unwrap()["import"]["inserted"]
                    .as_i64()
                    .unwrap()
            })
        })
        .collect();
    let inserted: i64 = hs.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(
        inserted, 20,
        "concurrent replays must insert each message once"
    );
    let c = ro(&db);
    assert_eq!(
        q_i64(
            &c,
            "SELECT COUNT(*) FROM l0_conversations WHERE session_key='replay'"
        ),
        60
    );
    assert_eq!(
        q_i64(&c, "SELECT COUNT(*) FROM l0_fts WHERE session_key='replay'"),
        60
    );
    assert_eq!(
        q_i64(
            &c,
            "SELECT MIN(timestamp) FROM l0_conversations WHERE session_key='replay'"
        ),
        1_759_312_800_000
    );
    assert_eq!(q_i64(&c, "SELECT COUNT(*) FROM l0_conversations"), 560);
    let jsonl: usize = std::fs::read_dir(dir.join("conversations"))
        .unwrap()
        .map(|e| {
            std::fs::read_to_string(e.unwrap().path())
                .unwrap()
                .lines()
                .count()
        })
        .sum();
    assert_eq!(jsonl, 60, "JSONL must hold each imported message once");
    // live capture on the same server still uses the cursor
    let (_, v) = srv.call("POST", "/capture", Some(&json!({"user_content": "a", "assistant_content": "b",
        "session_key": "replay", "messages": [{"role": "user", "content": "old live", "timestamp": 1_000}]})));
    assert_eq!(v["l0_recorded"], 0);
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "replayed 萨格勒布", "session_key": "replay", "limit": 100})),
    );
    assert!(v["total"].as_i64().unwrap() >= 5, "{v}");
    println!("import (port {PORT_IMPORT}): 40 inserted, 3x40 duplicates, 8 concurrent x60 -> +20, rows 60/60 FTS, JSONL 60");
    println!("real_import_replay_idempotent: ok");
}

#[test]
fn real_live_8420_health_get_only() {
    if !real() || std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skip: live probe needs GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let host = std::env::var("GALION_VPS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    // exactly one read-only GET /health; nothing written on the VPS
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &host,
            "curl -s -m 5 -w '\\n%{http_code} %{time_total}' http://127.0.0.1:8420/health",
        ])
        .output()
        .expect("ssh");
    assert!(out.status.success(), "ssh probe failed");
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, tail) = text.rsplit_once('\n').unwrap();
    let v: Value = serde_json::from_str(body).expect("live /health is JSON");
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    println!("live GET /health -> {tail} (code, seconds); keys {keys:?}");
    println!(
        "live status={} implementation={} implementation_version={} read_only={} fts={}",
        v["status"], v["implementation"], v["implementation_version"], v["read_only"], v["fts"]
    );
    assert!(tail.starts_with("200 "));
}

/// Rollback/roll-forward with the previous binary (main 13a296c, the design
/// deployed on 8420 as 0.1.0): it must open a v4 database, read and write
/// it, and leave `user_version` alone; this binary then reopens it as current.
/// Gated on OPENMIND_MEMORY_PREV_BIN (the script builds it from 13a296c).
#[test]
fn real_rollback_to_previous_binary() {
    if !real() {
        return;
    }
    let Some(prev) = std::env::var_os("OPENMIND_MEMORY_PREV_BIN") else {
        eprintln!("skip: set OPENMIND_MEMORY_PREV_BIN (scripts/real/memory-schema.sh builds it)");
        return;
    };
    let dir = fresh_dir("rollback");
    let db = dir.join("vectors.db");
    pre_v3_fixture(&db, 300, 30);
    let ahead = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 86_400_000;
    let cap = |srv: &Server, text: &str, ts: i64| {
        let (_, v) = srv.call("POST", "/capture", Some(&json!({"user_content": text, "assistant_content": "ok",
            "session_key": "rb", "messages": [{"role": "user", "content": text, "timestamp": ts}]})));
        assert_eq!(v["l0_recorded"], 1, "{v}");
    };
    {
        let srv = Server::start(PORT_DEFAULT, &dir, &[]);
        cap(&srv, "new binary wrote 新的 🦀", ahead);
    }
    assert_eq!(q_i64(&ro(&db), "PRAGMA user_version"), SCHEMA_VERSION);
    // roll back: previous binary on the v4 DB
    {
        let mut cmd = Command::new(&prev);
        cmd.env_clear()
            .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
            .env("OPENMIND_MEMORY_PORT", PORT_DEFAULT.to_string())
            .env("OPENMIND_MEMORY_DATA_DIR", &dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let srv = Server {
            child: Some(cmd.spawn().unwrap()),
            port: PORT_DEFAULT,
        };
        let t0 = Instant::now();
        while http(PORT_DEFAULT, "GET", "/health", None)
            .map(|r| r.0)
            .unwrap_or(0)
            != 200
        {
            assert!(t0.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        let (_, h) = srv.call("GET", "/health", None);
        assert_eq!(
            (h["status"].clone(), h["fts"].clone()),
            (json!("ok"), json!(true)),
            "{h}"
        );
        let (_, v) = srv.call(
            "POST",
            "/search/conversations",
            Some(&json!({"query": "新的", "session_key": "rb"})),
        );
        assert_eq!(v["total"], 1, "previous binary cannot read v4 rows: {v}");
        cap(&srv, "previous binary wrote 旧的 👍🏽", ahead + 10);
        println!(
            "previous binary on v4 DB: health {} fts {}, read + write ok",
            h["status"], h["fts"]
        );
    }
    let c = ro(&db);
    assert_eq!(
        q_i64(&c, "PRAGMA user_version"),
        SCHEMA_VERSION,
        "previous binary changed user_version"
    );
    assert_eq!(q_i64(&c, "SELECT COUNT(*) FROM l0_conversations"), 302);
    drop(c);
    // roll forward again
    let srv = Server::start(PORT_DEFAULT, &dir, &[]);
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "旧的", "session_key": "rb"})),
    );
    assert_eq!(v["total"], 1);
    println!("real_rollback_to_previous_binary: ok (port {PORT_DEFAULT}, user_version stays {SCHEMA_VERSION})");
}
