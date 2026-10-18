// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! SQLite session store, column-compatible with upstream hermes-agent
//! state.db (schema v30: `sessions`, `messages`). Read-only mode opens the
//! Python state.db with `mode=ro` for history; read-write mode owns a
//! separate DB file created with the same column layout.

use crate::session::{wire_message, SessionRow, SessionStore};
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Mutex;

/// Upstream DDL (hermes_state_common.py SCHEMA_SQL, v30) for the tables
/// hermes-rs writes. Same columns, so either side can read the other.
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS system_prompts (hash TEXT PRIMARY KEY, prompt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY, source TEXT NOT NULL, user_id TEXT, session_key TEXT, chat_id TEXT, chat_type TEXT,
    thread_id TEXT, display_name TEXT, origin_json TEXT, expiry_finalized INTEGER DEFAULT 0, model TEXT,
    model_config TEXT, system_prompt TEXT, system_prompt_hash TEXT, parent_session_id TEXT, started_at REAL NOT NULL,
    ended_at REAL, end_reason TEXT, message_count INTEGER DEFAULT 0, tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0, output_tokens INTEGER DEFAULT 0, cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0, cwd TEXT, git_branch TEXT,
    git_repo_root TEXT, git_metadata_generation INTEGER NOT NULL DEFAULT 0, billing_provider TEXT,
    billing_base_url TEXT, billing_mode TEXT, estimated_cost_usd REAL, actual_cost_usd REAL, cost_status TEXT,
    cost_source TEXT, pricing_version TEXT, title TEXT, title_source TEXT, last_activity_at REAL,
    last_activity_description TEXT, last_activity_provenance TEXT, api_call_count INTEGER DEFAULT 0,
    handoff_state TEXT, handoff_platform TEXT, handoff_error TEXT, compression_failure_cooldown_until REAL,
    compression_failure_error TEXT, compression_fallback_streak INTEGER NOT NULL DEFAULT 0,
    compression_ineffective_count INTEGER NOT NULL DEFAULT 0, compression_recovery_deadline REAL,
    profile_name TEXT, transport_profile TEXT, rewind_count INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0,
    last_read_at REAL, tool_names TEXT,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id),
    FOREIGN KEY (system_prompt_hash) REFERENCES system_prompts(hash)
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL REFERENCES sessions(id), role TEXT NOT NULL,
    content TEXT, tool_call_id TEXT, tool_calls TEXT, tool_name TEXT, effect_disposition TEXT,
    timestamp REAL NOT NULL, token_count INTEGER, finish_reason TEXT, reasoning TEXT, reasoning_content TEXT,
    reasoning_details TEXT, codex_reasoning_items TEXT, codex_message_items TEXT, platform_message_id TEXT,
    observed INTEGER DEFAULT 0, _compressed_summary INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1,
    compacted INTEGER NOT NULL DEFAULT 0, api_content TEXT, display_kind TEXT, display_metadata TEXT,
    display_identity BLOB, display_order INTEGER
);
CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id, id);
"#;

pub struct SqliteStore {
    conn: Mutex<Connection>,
    readonly: bool,
    has_active: bool,
}

/// REAL, INTEGER or TEXT ("2026-10-18 19:00:00", ISO) -> unix seconds.
fn as_f64(v: ValueRef) -> Option<f64> {
    match v {
        ValueRef::Real(f) => Some(f),
        ValueRef::Integer(i) => Some(i as f64),
        ValueRef::Text(t) => {
            let s = std::str::from_utf8(t).ok()?.trim().replace(' ', "T");
            s.parse::<f64>()
                .ok()
                .or_else(|| crate::time::parse_rfc3339(&s).map(|x| x as f64))
        }
        _ => None,
    }
}

fn as_str(v: ValueRef) -> Option<String> {
    match v {
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).to_string()),
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        _ => None,
    }
}

fn as_u64(v: ValueRef) -> u64 {
    as_f64(v).map(|f| f.max(0.0) as u64).unwrap_or(0)
}

const ROW_COLS: &str = "id, source, session_key, model, billing_provider, title, parent_session_id, started_at, ended_at, message_count, input_tokens, output_tokens, estimated_cost_usd";

