// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Spike: run the SQL that openmind-memory and hermes-rs actually issue
//! against copies of real-SQLite fixture databases, through Turso
//! (`turso_core`, pure Rust, no C). Every check prints one line,
//! `PASS <name>: <detail>` or `FAIL <name>: <error>`. The exit status is
//! the number of failures (capped at 100). Usage:
//!
//! ```text
//! sqlite-pure-spike <dir with memory.db, hermes.db, legacy-state.db copies>
//! ```

#[allow(dead_code)] // LEGACY_FTS is only used by the fixture maker.
mod sql;

use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use turso_core::{
    Connection, Database, DatabaseOpts, Numeric, OpenFlags, OpenOptions, PlatformIO, SqliteDialect,
    Value, IO,
};

type Conn = Arc<Connection>;

fn err(e: impl std::fmt::Display) -> String {
    let s = e.to_string();
    s.chars().take(200).collect()
}

/// An open database and one connection.
struct Opened(#[allow(dead_code)] Arc<Database>, Conn);

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("opened")
    }
}

fn open(path: &Path, read_only: bool) -> Result<Opened, String> {
    let io: Arc<dyn IO> = Arc::new(PlatformIO::new().map_err(err)?);
    let db_opts = DatabaseOpts::new().with_index_method(cfg!(feature = "fts"));
    let mut opts = OpenOptions::new(Arc::new(SqliteDialect)).db_opts(db_opts);
    if read_only {
        opts = opts.flags(OpenFlags::ReadOnly);
    }
    let db = Database::open(io, &path.display().to_string(), opts).map_err(err)?;
    let conn = db.connect().map_err(err)?;
    Ok(Opened(db, conn))
}

fn query(c: &Conn, sql: &str, params: &[Value]) -> Result<Vec<Vec<String>>, String> {
    let mut st = c.prepare(sql).map_err(err)?;
    for (i, p) in params.iter().enumerate() {
        st.bind_at(NonZero::new(i + 1).unwrap(), p.clone())
            .map_err(err)?;
    }
    let rows = st.run_collect_rows().map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|r| r.iter().map(|v| v.to_string()).collect())
        .collect())
}

fn t(s: &str) -> Value {
    Value::from_text(s.to_string())
}

fn i(n: i64) -> Value {
    Value::Numeric(Numeric::Integer(n))
}

#[derive(Default)]
struct Report {
    fails: u32,
}

impl Report {
    fn check<T: std::fmt::Debug>(&mut self, name: &str, r: Result<T, String>) -> Option<T> {
        match r {
            Ok(v) => {
                let d = format!("{v:?}");
                println!("PASS {name}: {}", d.chars().take(160).collect::<String>());
                Some(v)
            }
            Err(e) => {
                self.fails += 1;
                println!("FAIL {name}: {e}");
                None
            }
        }
    }
}

