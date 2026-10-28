// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// A deterministic Hermes home for tests and for the real-life script:
// context-cache.json, context_length_cache.yaml, a file cache with fixed
// mtimes, activity.jsonl, MEMORY.md, a task, and a state.db with the tables
// desk-hermes reads. The numbers are chosen so every aggregate in the
// mindpack has one known answer (see tests/mindpack.rs).

#![allow(dead_code)]

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

pub const SCHEMA: &str = "
create table sessions (id text primary key, source text, model text);
create table messages (id integer primary key, session_id text, role text, tool_name text, timestamp real);
create table session_model_usage (session_id text, model text, input_tokens integer, output_tokens integer,
    cache_read_tokens integer, cache_write_tokens integer, reasoning_tokens integer);
create table async_delegations (delegation_id text, state text, dispatched_at real);
";

pub const DATA: &str = "
insert into sessions values ('s1','cli','galion:7b'), ('s2','api','gpt-x'), ('s3','cli','idle');
insert into messages (session_id, role, tool_name, timestamp) values
    ('s1','user',null,1700000000), ('s1','assistant',null,1700000010), ('s1','tool','clock',1700000020),
    ('s2','user',null,1700000100000), ('s2','assistant','',1700000200000);
insert into session_model_usage values
    ('s1','galion:7b',100,50,10,5,1), ('s1','galion:7b',200,60,0,0,0), ('s2','gpt-x',1000,10,500,0,7);
insert into async_delegations values ('d1','done',1700000300), ('d2','running',1700000400500);
";

fn write(path: &Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn set_mtime(path: &Path, secs: u64) {
    let f = File::options().write(true).open(path).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

/// Build the fixture home in `home` (created; existing files overwritten).
pub fn build(home: &Path) {
    fs::create_dir_all(home).unwrap();
    write(
        &home.join("context-cache.json"),
        br#"{"sessA": {"model": "galion:7b", "turns": [1, 2, 3]}, "sessB": {"model": null, "turns": "ab"}, "sessC": 5}"#,
    );
    write(
        &home.join("context_length_cache.yaml"),
        b"models:\n  galion:7b: 32768\n",
    );
    write(&home.join("cache/catalog.json"), &[b'c'; 10]);
    write(&home.join("cache/images/a.png"), &[b'a'; 100]);
    write(&home.join("cache/images/b.png"), &[b'b'; 50]);
    write(&home.join("cache/docs/x.txt"), &[b'x'; 30]);
    set_mtime(&home.join("cache/catalog.json"), 1_700_001_000);
    set_mtime(&home.join("cache/images/a.png"), 1_700_001_003);
    set_mtime(&home.join("cache/images/b.png"), 1_700_001_002);
    set_mtime(&home.join("cache/docs/x.txt"), 1_700_001_001);
    write(&home.join("models_dev_cache.json"), &[b'm'; 20]);
    let mut activity = String::new();
    activity.push_str("{\"at\": 1, \"kind\": \"old\", \"text\": \"dropped\"}\n");
    activity.push_str("{\"at\": 2, \"kind\": \"old\", \"text\": \"dropped\"}\n");
    activity.push_str(
        "{\"at\": 1700000500, \"kind\": \"chat\", \"text\": \"hello   there\\n world\"}\n",
    );
    activity.push_str("{\"at\": 1700000600000, \"kind\": \"tool\", \"text\": \"ms stamp\"}\n");
    activity.push_str(&format!(
        "{{\"at\": \"1700000700.9\", \"kind\": null, \"text\": \"{}\"}}\n",
        "y".repeat(120)
    ));
    activity.push_str("{\"at\": null, \"kind\": \"cron\", \"text\": null}\n");
    activity.push_str("{\"kind\": \"note\", \"text\": \"Caf\\u00e9\"}\n");
    write(&home.join("activity.jsonl"), activity.as_bytes());
    write(
        &home.join("memories/MEMORY.md"),
        "Mate builds   Galion.\nSecond line § hidden part".as_bytes(),
    );
    write(&home.join("tasks/one-galion.md"), b"# Ship one\nbody\n");
    write(&home.join("tasks/one-galion.run"), b"running\n");

    let db = home.join("state.db");
    let _ = fs::remove_file(&db);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute_batch(DATA).unwrap();
    conn.close().unwrap();
}
