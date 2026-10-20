// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `make <dir>`: memory.db (openmind-memory schema, WAL, FTS5 rows),
//! hermes.db (hermes-rs schema, rollback journal) and legacy-state.db
//! (hermes schema plus an FTS5 index with sync triggers, like the Python
//! state.db), all written by real SQLite.
//! `verify <dir>`: reopen what the pure-Rust engine changed, with real
//! SQLite: integrity_check, and read back the rows it wrote.

#[allow(dead_code)]
#[path = "../../src/sql.rs"]
mod sql;

use rusqlite::{params, Connection};
use std::path::Path;

fn make(dir: &Path) -> rusqlite::Result<()> {
    std::fs::create_dir_all(dir).expect("dir");
    let m = Connection::open(dir.join("memory.db"))?;
    let mode: String = m.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    m.execute_batch(sql::MEMORY_SCHEMA)?;
    for (i, text) in [
        "zagreb harbour sailing plan for saturday",
        "the desk wakes the pod before the model call",
        "sailing lesson moved to sunday morning",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("l1-{i}");
        m.execute(
            "INSERT OR REPLACE INTO l1_records (record_id, content, type, priority, scene_name, session_key, session_id, timestamp_str, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![id, text, "fact", 50, "", "s1", "s1", "2026-10-20T21:00:00+02:00", "{}"],
        )?;
        m.execute(
            "INSERT INTO l1_fts (content, content_original, record_id, type, priority, scene_name, session_key, session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '', '', ?)",
            params![text, text, id, "fact", 50, "", "s1", "s1", "2026-10-20T21:00:00+02:00", "{}"],
        )?;
        let l0 = format!("l0-{i}");
        m.execute(
            "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![l0, "s1", "s1", "user", text, "2026-10-20T21:00:00+02:00", 1_760_986_800_000i64 + i as i64],
        )?;
        m.execute(
            "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, session_id, role, recorded_at, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![text, text, l0, "s1", "s1", "user", "2026-10-20T21:00:00+02:00", 1_760_986_800_000i64 + i as i64],
        )?;
    }
    let nofts: String = sql::MEMORY_SCHEMA
        .split(';')
        .filter(|s| !s.contains("VIRTUAL TABLE"))
        .collect::<Vec<_>>()
        .join(";");
    let n = Connection::open(dir.join("memory-nofts.db"))?;
    let _: String = n.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    n.execute_batch(&nofts)?;
    n.execute(
        "INSERT INTO l1_records (record_id, content, session_key, session_id) VALUES ('l1-0', 'made by sqlite', 's1', 's1')",
        [],
    )?;
    n.execute(
        "INSERT INTO l0_conversations (record_id, session_key, message_text) VALUES ('l0-0', 's1', 'made by sqlite')",
        [],
    )?;
    println!("memory-nofts.db: memory schema without the fts5 tables, 1 l1 + 1 l0 row");
    let ver: String = m.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    println!("memory.db: SQLite {ver}, journal_mode={mode}, 3 l1 + 3 l0 rows with FTS5");

    let h = Connection::open(dir.join("hermes.db"))?;
    h.pragma_update(None, "journal_mode", "delete")?;
    h.execute_batch(sql::HERMES_SCHEMA)?;
    h.execute(
        "INSERT INTO sessions (id, source, session_key, model, started_at, billing_provider) VALUES ('s-1', 'api_server', 'galion-one', 'm', 1760800000.5, 'openrouter')",
        [],
    )?;
    println!("hermes.db: journal_mode=delete, 1 session");

    let l = Connection::open(dir.join("legacy-state.db"))?;
    let lmode: String = l.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    l.execute_batch(sql::HERMES_SCHEMA)?;
    l.execute_batch(sql::LEGACY_FTS)?;
    l.execute_batch(
        "INSERT INTO sessions (id, source, session_key, model, started_at, billing_provider) VALUES
           ('s-py', 'api_server', 'galion-one', 'anthropic/claude-haiku-5.5', 1760800000.5, 'openrouter');
         INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'user', 'hi from python', 1, 1);
         INSERT INTO messages (session_id, role, content, tool_calls, timestamp, active) VALUES
           ('s-py', 'assistant', NULL, '[{\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"clock\",\"arguments\":\"{}\"}}]', 2, 1);
         INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'assistant', 'compacted away', 4, 0);",
    )?;
    let fts: i64 = l.query_row(
        "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'python'",
        [],
        |r| r.get(0),
    )?;
    println!("legacy-state.db: journal_mode={lmode}, FTS5 + 3 triggers, 3 messages, MATCH python -> {fts}");
    Ok(())
}

fn verify(dir: &Path) -> rusqlite::Result<()> {
    for name in [
        "memory.db",
        "memory-fresh.db",
        "memory-nofts.db",
        "hermes.db",
        "legacy-state.db",
    ] {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let c = Connection::open(&p)?;
        let ok: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        println!("verify {name}: integrity_check={ok}");
    }
    let m = Connection::open(dir.join("memory.db"))?;
    let n: i64 = m.query_row(
        "SELECT COUNT(*) FROM l0_conversations WHERE record_id LIKE 'turso-%'",
        [],
        |r| r.get(0),
    )?;
    let v: i64 = m.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let fts: Result<i64, _> = m.query_row(
        "SELECT COUNT(*) FROM l0_fts WHERE l0_fts MATCH 'turso'",
        [],
        |r| r.get(0),
    );
    println!("verify memory.db: rows written by turso={n}, user_version={v}, FTS5 MATCH 'turso' -> {fts:?}");
    let h = Connection::open(dir.join("hermes.db"))?;
    let (cnt, msgs): (i64, i64) = h.query_row(
        "SELECT (SELECT message_count FROM sessions WHERE id = 's-1'), (SELECT COUNT(*) FROM messages)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    println!("verify hermes.db: s-1 message_count={cnt}, messages={msgs}");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(args.get(2).map_or(".", String::as_str));
    let r = match args.get(1).map(String::as_str) {
        Some("make") => make(dir),
        Some("verify") => verify(dir),
        _ => {
            eprintln!("usage: sqlite-pure-spike-fixtures make|verify <dir>");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
