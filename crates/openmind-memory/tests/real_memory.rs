// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life tests: start the real `openmind-memory` binary on 127.0.0.1
//! (swarm slot 09 ports 19090-19099) with a fresh FIXTURE database and hit
//! its HTTP routes over real sockets. Gated: they run only with
//! `GALION_REAL=1` (plus `GALION_REAL_VPS=1` for the read-only live probe).
//! Use `scripts/real/memory-real.sh`, which builds release and runs these
//! with `--test-threads=1 --nocapture`. Synthetic data only.

use openmind_memory::store::Store;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT_ROUTES: u16 = 19090;
const PORT_AUTH: u16 = 19091;
const PORT_MIGRATE: u16 = 19092;
const PORT_CRASH: u16 = 19093;
const PORT_PERF: u16 = 19094;
const PORT_RO: u16 = 19095;
/// A fixture Bearer value for the box-only auth test. Not a real secret.
const FIXTURE_KEY: &str = "fixture-key-not-a-secret";

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1 (see scripts/real/memory-real.sh)");
    }
    on
}

fn bin() -> PathBuf {
    std::env::var_os("OPENMIND_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openmind-memory")))
}

fn fresh_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("real-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Server {
    child: Option<Child>,
    port: u16,
}

impl Server {
    fn start(port: u16, dir: &Path, env: &[(&str, &str)]) -> Server {
        let mut cmd = Command::new(bin());
        for k in [
            "OPENMIND_MEMORY_API_KEY",
            "TDAI_GATEWAY_API_KEY",
            "OPENMIND_MEMORY_READ_ONLY",
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
        let child = cmd.spawn().expect("spawn openmind-memory");
        let s = Server {
            child: Some(child),
            port,
        };
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(10) {
            if let Ok((200, _, _)) = try_http(port, "GET", "/health", None, &[]) {
                return s;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("server on {port} did not become healthy");
    }

    /// SIGKILL (std's Child::kill is SIGKILL on Unix), like `kill -9`.
    fn kill9(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let b = body.map(Value::to_string);
        let (s, text, _) = try_http(self.port, method, path, b.as_deref(), &[]).unwrap();
        (s, parse(&text))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill9();
    }
}

fn parse(text: &str) -> Value {
    if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(text).unwrap_or(Value::String(text.into()))
    }
}

/// One HTTP/1.1 request on a fresh connection. Returns (status, body, latency).
fn try_http(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
    headers: &[(&str, &str)],
) -> std::io::Result<(u16, String, Duration)> {
    let t0 = Instant::now();
    let mut s = TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(Duration::from_secs(15)))?;
    let body = body.unwrap_or("");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let dt = t0.elapsed();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Ok((status, rest.to_string(), dt))
}

/// Key paths -> JSON type names, values dropped (shape only).
fn shape(v: &Value) -> Vec<(String, &'static str)> {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<(String, &'static str)>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    let p = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(&p, x, out);
                }
            }
            _ => out.push((
                prefix.to_string(),
                match v {
                    Value::Null => "null",
                    Value::Bool(_) => "boolean",
                    Value::Number(_) => "number",
                    Value::String(_) => "string",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                },
            )),
        }
    }
    let mut out = Vec::new();
    walk("", v, &mut out);
    out
}

/// `/health` shape read from live 127.0.0.1:8420 on 2026-10-20T19:05:32+02:00
/// (keys and types only).
const LIVE_HEALTH_SHAPE: [(&str, &str); 12] = [
    ("status", "string"),
    ("version", "string"),
    ("uptime", "number"),
    ("stores.vectorStore", "boolean"),
    ("stores.embeddingService", "boolean"),
    ("services.timerScanner", "null"),
    ("services.pipelineWorker", "null"),
    ("services.stateBackend", "string"),
    ("implementation", "string"),
    ("implementation_version", "string"),
    ("read_only", "boolean"),
    ("fts", "boolean"),
];

/// GET routes probed on live 8420 (Bearer-protected deployment), with the
/// status seen there on 2026-10-20T19:05:32+02:00.
const LIVE_GET_CONTRACT: [(&str, u16); 10] = [
    ("/health", 200),
    ("/", 401),
    ("/recall", 401),
    ("/search/memories", 401),
    ("/search/conversations", 401),
    ("/session/end", 401),
    ("/capture", 401),
    ("/v2/offload/query-mmd", 401),
    ("/no-such-route", 401),
    ("/health/", 401),
];