fn memory(r: &mut Report, dir: &Path) {
    println!("== memory.db (openmind-memory store.rs + memory-schema 7952626)");
    let Some(Opened(_db, c)) = r.check(
        "memory open read-write",
        open(&dir.join("memory.db"), false),
    ) else {
        return;
    };
    r.check(
        "PRAGMA cache_size = -2048",
        c.execute("PRAGMA cache_size = -2048;").map_err(err),
    );
    r.check(
        "PRAGMA journal_mode = WAL",
        query(&c, "PRAGMA journal_mode = WAL", &[]),
    );
    r.check(
        "schema SQL (CREATE ... IF NOT EXISTS, incl. fts5)",
        c.execute(sql::MEMORY_SCHEMA).map_err(err),
    );
    r.check(
        "ALTER TABLE ADD COLUMN (duplicate, error expected and ignored by the code)",
        match c.execute("ALTER TABLE l0_conversations ADD COLUMN timestamp INTEGER DEFAULT 0") {
            Ok(()) => Err("no error: duplicate column was accepted".into()),
            Err(e) => Ok(err(e)),
        },
    );
    r.check("ALTER TABLE ADD COLUMN (new column)", c.execute("CREATE TABLE IF NOT EXISTS spike_alter (a TEXT); ALTER TABLE spike_alter ADD COLUMN b INTEGER DEFAULT 0").map_err(err));
    r.check(
        "pragma_table_info('l1_fts') (has_v2_fts)",
        query(&c, "SELECT name FROM pragma_table_info('l1_fts')", &[]),
    );
    for table in ["l1_records", "l0_conversations", "l1_fts", "l0_fts"] {
        r.check(
            &format!("SELECT COUNT(*) FROM {table}"),
            query(&c, &format!("SELECT COUNT(*) FROM {table}"), &[]),
        );
    }
    r.check(
        "search_l1_fts: bm25(l1_fts) ... MATCH",
        query(&c, "SELECT record_id, content_original AS content, type, priority, scene_name, \
                   session_key, session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json, \
                   bm25(l1_fts) AS rank FROM l1_fts WHERE l1_fts MATCH ? ORDER BY rank ASC LIMIT ?", &[t("sailing"), i(5)]),
    );
    r.check(
        "search_l0_fts: bm25(l0_fts) ... MATCH",
        query(&c, "SELECT record_id, message_text_original AS message_text, session_key, session_id, role, \
                   recorded_at, timestamp, bm25(l0_fts) AS rank FROM l0_fts WHERE l0_fts MATCH ? \
                   ORDER BY rank ASC LIMIT ?", &[t("sailing"), i(5)]),
    );
    r.check(
        "l0_text_fallback: LIKE ? ESCAPE '\\'",
        query(
            &c,
            "SELECT message_text FROM l0_conversations WHERE message_text LIKE ? ESCAPE '\\' \
                   ORDER BY timestamp DESC LIMIT ?",
            &[t("%sailing%"), i(5)],
        ),
    );
    r.check("upsert_l0 BEGIN", c.execute("BEGIN").map_err(err));
    r.check(
        "upsert_l0 INSERT ... ON CONFLICT(record_id) DO UPDATE",
        query(&c, "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(record_id) DO UPDATE SET \
             message_text=excluded.message_text, recorded_at=excluded.recorded_at, timestamp=excluded.timestamp",
            &[t("turso-1"), t("s1"), t("s1"), t("user"), t("written by turso"), t("2026-10-20T21:30:00+02:00"), i(1_760_988_600_000)]),
    );
    r.check(
        "upsert_l0 DELETE FROM l0_fts (fts5)",
        query(
            &c,
            "DELETE FROM l0_fts WHERE record_id = ?",
            &[t("turso-1")],
        ),
    );
    r.check(
        "upsert_l0 INSERT INTO l0_fts (fts5)",
        query(&c, "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, session_id, role, recorded_at, timestamp) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)", &[t("written by turso"), t("written by turso"), t("turso-1"), t("s1"), t("s1"), t("user"), t("2026-10-20T21:30:00+02:00"), i(1)]),
    );
    // The code commits whatever succeeded; a failed FTS insert would roll back.
    r.check("upsert_l0 COMMIT", c.execute("COMMIT").map_err(err));
    r.check(
        "insert_l1 INSERT OR REPLACE INTO l1_records",
        query(&c, "INSERT OR REPLACE INTO l1_records (record_id, content, type, priority, scene_name, session_key, session_id, timestamp_str, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)", &[t("turso-l1"), t("turso wrote this fact"), t("fact"), i(50), t(""), t("s1"), t("s1"), t(""), t("{}")]),
    );
    r.check(
        "PRAGMA user_version (read)",
        query(&c, "PRAGMA user_version", &[]),
    );
    r.check(
        "PRAGMA user_version = 3 (write)",
        c.execute("PRAGMA user_version = 3").map_err(err),
    );
    r.check(
        "sqlite_master lookup",
        query(
            &c,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            &[t("l1_fts")],
        ),
    );
    r.check(
        "memory-schema FTS rebuild: DELETE FROM l1_fts (fts5)",
        c.execute("DELETE FROM l1_fts").map_err(err),
    );
    r.check(
        "tools snapshot alternative: VACUUM INTO",
        c.execute(format!(
            "VACUUM INTO '{}'",
            dir.join("memory-snapshot.db").display()
        ))
        .map_err(err),
    );
    if cfg!(feature = "fts") {
        println!("== Turso native FTS (feature fts, tantivy) as an FTS5 replacement");
        r.check(
            "CREATE INDEX ... USING fts (content)",
            c.execute(
                "CREATE INDEX IF NOT EXISTS l1_records_fts ON l1_records USING fts (content)",
            )
            .map_err(err),
        );
        r.check(
            "fts_match / fts_score",
            query(&c, "SELECT record_id, fts_score(content, ?) AS score FROM l1_records WHERE fts_match(content, ?) ORDER BY score DESC LIMIT 5", &[t("sailing"), t("sailing")]),
        );
    }
    r.check("memory close", c.close().map_err(err));
}

