// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! SQLite store, same schema as the gateway's `core/store/sqlite.ts` (v2 FTS
//! schema). Only the tables used by keyword recall are touched; vec0 tables
//! (sqlite-vec) are never created or read: the live gateway runs with no
//! embedding provider (`/health` reports `embeddingService:false`).

use crate::fts::{bm25_rank_to_score, build_fts_query, tokenize_for_fts};
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OpenFlags, Row};
use std::path::Path;
use std::sync::Mutex;

pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS embedding_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS l1_records (
  record_id TEXT PRIMARY KEY, content TEXT NOT NULL, type TEXT DEFAULT '', priority INTEGER DEFAULT 50,
  scene_name TEXT DEFAULT '', session_key TEXT DEFAULT '', session_id TEXT DEFAULT '',
  timestamp_str TEXT DEFAULT '', timestamp_start TEXT DEFAULT '', timestamp_end TEXT DEFAULT '',
  created_time TEXT DEFAULT '', updated_time TEXT DEFAULT '', metadata_json TEXT DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_l1_type ON l1_records(type);
CREATE INDEX IF NOT EXISTS idx_l1_session_key ON l1_records(session_key);
CREATE INDEX IF NOT EXISTS idx_l1_session_id ON l1_records(session_id);
CREATE INDEX IF NOT EXISTS idx_l1_scene ON l1_records(scene_name);
CREATE INDEX IF NOT EXISTS idx_l1_ts_start ON l1_records(timestamp_start);
CREATE INDEX IF NOT EXISTS idx_l1_ts_end ON l1_records(timestamp_end);
CREATE INDEX IF NOT EXISTS idx_l1_session_updated ON l1_records(session_id, updated_time);
CREATE INDEX IF NOT EXISTS idx_l1_sessionkey_updated ON l1_records(session_key, updated_time);
CREATE TABLE IF NOT EXISTS l0_conversations (
  record_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, session_id TEXT DEFAULT '',
  role TEXT NOT NULL DEFAULT '', message_text TEXT NOT NULL, recorded_at TEXT DEFAULT '', timestamp INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_l0_session ON l0_conversations(session_key);
CREATE INDEX IF NOT EXISTS idx_l0_session_id ON l0_conversations(session_id);
CREATE INDEX IF NOT EXISTS idx_l0_recorded ON l0_conversations(recorded_at);
CREATE INDEX IF NOT EXISTS idx_l0_timestamp ON l0_conversations(timestamp);
CREATE VIRTUAL TABLE IF NOT EXISTS l1_fts USING fts5(
  content, content_original UNINDEXED, record_id UNINDEXED, type UNINDEXED, priority UNINDEXED,
  scene_name UNINDEXED, session_key UNINDEXED, session_id UNINDEXED, timestamp_str UNINDEXED,
  timestamp_start UNINDEXED, timestamp_end UNINDEXED, metadata_json UNINDEXED
);
CREATE VIRTUAL TABLE IF NOT EXISTS l0_fts USING fts5(
  message_text, message_text_original UNINDEXED, record_id UNINDEXED, session_key UNINDEXED,
  session_id UNINDEXED, role UNINDEXED, recorded_at UNINDEXED, timestamp UNINDEXED
);
"#;

#[derive(Debug, Clone, PartialEq)]
pub struct L1Hit {
    pub record_id: String,
    pub content: String,
    pub kind: String,
    pub priority: i64,
    pub scene_name: String,
    pub session_key: String,
    pub timestamp_str: String,
    pub timestamp_start: String,
    pub timestamp_end: String,
    pub metadata_json: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct L0Hit {
    pub record_id: String,
    pub session_key: String,
    pub session_id: String,
    pub role: String,
    pub message_text: String,
    pub recorded_at: String,
    pub timestamp: i64,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct L0Record {
    pub id: String,
    pub session_key: String,
    pub session_id: String,
    pub role: String,
    pub message_text: String,
    pub recorded_at: String,
    pub timestamp: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
    pub fts_available: bool,
    pub read_only: bool,
}

fn s(row: &Row, i: usize) -> String {
    match row.get_ref(i) {
        Ok(ValueRef::Text(t)) => String::from_utf8_lossy(t).into_owned(),
        Ok(ValueRef::Integer(n)) => n.to_string(),
        Ok(ValueRef::Real(f)) => f.to_string(),
        _ => String::new(),
    }
}

fn n(row: &Row, i: usize) -> i64 {
    match row.get_ref(i) {
        Ok(ValueRef::Integer(n)) => n,
        Ok(ValueRef::Real(f)) => f as i64,
        Ok(ValueRef::Text(t)) => String::from_utf8_lossy(t).trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn f(row: &Row, i: usize) -> f64 {
    match row.get_ref(i) {
        Ok(ValueRef::Real(f)) => f,
        Ok(ValueRef::Integer(n)) => n as f64,
        _ => f64::NAN,
    }
}

impl Store {
    pub fn open(path: &Path, read_only: bool) -> rusqlite::Result<Store> {
        let conn = if read_only {
            Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        } else {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            Connection::open(path)?
        };
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        // Small page cache: target is low RSS on cheap devices, not throughput.
        conn.execute_batch("PRAGMA cache_size = -2048;")?;
        if !read_only {
            let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
            conn.execute_batch(SCHEMA_SQL)?;
            // Pre-v3 DBs lack l0_conversations.timestamp (gateway runs the same ALTER).
            let _ = conn.execute_batch(
                "ALTER TABLE l0_conversations ADD COLUMN timestamp INTEGER DEFAULT 0",
            );
        }
        let fts_available = Self::has_v2_fts(&conn);
        Ok(Store {
            conn: Mutex::new(conn),
            fts_available,
            read_only,
        })
    }

    fn has_v2_fts(conn: &Connection) -> bool {
        let col = |t: &str, c: &str| -> bool {
            conn.prepare(&format!("SELECT name FROM pragma_table_info('{t}')"))
                .and_then(|mut st| {
                    let names: Vec<String> = st
                        .query_map([], |r| r.get(0))?
                        .filter_map(Result::ok)
                        .collect();
                    Ok(names.iter().any(|x| x == c))
                })
                .unwrap_or(false)
        };
        col("l1_fts", "content_original") && col("l0_fts", "message_text_original")
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn count(&self, table: &str) -> i64 {
        if !matches!(
            table,
            "l1_records" | "l0_conversations" | "l0_fts" | "l1_fts"
        ) {
            return 0;
        }
        self.lock()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// `searchL1Fts`: BM25 over l1_fts, best first. Empty on any error.
    pub fn search_l1_fts(&self, fts_query: &str, limit: usize) -> Vec<L1Hit> {
        if !self.fts_available {
            return Vec::new();
        }
        let conn = self.lock();
        let sql = "SELECT record_id, content_original AS content, type, priority, scene_name, \
                   session_key, session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json, \
                   bm25(l1_fts) AS rank FROM l1_fts WHERE l1_fts MATCH ? ORDER BY rank ASC LIMIT ?";
        let res = conn.prepare_cached(sql).and_then(|mut st| {
            let rows = st.query_map(params![fts_query, limit as i64], |r| {
                Ok(L1Hit {
                    record_id: s(r, 0),
                    content: s(r, 1),
                    kind: s(r, 2),
                    priority: n(r, 3),
                    scene_name: s(r, 4),
                    session_key: s(r, 5),
                    timestamp_str: s(r, 7),
                    timestamp_start: s(r, 8),
                    timestamp_end: s(r, 9),
                    metadata_json: s(r, 10),
                    score: bm25_rank_to_score(f(r, 11)),
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        });
        res.unwrap_or_else(|e| {
            crate::log::warn(&format!("[L1-fts-search] failed (non-fatal): {e}"));
            Vec::new()
        })
    }

    /// `searchL0Fts`: BM25 over l0_fts, best first. Empty on any error.
    pub fn search_l0_fts(&self, fts_query: &str, limit: usize) -> Vec<L0Hit> {
        if !self.fts_available {
            return Vec::new();
        }
        let conn = self.lock();
        let sql = "SELECT record_id, message_text_original AS message_text, session_key, session_id, role, \
                   recorded_at, timestamp, bm25(l0_fts) AS rank FROM l0_fts WHERE l0_fts MATCH ? \
                   ORDER BY rank ASC LIMIT ?";
        let res = conn.prepare_cached(sql).and_then(|mut st| {
            let rows = st.query_map(params![fts_query, limit as i64], |r| {
                Ok(L0Hit {
                    record_id: s(r, 0),
                    message_text: s(r, 1),
                    session_key: s(r, 2),
                    session_id: s(r, 3),
                    role: s(r, 4),
                    recorded_at: s(r, 5),
                    timestamp: n(r, 6),
                    score: bm25_rank_to_score(f(r, 7)),
                })
            })?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        });
        res.unwrap_or_else(|e| {
            crate::log::warn(&format!("[L0-fts-search] failed (non-fatal): {e}"));
            Vec::new()
        })
    }

    /// `searchL0TextFallback`: l0_fts when it has rows, else a LIKE scan.
    pub fn l0_text_fallback(&self, raw: &str, limit: usize) -> Vec<String> {
        let q = raw.trim();
        if q.is_empty() {
            return Vec::new();
        }
        if self.fts_available && self.count("l0_fts") > 0 {
            let Some(fq) = build_fts_query(q) else {
                return Vec::new();
            };
            return self
                .search_l0_fts(&fq, limit)
                .into_iter()
                .map(|h| h.message_text)
                .filter(|t| !t.is_empty())
                .collect();
        }
        let escaped: String = q
            .chars()
            .flat_map(|c| {
                if matches!(c, '\\' | '%' | '_') {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect();
        let conn = self.lock();
        let res = conn
            .prepare_cached(
                "SELECT message_text FROM l0_conversations WHERE message_text LIKE ? ESCAPE '\\' \
                 ORDER BY timestamp DESC LIMIT ?",
            )
            .and_then(|mut st| {
                let rows = st.query_map(params![format!("%{escaped}%"), limit as i64], |r| {
                    Ok(s(r, 0))
                })?;
                Ok(rows
                    .filter_map(Result::ok)
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>())
            });
        res.unwrap_or_default()
    }

    /// `upsertL0` (metadata + FTS, no vectors), in one transaction.
    pub fn upsert_l0(&self, r: &L0Record) -> rusqlite::Result<()> {
        if self.read_only {
            return Ok(());
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(record_id) DO UPDATE SET \
             message_text=excluded.message_text, recorded_at=excluded.recorded_at, timestamp=excluded.timestamp",
            params![r.id, r.session_key, r.session_id, r.role, r.message_text, r.recorded_at, r.timestamp],
        )?;
        if self.fts_available {
            tx.execute("DELETE FROM l0_fts WHERE record_id = ?", params![r.id])?;
            tx.execute(
                "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, session_id, role, recorded_at, timestamp) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![tokenize_for_fts(&r.message_text), r.message_text, r.id, r.session_key, r.session_id, r.role, r.recorded_at, r.timestamp],
            )?;
        }
        tx.commit()
    }

    /// Test/fixture helper: insert one L1 record into both tables.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_l1(
        &self,
        id: &str,
        content: &str,
        kind: &str,
        priority: i64,
        scene: &str,
        session_key: &str,
        timestamp_str: &str,
        metadata_json: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR REPLACE INTO l1_records (record_id, content, type, priority, scene_name, session_key, session_id, timestamp_str, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![id, content, kind, priority, scene, session_key, session_key, timestamp_str, metadata_json],
        )?;
        conn.execute(
            "INSERT INTO l1_fts (content, content_original, record_id, type, priority, scene_name, session_key, session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '', '', ?)",
            params![tokenize_for_fts(content), content, id, kind, priority, scene, session_key, session_key, timestamp_str, metadata_json],
        )?;
        Ok(())
    }
}