const WORDS: [&str; 24] = [
    "zagreb", "split", "rijeka", "osijek", "zadar", "pula", "kiwi", "mango", "rust", "sqlite",
    "falcon", "harbor", "violet", "copper", "lantern", "meadow", "glacier", "orbit", "pepper",
    "quartz", "saddle", "timber", "willow", "yonder",
];

/// Deterministic fixture sentence number `i` (LCG, no rand crate).
fn sentence(i: u64) -> String {
    let mut x = i
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut w = Vec::new();
    for _ in 0..8 {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        w.push(WORDS[((x >> 33) % WORDS.len() as u64) as usize]);
    }
    format!("fixture note {i} {}", w.join(" "))
}

/// Epoch ms one day ahead: /capture keeps only messages newer than the
/// session cursor, which starts at the request time (gateway parity).
fn ahead_ms() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    now + 86_400_000
}

fn capture_body(session: &str, ts: i64, user: &str, assistant: Option<&str>) -> Value {
    let mut msgs = vec![json!({"role": "user", "content": user, "timestamp": ts})];
    if let Some(a) = assistant {
        msgs.push(json!({"role": "assistant", "content": a, "timestamp": ts + 1}));
    }
    json!({
        "user_content": user,
        "assistant_content": assistant.unwrap_or("ok"),
        "session_key": session,
        "messages": msgs,
    })
}