fn row_of(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
    let g = |i: usize| r.get_ref(i);
    Ok(SessionRow {
        id: as_str(g(0)?).unwrap_or_default(),
        source: as_str(g(1)?).unwrap_or_default(),
        session_key: as_str(g(2)?),
        model: as_str(g(3)?),
        provider: as_str(g(4)?),
        title: as_str(g(5)?),
        parent_session_id: as_str(g(6)?),
        started_at: as_f64(g(7)?).unwrap_or(0.0),
        ended_at: as_f64(g(8)?),
        message_count: as_u64(g(9)?),
        input_tokens: as_u64(g(10)?),
        output_tokens: as_u64(g(11)?),
        cost_usd: as_f64(g(12)?).unwrap_or(0.0),
    })
}

impl SqliteStore {
    /// Own DB: create if missing, same column layout as upstream.
    pub fn open_rw(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        // DELETE journal like Galion's Hermes (SQLite WAL bug note in hermes.ts).
        let _ = conn.pragma_update(None, "journal_mode", "delete");
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(SqliteStore {
            conn: Mutex::new(conn),
            readonly: false,
            has_active: true,
        })
    }

    /// Legacy DB (the Python state.db): read-only, never written.
    pub fn open_ro(path: &Path) -> Result<Self, String> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let has_active = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'active'")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        Ok(SqliteStore {
            conn: Mutex::new(conn),
            readonly: true,
            has_active,
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Option<T> {
        let c = self.conn.lock().ok()?;
        f(&c).ok()
    }
}

impl SessionStore for SqliteStore {
    fn writable(&self) -> bool {
        !self.readonly
    }