fn fresh(r: &mut Report, dir: &Path, file: &str, what: &str) {
    println!("== {file} ({what})");
    let Some(Opened(_db, c)) = r.check("fresh open", open(&dir.join(file), false)) else {
        return;
    };
    let schema: String = sql::MEMORY_SCHEMA
        .split(";")
        .filter(|s| !s.contains("VIRTUAL TABLE"))
        .collect::<Vec<_>>()
        .join(";");
    r.check("fresh schema SQL", c.execute(&schema).map_err(err));
    r.check(
        "fresh schema SQL again (IF NOT EXISTS)",
        c.execute(&schema).map_err(err),
    );
    for (id, text) in [
        ("turso-a", "first"),
        ("turso-b", "second"),
        ("turso-a", "first, replaced"),
    ] {
        r.check(
            &format!("fresh INSERT OR REPLACE l1_records {id}"),
            query(&c, "INSERT OR REPLACE INTO l1_records (record_id, content, type, priority, scene_name, session_key, session_id, timestamp_str, metadata_json) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)", &[t(id), t(text), t("fact"), i(50), t(""), t("s1"), t("s1"), t(""), t("{}")]),
        );
    }
    for (id, text) in [
        ("turso-1", "written by turso"),
        ("turso-1", "updated by turso"),
    ] {
        r.check(
            &format!("fresh upsert_l0 ON CONFLICT {id}"),
            query(&c, "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
                 VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(record_id) DO UPDATE SET \
                 message_text=excluded.message_text, recorded_at=excluded.recorded_at, timestamp=excluded.timestamp",
                &[t(id), t("s1"), t("s1"), t("user"), t(text), t("2026-10-20T21:30:00+02:00"), i(1)]),
        );
    }
    if cfg!(feature = "fts") && file == "memory-fresh.db" {
        r.check(
            "fresh CREATE INDEX ... USING fts (content)",
            c.execute(
                "CREATE INDEX IF NOT EXISTS l1_records_fts ON l1_records USING fts (content)",
            )
            .map_err(err),
        );
        r.check(
            "fresh insert after fts index",
            query(
                &c,
                "INSERT INTO l1_records (record_id, content, session_key) VALUES (?, ?, ?)",
                &[
                    t("turso-c"),
                    t("sailing on saturday, sailing again"),
                    t("s1"),
                ],
            ),
        );
        r.check(
            "fresh fts_match / fts_score",
            query(&c, "SELECT record_id, fts_score(content, ?) AS score FROM l1_records WHERE fts_match(content, ?) ORDER BY score DESC LIMIT 5", &[t("sailing"), t("sailing")]),
        );
    }
    r.check(
        "fresh counts",
        query(
            &c,
            "SELECT (SELECT COUNT(*) FROM l1_records), (SELECT COUNT(*) FROM l0_conversations)",
            &[],
        ),
    );
    r.check("fresh close", c.close().map_err(err));
}