fn ro(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn schema_dump(c: &Connection) -> Vec<(String, String)> {
    let mut st = c
        .prepare("SELECT name, COALESCE(sql,'') FROM sqlite_master ORDER BY name")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
}

fn columns(c: &Connection, table: &str) -> Vec<String> {
    let mut st = c
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect()
}

fn is_js_iso_utc(s: &str) -> bool {
    // JS toISOString() shape, the gateway's stored format: 24 chars, ms, UTC suffix
    let b = s.as_bytes();
    b.len() == 24
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'.'
        && b[23] == b'Z'
        && s.chars().filter(char::is_ascii_digit).count() == 17
}

/// Seed a fresh FIXTURE database through the library (the binary has no L1
/// write route: L1 rows come from the Node pipeline, `/seed` is 501).
fn seed_l1(dir: &Path, n: u64) {
    let store = Store::open(&dir.join("vectors.db"), false).unwrap();
    for i in 0..n {
        let kind = ["episodic", "persona", "instruction"][(i % 3) as usize];
        store
            .insert_l1(
                &format!("l1-{i:05}"),
                &sentence(i),
                kind,
                50,
                &format!("Scene {}", i % 7),
                "fixture-session",
                "2026-10-20T19:00:00+02:00",
                "{}",
            )
            .unwrap();
    }
}

#[test]
fn real_every_route_on_fixture_db() {
    if !real() {
        return;
    }
    let dir = fresh_dir("routes");
    // Fixture: 20 strong "kiwi" episodic rows and one weak instruction row,
    // so a type filter applied only after "take limit*3" would miss it.
    {
        let store = Store::open(&dir.join("vectors.db"), false).unwrap();
        for i in 0..20 {
            store
                .insert_l1(
                    &format!("e{i:02}"),
                    "kiwi kiwi kiwi",
                    "episodic",
                    50,
                    "Orchard",
                    "fixture-session",
                    "",
                    "{}",
                )
                .unwrap();
        }
        store
            .insert_l1(
                "i00",
                "kiwi plus a much longer sentence about harbor lanterns and copper meadows",
                "instruction",
                10,
                "Harbor Log",
                "fixture-session",
                "",
                "{}",
            )
            .unwrap();
    }
    let srv = Server::start(PORT_ROUTES, &dir, &[("MEMORY_MAX_BODY_BYTES", "4096")]);

    // health: same shape as live 8420
    let (s, h) = srv.call("GET", "/health", None);
    assert_eq!(s, 200);
    let got = shape(&h);
    let want: Vec<(String, &str)> = LIVE_HEALTH_SHAPE
        .iter()
        .map(|(k, t)| (k.to_string(), *t))
        .collect();
    assert_eq!(got, want, "health shape differs from live 8420");
    assert_eq!(h["status"], "ok");
    assert_eq!(h["fts"], true);

    // OPTIONS, 404, 501, invalid JSON, 413
    assert_eq!(
        try_http(srv.port, "OPTIONS", "/recall", None, &[])
            .unwrap()
            .0,
        204
    );
    assert_eq!(srv.call("GET", "/nope", None).0, 404);
    assert_eq!(srv.call("POST", "/seed", Some(&json!({}))).0, 501);
    let (s, _, _) = try_http(srv.port, "POST", "/capture", Some("{not json"), &[]).unwrap();
    assert_eq!(s, 400);
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(5000));
    assert_eq!(
        try_http(srv.port, "POST", "/capture", Some(&big), &[])
            .unwrap()
            .0,
        413
    );
    // v2 routes always need a Bearer + service id, even with auth disabled
    assert_eq!(
        srv.call("POST", "/v2/conversation/add", Some(&json!({}))).0,
        401
    );

    // save (capture)
    assert_eq!(
        srv.call("POST", "/capture", Some(&json!({"session_key": "a"})))
            .0,
        400
    );
    let ts: i64 = ahead_ms();
    let body = capture_body(
        "sess-a",
        ts,
        "Where is the copper lantern stored?",
        Some("The copper lantern is in the Zadar harbor shed."),
    );
    let (s, v) = srv.call("POST", "/capture", Some(&body));
    assert_eq!(
        (s, v.clone()),
        (200, json!({"l0_recorded": 2, "scheduler_notified": false}))
    );
    // same timestamps again: the per-session cursor drops them
    let (_, v) = srv.call("POST", "/capture", Some(&body));
    assert_eq!(v["l0_recorded"], 0);
    let other = capture_body("sess-b", ts + 10, "copper kettle in Pula", None);
    assert_eq!(
        srv.call("POST", "/capture", Some(&other)).1["l0_recorded"],
        1
    );

    // search conversations (+ session filter in SQL)
    let (s, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "copper", "limit": 5})),
    );
    assert_eq!(s, 200);
    assert_eq!(v["total"], 3);
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["results", "total"]);
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "copper", "limit": 1, "session_key": "sess-b"})),
    );
    assert_eq!(v["total"], 1);
    assert!(v["results"].as_str().unwrap().contains("Session: sess-b"));
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "copper", "session_key": "nobody"})),
    );
    assert_eq!(v["total"], 0);
    assert_eq!(
        srv.call("POST", "/search/conversations", Some(&json!({})))
            .0,
        400
    );

    // search memories: MEM-SEARCH documented key order results,total,strategy
    let (s, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "kiwi", "limit": 3})),
    );
    assert_eq!(s, 200);
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["results", "total", "strategy"]);
    assert_eq!(
        (v["total"].clone(), v["strategy"].clone()),
        (json!(3), json!("fts"))
    );
    // filter-before-limit (ported from the VPS rewrite)
    let (_, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "kiwi", "limit": 1, "type": "instruction"})),
    );
    assert_eq!(v["total"], 1, "type filter starved: {v}");
    let (_, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "kiwi", "limit": 2, "scene": "harbor"})),
    );
    assert_eq!(v["total"], 1);
    let (_, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "nothinghere"})),
    );
    assert_eq!(
        (v["total"].clone(), v["strategy"].clone()),
        (json!(0), json!("fts"))
    );
    // same query twice gives the same answer
    let a = srv
        .call(
            "POST",
            "/search/memories",
            Some(&json!({"query": "kiwi", "limit": 5})),
        )
        .1;
    let b = srv
        .call(
            "POST",
            "/search/memories",
            Some(&json!({"query": "kiwi", "limit": 5})),
        )
        .1;
    assert_eq!(a, b);

    // recall
    let (s, v) = srv.call(
        "POST",
        "/recall",
        Some(&json!({"query": "copper lantern", "session_key": "sess-a"})),
    );
    assert_eq!(s, 200);
    for k in ["context", "memory_count", "code", "message", "retryable"] {
        assert!(v.get(k).is_some(), "recall lacks {k}: {v}");
    }
    assert_eq!(
        srv.call("POST", "/recall", Some(&json!({"query": "x"}))).0,
        400
    );

    // sessions
    assert_eq!(
        srv.call(
            "POST",
            "/session/end",
            Some(&json!({"session_key": "sess-a"}))
        ),
        (200, json!({"flushed": true}))
    );
    assert_eq!(srv.call("POST", "/session/end", Some(&json!({}))).0, 400);

    // timestamps: stored ms kept exactly, recorded_at in the gateway's format
    let db = ro(&dir.join("vectors.db"));
    let mut st = db
        .prepare("SELECT role, timestamp, recorded_at FROM l0_conversations WHERE session_key='sess-a' ORDER BY timestamp")
        .unwrap();
    let rows: Vec<(String, i64, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].0.as_str(), rows[0].1), ("user", ts));
    assert_eq!((rows[1].0.as_str(), rows[1].1), ("assistant", ts + 1));
    assert!(
        rows.iter().all(|r| is_js_iso_utc(&r.2)),
        "recorded_at {rows:?}"
    );
    assert_eq!(count(&db, "l0_fts"), count(&db, "l0_conversations"));
    let shards: Vec<_> = std::fs::read_dir(dir.join("conversations"))
        .unwrap()
        .collect();
    assert_eq!(shards.len(), 1);
    let jsonl = std::fs::read_to_string(shards[0].as_ref().unwrap().path()).unwrap();
    let lines: Vec<Value> = jsonl
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(lines
        .iter()
        .all(|l| is_js_iso_utc(l["recordedAt"].as_str().unwrap())));
    assert_eq!(lines[0]["timestamp"], ts);
    println!("real_every_route_on_fixture_db: ok (port {PORT_ROUTES})");
}