    fn get(&self, id: &str) -> Option<SessionRow> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {ROW_COLS} FROM sessions WHERE id = ?1"),
                [id],
                row_of,
            )
            .optional()
        })
        .flatten()
    }

    fn find_by_key(&self, source: &str, key: &str) -> Option<SessionRow> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {ROW_COLS} FROM sessions WHERE source = ?1 AND session_key = ?2 AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1"),
                [source, key],
                row_of,
            )
            .optional()
        })
        .flatten()
    }

    fn upsert(&self, r: &SessionRow) -> Result<(), String> {
        if self.readonly {
            return Err("read-only session store".into());
        }
        let c = self.conn.lock().map_err(|_| "lock")?;
        c.execute(
            "INSERT INTO sessions (id, source, session_key, model, billing_provider, title, parent_session_id, started_at, ended_at, message_count, input_tokens, output_tokens, estimated_cost_usd, last_activity_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET session_key = excluded.session_key, model = excluded.model,
               billing_provider = excluded.billing_provider, title = excluded.title, ended_at = excluded.ended_at,
               input_tokens = excluded.input_tokens, output_tokens = excluded.output_tokens,
               estimated_cost_usd = excluded.estimated_cost_usd, last_activity_at = excluded.last_activity_at",
            params![r.id, r.source, r.session_key, r.model, r.provider, r.title, r.parent_session_id, r.started_at, r.ended_at,
                    r.message_count as i64, r.input_tokens as i64, r.output_tokens as i64, r.cost_usd, crate::time::now_f64()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn messages(&self, id: &str) -> Vec<Value> {
        let active = if self.has_active {
            " AND active = 1"
        } else {
            ""
        };
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT role, content, tool_calls, tool_call_id, tool_name FROM messages WHERE session_id = ?1{active} ORDER BY id"
            ))?;
            let rows = st.query_map([id], |r| {
                let role = as_str(r.get_ref(0)?).unwrap_or_default();
                let content = as_str(r.get_ref(1)?);
                let mut m = json!({"role": role, "content": content});
                if let Some(tc) = as_str(r.get_ref(2)?) {
                    if let Ok(v) = serde_json::from_str::<Value>(&tc) {
                        if !v.is_null() {
                            m["tool_calls"] = v;
                        }
                    }
                }
                if let Some(x) = as_str(r.get_ref(3)?) {
                    m["tool_call_id"] = json!(x);
                }
                if let Some(x) = as_str(r.get_ref(4)?) {
                    m["name"] = json!(x);
                }
                Ok(m)
            })?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default()
    }

    fn append(&self, id: &str, msgs: &[Value]) -> Result<(), String> {
        if self.readonly {
            return Err("read-only session store".into());
        }
        let mut c = self.conn.lock().map_err(|_| "lock")?;
        let tx = c.transaction().map_err(|e| e.to_string())?;
        let now = crate::time::now_f64();
        for m in msgs {
            let w = wire_message(m);
            let content = match &w["content"] {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            };
            let tool_calls = w.get("tool_calls").map(|v| v.to_string());
            tx.execute(
                "INSERT INTO messages (session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, w["role"].as_str().unwrap_or("user"), content, w["tool_call_id"].as_str(), tool_calls, w["name"].as_str(), now],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE sessions SET message_count = (SELECT COUNT(*) FROM messages WHERE session_id = ?1), last_activity_at = ?2 WHERE id = ?1",
            params![id, now],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    fn list(&self, limit: usize) -> Vec<SessionRow> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {ROW_COLS} FROM sessions ORDER BY started_at DESC LIMIT ?1"
            ))?;
            let rows = st.query_map([limit as i64], row_of)?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(crate::new_id(name))
    }

    #[test]
    fn reads_a_python_shaped_state_db_read_only() {
        let p = tmp("legacy-state-").with_extension("db");
        {
            // What Python Hermes (and the console's seedFlashPrompt quirk) writes.
            let c = Connection::open(&p).unwrap();
            c.execute_batch(SCHEMA).unwrap();
            c.execute_batch(
                "INSERT INTO sessions (id, source, session_key, model, started_at, billing_provider) VALUES
                   ('s-py', 'api_server', 'galion-one', 'anthropic/claude-haiku-5.5', 1760800000.5, 'openrouter'),
                   ('s-console', 'api_server', NULL, 'deepseek-ai/DeepSeek-V4.1-Flash', datetime('now'), 'river');
                 INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'user', 'hi', 1, 1);
                 INSERT INTO messages (session_id, role, content, tool_calls, timestamp, active) VALUES
                   ('s-py', 'assistant', NULL, '[{\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"clock\",\"arguments\":\"{}\"}}]', 2, 1);
                 INSERT INTO messages (session_id, role, content, tool_call_id, tool_name, timestamp, active) VALUES ('s-py', 'tool', 'now', 'c1', 'clock', 3, 1);
                 INSERT INTO messages (session_id, role, content, timestamp, active) VALUES ('s-py', 'assistant', 'compacted away', 4, 0);",
            )
            .unwrap();
        }
        let s = SqliteStore::open_ro(&p).unwrap();
        assert!(!s.writable());
        let r = s.get("s-py").unwrap();
        assert_eq!(r.model.as_deref(), Some("anthropic/claude-haiku-5.5"));
        assert_eq!(r.provider.as_deref(), Some("openrouter"));
        assert!((r.started_at - 1760800000.5).abs() < 1e-6);
        assert!(
            s.get("s-console").unwrap().started_at > 1.7e9,
            "TEXT datetime is coerced"
        );
        assert_eq!(
            s.find_by_key("api_server", "galion-one").unwrap().id,
            "s-py"
        );
        let m = s.messages("s-py");
        assert_eq!(m.len(), 3, "inactive rows are skipped");
        assert_eq!(m[1]["tool_calls"][0]["function"]["name"], "clock");
        assert_eq!(m[2]["tool_call_id"], "c1");
        assert_eq!(m[2]["name"], "clock");
        assert!(s.upsert(&SessionRow::new("x", "api_server", None)).is_err());
        assert!(s
            .append("s-py", &[json!({"role": "user", "content": "no"})])
            .is_err());
        assert_eq!(s.list(10).len(), 2);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn own_db_roundtrip_and_layered_fallback() {
        let own = tmp("own-state-").with_extension("db");
        let s = SqliteStore::open_rw(&own).unwrap();
        let mut r = SessionRow::new("s1", "api_server", Some("k"));
        r.model = Some("m".into());
        s.upsert(&r).unwrap();
        s.append(
            "s1",
            &[
                json!({"role": "user", "content": "a"}),
                json!({"role": "assistant", "content": "b"}),
            ],
        )
        .unwrap();
        assert_eq!(s.get("s1").unwrap().message_count, 2);
        assert_eq!(s.messages("s1")[1]["content"], "b");
        r.model = Some("m2".into());
        s.upsert(&r).unwrap();
        assert_eq!(s.get("s1").unwrap().model.as_deref(), Some("m2"));
        let _ = std::fs::remove_file(own);
    }
}
