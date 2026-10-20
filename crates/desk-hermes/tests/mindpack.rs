// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// desk-hermes behaviour on a fixture Hermes home: every aggregate, the
// read-only guarantee, damaged inputs, and the binary's stdout.

mod fixture;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("test-out/desk-hermes")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pack(home: &Path) -> Value {
    desk_hermes::mindpack(home, &home.join("state.db"))
}

const SKELETON: &str = r#"{"context":{"cached_sessions":0,"cached_turns":0,"cache_bytes":0,"model_window":0},"cache":{"read":0,"write":0,"input":0,"output":0,"reasoning":0,"files":0,"file_bytes":0},"history":[],"sessions":[],"mind":{"line":"","sessions":0,"messages":0},"tools":[],"models":[],"roles":{"user":0,"assistant":0,"tool":0},"road":[],"stores":[],"recent":[],"cached":[],"tasks":[]}"#;

#[test]
fn empty_home_is_the_exact_skeleton() {
    let home = scratch("empty");
    assert_eq!(desk_hermes::render(&pack(&home)), SKELETON);
    assert!(
        !home.join("state.db").exists(),
        "read-only open must not create state.db"
    );
}

#[test]
fn fixture_context_and_cache() {
    let home = scratch("fixture-a");
    fixture::build(&home);
    let v = pack(&home);
    let cache_bytes = std::fs::metadata(home.join("context-cache.json"))
        .unwrap()
        .len();
    assert_eq!(
        v["context"],
        json!({"cached_sessions": 3, "cached_turns": 5, "cache_bytes": cache_bytes, "model_window": 32768})
    );
    assert_eq!(
        v["cached"],
        json!([
            {"id": "sessA", "model": "galion:7b", "turns": 3},
            {"id": "sessB", "model": "", "turns": 2},
            {"id": "sessC", "model": "", "turns": 0}
        ])
    );
    assert_eq!(
        v["cache"],
        json!({"read": 510, "write": 5, "input": 1300, "output": 120, "reasoning": 8, "files": 4, "file_bytes": 190})
    );
    assert_eq!(
        v["stores"],
        json!([
            {"name": "images", "n": 2, "bytes": 150},
            {"name": "docs", "n": 1, "bytes": 30},
            {"name": "catalog", "n": 1, "bytes": 10},
            {"name": "models.dev", "n": 1, "bytes": 20}
        ])
    );
    assert_eq!(
        v["recent"],
        json!([
            {"name": "images/a.png", "bytes": 100, "at": 1700001003},
            {"name": "images/b.png", "bytes": 50, "at": 1700001002},
            {"name": "docs/x.txt", "bytes": 30, "at": 1700001001},
            {"name": "catalog.json", "bytes": 10, "at": 1700001000}
        ])
    );
}

#[test]
fn fixture_database_aggregates() {
    let home = scratch("fixture-b");
    fixture::build(&home);
    let v = pack(&home);
    assert_eq!(v["mind"]["sessions"], 3);
    assert_eq!(v["mind"]["messages"], 5);
    let road = json!([
        {"id": "s2", "source": "api", "model": "gpt-x", "messages": 2, "at": 1700000200},
        {"id": "s1", "source": "cli", "model": "galion:7b", "messages": 3, "at": 1700000020}
    ]);
    assert_eq!(
        v["road"], road,
        "s3 has no messages and is left out; ms stamps are seconds"
    );
    assert_eq!(v["sessions"], road);
    assert_eq!(v["tools"], json!([{"name": "clock", "n": 1}]));
    assert_eq!(
        v["models"],
        json!([
            {"name": "gpt-x", "calls": 1, "input": 1000, "output": 10, "read": 500, "reasoning": 7},
            {"name": "galion:7b", "calls": 2, "input": 300, "output": 110, "read": 10, "reasoning": 1}
        ])
    );
    assert_eq!(v["roles"], json!({"user": 2, "assistant": 2, "tool": 1}));
    assert_eq!(
        v["tasks"],
        json!([
            {"id": "one-galion", "state": "running", "title": "# Ship one", "at": 0},
            {"id": "d2", "state": "running", "title": "", "at": 1700000400},
            {"id": "d1", "state": "done", "title": "", "at": 1700000300}
        ])
    );
}