#[test]
fn real_auth_get_contract_matches_live() {
    if !real() {
        return;
    }
    let dir = fresh_dir("auth");
    let srv = Server::start(PORT_AUTH, &dir, &[("OPENMIND_MEMORY_API_KEY", FIXTURE_KEY)]);
    for (path, want) in LIVE_GET_CONTRACT {
        let (s, body, _) = try_http(srv.port, "GET", path, None, &[]).unwrap();
        assert_eq!(s, want, "GET {path}");
        let keys: Vec<String> = parse(&body)
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        let want_keys: Vec<&str> = match (path, s) {
            (_, 200) => keys.iter().map(String::as_str).collect(),
            ("/v2/offload/query-mmd", _) => vec!["code", "message", "request_id"],
            _ => vec!["error"],
        };
        assert_eq!(keys, want_keys, "GET {path} body keys");
    }
    let bearer = format!("Bearer {FIXTURE_KEY}");
    let auth = [("Authorization", bearer.as_str())];
    assert_eq!(
        try_http(srv.port, "GET", "/recall", None, &auth).unwrap().0,
        404
    );
    let wrong = [("Authorization", "Bearer wrong")];
    assert_eq!(
        try_http(srv.port, "POST", "/recall", Some("{}"), &wrong)
            .unwrap()
            .0,
        401
    );
    println!("real_auth_get_contract_matches_live: ok (port {PORT_AUTH})");
}

const V3_TABLES: [&str; 6] = [
    "embedding_meta",
    "l0_conversations",
    "l0_fts",
    "l1_fts",
    "l1_records",
    "idx_l0_timestamp",
];