fn hermes(r: &mut Report, dir: &Path) {
    println!("== hermes.db (hermes-rs session_sqlite.rs, own DB)");
    if let Some(Opened(_db, c)) = r.check(
        "hermes open read-write",
        open(&dir.join("hermes.db"), false),
    ) {
        r.check(
            "PRAGMA journal_mode = delete (hermes default)",
            query(&c, "PRAGMA journal_mode = delete", &[]),
        );
        r.check(
            "schema SQL (AUTOINCREMENT, FOREIGN KEY)",
            c.execute(sql::HERMES_SCHEMA).map_err(err),
        );
        r.check(
            "upsert sessions ON CONFLICT(id) DO UPDATE ... excluded",
            query(&c, "INSERT INTO sessions (id, source, session_key, model, billing_provider, title, parent_session_id, started_at, ended_at, message_count, input_tokens, output_tokens, estimated_cost_usd, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET session_key = excluded.session_key, model = excluded.model,
               billing_provider = excluded.billing_provider, title = excluded.title, ended_at = excluded.ended_at,
               input_tokens = excluded.input_tokens, output_tokens = excluded.output_tokens,
               estimated_cost_usd = excluded.estimated_cost_usd, last_activity_at = excluded.last_activity_at",
                &[t("s-1"), t("api_server"), t("galion-one"), t("m2"), t("openrouter"), t("title"), Value::Null, Value::from_f64(1760800000.5), Value::Null, i(0), i(1), i(2), Value::from_f64(0.01), Value::from_f64(1760800001.0)]),
        );
        r.check("append BEGIN", c.execute("BEGIN").map_err(err));
        r.check(
            "append INSERT INTO messages (AUTOINCREMENT id)",
            query(&c, "INSERT INTO messages (session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                &[t("s-1"), t("user"), t("hello from turso"), Value::Null, Value::Null, Value::Null, Value::from_f64(1760800002.0)]),
        );
        r.check(
            "append UPDATE ... (SELECT COUNT(*) ...)",
            query(&c, "UPDATE sessions SET message_count = (SELECT COUNT(*) FROM messages WHERE session_id = ?1), last_activity_at = ?2 WHERE id = ?1", &[t("s-1"), Value::from_f64(1760800003.0)]),
        );
        r.check("append COMMIT", c.execute("COMMIT").map_err(err));
        r.check(
            "list ORDER BY started_at DESC LIMIT",
            query(
                &c,
                &format!(
                    "SELECT {} FROM sessions ORDER BY started_at DESC LIMIT ?1",
                    sql::ROW_COLS
                ),
                &[i(5)],
            ),
        );
        r.check("hermes close", c.close().map_err(err));
    }
    println!("== legacy-state.db (Python Hermes state.db: FTS5 + triggers), read-only");
    if let Some(Opened(_db, c)) = r.check(
        "legacy open read-only",
        open(&dir.join("legacy-state.db"), true),
    ) {
        r.check(
            "has_active: pragma_table_info('messages')",
            query(
                &c,
                "SELECT 1 FROM pragma_table_info('messages') WHERE name = 'active'",
                &[],
            ),
        );
        r.check(
            "get session",
            query(
                &c,
                &format!("SELECT {} FROM sessions WHERE id = ?1", sql::ROW_COLS),
                &[t("s-py")],
            ),
        );
        r.check(
            "messages ... AND active = 1 ORDER BY id",
            query(&c, "SELECT role, content, tool_calls, tool_call_id, tool_name FROM messages WHERE session_id = ?1 AND active = 1 ORDER BY id", &[t("s-py")]),
        );
        r.check(
            "read-only open refuses writes",
            match c.execute("INSERT INTO system_prompts (hash, prompt) VALUES ('x', 'y')") {
                Ok(()) => Err("write accepted on a read-only open".into()),
                Err(e) => Ok(err(e)),
            },
        );
    }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let dir = Path::new(&dir);
    let mut r = Report::default();
    println!("turso_core 0.8.3, features: fts={}", cfg!(feature = "fts"));
    memory(&mut r, dir);
    fresh(
        &mut r,
        dir,
        "memory-fresh.db",
        "created by Turso: the memory schema without the two fts5 tables",
    );
    fresh(
        &mut r,
        dir,
        "memory-nofts.db",
        "created by SQLite: the memory schema without the two fts5 tables",
    );
    hermes(&mut r, dir);
    println!("== {} check(s) failed", r.fails);
    std::process::exit(r.fails.min(100) as i32);
}