#[test]
fn fixture_history_and_memory_line() {
    let home = scratch("fixture-c");
    fixture::build(&home);
    let v = pack(&home);
    assert_eq!(
        v["history"],
        json!([
            {"at": 1700000500, "kind": "chat", "text": "hello there world"},
            {"at": 1700000600, "kind": "tool", "text": "ms stamp"},
            {"at": 1700000700, "kind": "", "text": "y".repeat(90)},
            {"at": 0, "kind": "cron", "text": ""},
            {"at": 0, "kind": "note", "text": "Café"}
        ])
    );
    assert_eq!(v["mind"]["line"], "Mate builds Galion. Second line");
    let rendered = desk_hermes::render(&v);
    assert!(
        rendered.contains(r#""text":"Caf\u00e9""#),
        "ensure_ascii escaping"
    );
    assert!(!rendered.contains(": "), "compact separators");
}

#[test]
fn state_db_is_never_written() {
    let home = scratch("readonly");
    fixture::build(&home);
    let db = home.join("state.db");
    let before = std::fs::read(&db).unwrap();
    let mtime = std::fs::metadata(&db).unwrap().modified().unwrap();
    for _ in 0..3 {
        let _ = pack(&home);
    }
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert_eq!(std::fs::metadata(&db).unwrap().modified().unwrap(), mtime);
    for side in ["state.db-wal", "state.db-journal", "state.db-shm"] {
        assert!(!home.join(side).exists(), "{side} created");
    }
}

#[test]
fn damaged_inputs_keep_the_skeleton_values() {
    let home = scratch("damaged");
    std::fs::write(home.join("context-cache.json"), "{not json").unwrap();
    std::fs::write(home.join("state.db"), "this is not sqlite").unwrap();
    std::fs::write(home.join("activity.jsonl"), "{\"at\":1}\nbroken line\n").unwrap();
    std::fs::write(home.join("context_length_cache.yaml"), "none here").unwrap();
    assert_eq!(
        desk_hermes::render(&pack(&home)),
        SKELETON,
        "garbage in, skeleton out"
    );

    // A database with only some tables fills what it can.
    let partial = scratch("partial");
    let conn = rusqlite::Connection::open(partial.join("state.db")).unwrap();
    conn.execute_batch("create table sessions (id text, source text, model text); insert into sessions values ('a','b','c');")
        .unwrap();
    drop(conn);
    let v = pack(&partial);
    assert_eq!(v["mind"]["sessions"], 1);
    assert_eq!(v["mind"]["messages"], 0);
    assert_eq!(v["road"], json!([]));
    assert_eq!(v["cache"]["input"], 0);
}

#[test]
fn output_is_deterministic() {
    let home = scratch("determinism");
    fixture::build(&home);
    assert_eq!(
        desk_hermes::render(&pack(&home)),
        desk_hermes::render(&pack(&home))
    );
}

#[test]
fn paths_follow_env() {
    let (home, db) = desk_hermes::paths(&|_| None);
    assert_eq!(home, PathBuf::from("/opt/galion/hermes"));
    assert_eq!(db, PathBuf::from("/opt/galion/hermes/state.db"));
    let (home, db) = desk_hermes::paths(&|k| match k {
        "HERMES_HOME" => Some("/tmp/h".into()),
        _ => None,
    });
    assert_eq!(
        (home.clone(), db),
        (PathBuf::from("/tmp/h"), PathBuf::from("/tmp/h/state.db"))
    );
    let (_, db) = desk_hermes::paths(&|k| match k {
        "HERMES_STATE_DB" => Some("/tmp/other.db".into()),
        _ => None,
    });
    assert_eq!(db, PathBuf::from("/tmp/other.db"));
}

#[test]
fn binary_prints_one_json_line() {
    let home = scratch("binary");
    fixture::build(&home);
    let out = Command::new(env!("CARGO_BIN_EXE_desk-hermes"))
        .env("HERMES_HOME", &home)
        .env_remove("HERMES_STATE_DB")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.ends_with("}\n"));
    assert_eq!(text.lines().count(), 1);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v, pack(&home));
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "context", "cache", "history", "sessions", "mind", "tools", "models", "roles", "road",
            "stores", "recent", "cached", "tasks"
        ]
    );
}