#[test]
fn real_migrations_empty_idempotent_and_pre_v3() {
    if !real() {
        return;
    }
    // (a) up from an empty data dir
    let dir = fresh_dir("migrate-empty");
    let db_path = dir.join("vectors.db");
    let mut srv = Server::start(PORT_MIGRATE, &dir, &[]);
    srv.call(
        "POST",
        "/capture",
        Some(&capture_body(
            "m",
            ahead_ms(),
            "migration probe alpha",
            Some("beta reply"),
        )),
    );
    srv.kill9();
    let first = schema_dump(&ro(&db_path));
    let names: Vec<&str> = first.iter().map(|(n, _)| n.as_str()).collect();
    for t in V3_TABLES {
        assert!(names.contains(&t), "missing {t} in {names:?}");
    }
    assert!(
        !names.iter().any(|n| n.contains("vec")),
        "vec0 tables must not be created"
    );
    assert!(
        !names.contains(&"schema_migrations"),
        "the GH design keeps no schema_migrations table"
    );
    assert!(columns(&ro(&db_path), "l0_conversations").contains(&"timestamp".to_string()));
    let jm: String = ro(&db_path)
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(jm, "wal");

    // (b) idempotence: three more restarts change neither schema nor rows
    for _ in 0..3 {
        let mut s = Server::start(PORT_MIGRATE, &dir, &[]);
        s.kill9();
        let c = ro(&db_path);
        assert_eq!(schema_dump(&c), first);
        assert_eq!(count(&c, "l0_conversations"), 2);
        assert_eq!(count(&c, "l0_fts"), 2);
    }

    // (c) pre-v3 database (l0_conversations without `timestamp`)
    let dir = fresh_dir("migrate-prev3");
    let db_path = dir.join("vectors.db");
    {
        let c = Connection::open(&db_path).unwrap();
        c.execute_batch(
            "CREATE TABLE l0_conversations (record_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, \
             session_id TEXT DEFAULT '', role TEXT NOT NULL DEFAULT '', message_text TEXT NOT NULL, \
             recorded_at TEXT DEFAULT '');
             INSERT INTO l0_conversations VALUES ('old-1','old','old','user','legacy zadar row','2026-10-01T10:00:00.000Z');",
        )
        .unwrap();
    }
    let srv = Server::start(PORT_MIGRATE, &dir, &[]);
    let c = ro(&db_path);
    assert!(columns(&c, "l0_conversations").contains(&"timestamp".to_string()));
    let (ts, txt): (i64, String) = c
        .query_row(
            "SELECT timestamp, message_text FROM l0_conversations WHERE record_id='old-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((ts, txt.as_str()), (0, "legacy zadar row"));
    let (_, v) = srv.call(
        "POST",
        "/capture",
        Some(&capture_body("new", ahead_ms(), "fresh zadar row", None)),
    );
    assert_eq!(v["l0_recorded"], 1);
    // l0_fts is empty for legacy rows, so the LIKE fallback is not used once it has rows;
    // the FTS path finds the new row.
    let (_, v) = srv.call(
        "POST",
        "/search/conversations",
        Some(&json!({"query": "zadar"})),
    );
    assert_eq!(v["total"], 1);
    drop(srv);

    // (d) legacy v1 FTS (no *_original columns): opens, fts=false, writes still land
    let dir = fresh_dir("migrate-v1fts");
    {
        let c = Connection::open(dir.join("vectors.db")).unwrap();
        c.execute_batch("CREATE VIRTUAL TABLE l1_fts USING fts5(content, record_id UNINDEXED);
                         CREATE VIRTUAL TABLE l0_fts USING fts5(message_text, record_id UNINDEXED);").unwrap();
    }
    let srv = Server::start(PORT_MIGRATE, &dir, &[]);
    let (_, h) = srv.call("GET", "/health", None);
    assert_eq!(
        (h["status"].clone(), h["fts"].clone()),
        (json!("ok"), json!(false))
    );
    let (_, v) = srv.call(
        "POST",
        "/capture",
        Some(&capture_body("v1", ahead_ms(), "legacy fts write", None)),
    );
    assert_eq!(v["l0_recorded"], 1);
    let (s, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "legacy"})),
    );
    assert_eq!((s, v["strategy"].clone()), (200, json!("none")));
    drop(srv);
    assert_eq!(count(&ro(&dir.join("vectors.db")), "l0_conversations"), 1);
    println!("real_migrations_empty_idempotent_and_pre_v3: ok (port {PORT_MIGRATE})");
}

#[test]
fn real_read_only_mode() {
    if !real() {
        return;
    }
    let dir = fresh_dir("ro-missing");
    let srv = Server::start(PORT_RO, &dir, &[("OPENMIND_MEMORY_READ_ONLY", "1")]);
    let (_, h) = srv.call("GET", "/health", None);
    assert_eq!(
        (h["status"].clone(), h["read_only"].clone()),
        (json!("degraded"), json!(true))
    );
    drop(srv);
    assert!(
        !dir.join("vectors.db").exists(),
        "read-only must not create a db"
    );

    let dir = fresh_dir("ro-existing");
    seed_l1(&dir, 30);
    let before = std::fs::read(dir.join("vectors.db")).unwrap();
    let srv = Server::start(PORT_RO, &dir, &[("OPENMIND_MEMORY_READ_ONLY", "1")]);
    let (_, v) = srv.call(
        "POST",
        "/search/memories",
        Some(&json!({"query": "zagreb", "limit": 2})),
    );
    assert_eq!(v["strategy"], "fts");
    srv.call(
        "POST",
        "/capture",
        Some(&capture_body("ro", ahead_ms(), "must not be stored", None)),
    );
    drop(srv);
    assert_eq!(count(&ro(&dir.join("vectors.db")), "l0_conversations"), 0);
    assert_eq!(std::fs::read(dir.join("vectors.db")).unwrap(), before);
    assert!(!dir.join("conversations").exists());
    println!("real_read_only_mode: ok (port {PORT_RO})");
}

#[test]
fn real_crash_recovery_kill9_mid_write() {
    if !real() {
        return;
    }
    let dir = fresh_dir("crash");
    let db_path = dir.join("vectors.db");
    let mut acked_total = 0i64;
    for round in 0..3u64 {
        let mut srv = Server::start(PORT_CRASH, &dir, &[]);
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writers: Vec<_> = (0..4u64)
            .map(|w| {
                let stop = stop.clone();
                let base = ahead_ms();
                std::thread::spawn(move || {
                    let mut acked = 0i64;
                    let mut i = 0i64;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let ts = base + i * 10;
                        let body = capture_body(
                            &format!("crash-r{round}-w{w}"),
                            ts,
                            &format!("crash round {round} writer {w} item {i} quartz"),
                            Some("ack willow"),
                        );
                        match try_http(PORT_CRASH, "POST", "/capture", Some(&body.to_string()), &[])
                        {
                            Ok((200, b, _)) => {
                                acked += parse(&b)["l0_recorded"].as_i64().unwrap_or(0)
                            }
                            _ => break,
                        }
                        i += 1;
                    }
                    acked
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(700));
        srv.kill9(); // kill -9 while 4 writers are mid-request
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let acked: i64 = writers.into_iter().map(|h| h.join().unwrap()).sum();
        acked_total += acked;
        assert!(acked > 0, "round {round}: no writes landed before the kill");

        // integrity before restart (WAL replay happens on open)
        {
            let c = Connection::open(&db_path).unwrap();
            let ic: String = c
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .unwrap();
            assert_eq!(ic, "ok");
            c.execute("INSERT INTO l0_fts(l0_fts) VALUES('integrity-check')", [])
                .unwrap();
            let (rows, fts) = (count(&c, "l0_conversations"), count(&c, "l0_fts"));
            assert_eq!(rows, fts, "row/fts mismatch after kill -9 (tx not atomic)");
            assert!(
                rows >= acked_total,
                "acked {acked_total} but only {rows} rows survived"
            );
            println!("crash round {round}: acked {acked} (total {acked_total}), rows after kill -9 {rows}, integrity ok");
        }
        // restart: healthy, searchable, writable
        let srv = Server::start(PORT_CRASH, &dir, &[]);
        let (_, v) = srv.call(
            "POST",
            "/search/conversations",
            Some(&json!({"query": "quartz", "limit": 1})),
        );
        assert_eq!(v["total"], 1);
        let (_, v) = srv.call(
            "POST",
            "/capture",
            Some(&capture_body(
                &format!("after-{round}"),
                ahead_ms(),
                "post crash write",
                None,
            )),
        );
        assert_eq!(v["l0_recorded"], 1);
        acked_total += 1;
    }
    println!("real_crash_recovery_kill9_mid_write: ok (port {PORT_CRASH})");
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    let i = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn summary(name: &str, mut v: Vec<f64>) -> String {
    v.sort_by(f64::total_cmp);
    format!(
        "| {name} | {} | {:.3} | {:.3} | {:.3} | {:.3} |",
        v.len(),
        pct(&v, 50.0),
        pct(&v, 95.0),
        pct(&v, 99.0),
        v[v.len() - 1]
    )
}

#[test]
fn real_perf_baseline_10k() {
    if !real() {
        return;
    }
    let n: u64 = std::env::var("GALION_REAL_PERF_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let dir = fresh_dir("perf");
    let t = Instant::now();
    seed_l1(&dir, n);
    let seed_s = t.elapsed().as_secs_f64();
    let _srv = Server::start(PORT_PERF, &dir, &[]);
    let base = ahead_ms();
    let mut save = Vec::with_capacity(n as usize);
    for i in 0..n {
        let body = capture_body(
            &format!("perf-{}", i % 50),
            base + i as i64,
            &sentence(i + 100_000),
            None,
        );
        let (s, b, dt) =
            try_http(PORT_PERF, "POST", "/capture", Some(&body.to_string()), &[]).unwrap();
        assert_eq!((s, parse(&b)["l0_recorded"].as_i64()), (200, Some(1)));
        save.push(dt.as_secs_f64() * 1000.0);
    }
    let mut s_mem = Vec::new();
    let mut s_conv = Vec::new();
    let mut s_conv_sess = Vec::new();
    for i in 0..1000u64 {
        let q = format!(
            "{} {}",
            WORDS[(i % 24) as usize],
            WORDS[((i * 7 + 3) % 24) as usize]
        );
        let (s, _, dt) = try_http(
            PORT_PERF,
            "POST",
            "/search/memories",
            Some(&json!({"query": q, "limit": 5}).to_string()),
            &[],
        )
        .unwrap();
        assert_eq!(s, 200);
        s_mem.push(dt.as_secs_f64() * 1000.0);
        let (s, _, dt) = try_http(
            PORT_PERF,
            "POST",
            "/search/conversations",
            Some(&json!({"query": q, "limit": 5}).to_string()),
            &[],
        )
        .unwrap();
        assert_eq!(s, 200);
        s_conv.push(dt.as_secs_f64() * 1000.0);
        let (s, _, dt) = try_http(
            PORT_PERF,
            "POST",
            "/search/conversations",
            Some(
                &json!({"query": q, "limit": 5, "session_key": format!("perf-{}", i % 50)})
                    .to_string(),
            ),
            &[],
        )
        .unwrap();
        assert_eq!(s, 200);
        s_conv_sess.push(dt.as_secs_f64() * 1000.0);
    }
    let db = ro(&dir.join("vectors.db"));
    assert_eq!(count(&db, "l0_conversations") as u64, n);
    assert_eq!(count(&db, "l1_fts") as u64, n);
    let size = std::fs::metadata(dir.join("vectors.db"))
        .map(|m| m.len())
        .unwrap_or(0);
    println!("PERF fixture: {n} L1 rows seeded in {seed_s:.2}s via library, {n} L0 rows saved over HTTP; vectors.db {size} bytes (+WAL)");
    println!("| op (ms, fresh TCP connection per request) | n | p50 | p95 | p99 | max |");
    println!("|---|---|---|---|---|---|");
    println!("{}", summary("save POST /capture (1 L0 row)", save));
    println!(
        "{}",
        summary("search POST /search/memories (L1, 10k)", s_mem)
    );
    println!(
        "{}",
        summary("search POST /search/conversations (L0, 10k)", s_conv)
    );
    println!(
        "{}",
        summary("search /search/conversations + session_key", s_conv_sess)
    );
}

/// READ-ONLY contract probe of live 127.0.0.1:8420 over ssh: GET only,
/// status codes, latency and key/type shape. Bodies are never printed.
#[test]
fn real_live_8420_get_contract_readonly() {
    if !real() || std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skip: live probe needs GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let host = std::env::var("GALION_VPS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let paths: Vec<&str> = LIVE_GET_CONTRACT.iter().map(|(p, _)| *p).collect();
    let mut script = String::new();
    for p in &paths {
        script.push_str(&format!(
            "printf '%s ' '{p}'; curl -s -o /dev/null -w '%{{http_code}} %{{time_total}}\\n' -X GET 'http://127.0.0.1:8420{p}';"
        ));
    }
    script.push_str("for i in $(seq 1 20); do curl -s -o /dev/null -w 'H %{time_total}\\n' http://127.0.0.1:8420/health; done;");
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            &host,
            &script,
        ])
        .output()
        .expect("ssh");
    assert!(out.status.success(), "ssh probe failed");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lat = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.first() == Some(&"H") {
            lat.push(f[1].parse::<f64>().unwrap() * 1000.0);
            continue;
        }
        let want = LIVE_GET_CONTRACT
            .iter()
            .find(|(p, _)| *p == f[0])
            .unwrap()
            .1;
        let code: u16 = f[1].parse().unwrap();
        println!(
            "live GET {} -> {code} in {:.3} ms",
            f[0],
            f[2].parse::<f64>().unwrap() * 1000.0
        );
        assert_eq!(code, want, "live contract drift on GET {}", f[0]);
    }
    println!("{}", summary("live GET /health over loopback (20x)", lat));
    // shape only: the health body is parsed in memory and only key/type pairs kept
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            &host,
            "curl -s http://127.0.0.1:8420/health",
        ])
        .output()
        .expect("ssh");
    let v: Value = serde_json::from_slice(&out.stdout).expect("live health is JSON");
    let got = shape(&v);
    let want: Vec<(String, &str)> = LIVE_HEALTH_SHAPE
        .iter()
        .map(|(k, t)| (k.to_string(), *t))
        .collect();
    assert_eq!(got, want, "live /health shape drifted");
    println!(
        "live /health shape: {} keys, identical to the box binary",
        got.len()
    );
}
