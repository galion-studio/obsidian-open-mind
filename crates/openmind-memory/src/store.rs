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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};

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

/// Indexes added by openmind-memory (growth): keyset pagination per session
/// and newest-first listings. Plain b-tree indexes; the Node gateway ignores them.
pub const GROWTH_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_l0_session_ts ON l0_conversations(session_key, timestamp, record_id);
CREATE INDEX IF NOT EXISTS idx_l0_ts_id ON l0_conversations(timestamp, record_id);
CREATE INDEX IF NOT EXISTS idx_l1_updated_id ON l1_records(updated_time, record_id);
CREATE INDEX IF NOT EXISTS idx_l1_sk_updated_id ON l1_records(session_key, updated_time, record_id);
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
    /// The one connection that writes (all writes are serialised here).
    writer: Mutex<Connection>,
    /// Read-only connections (WAL lets them run beside the writer).
    readers: Mutex<Vec<Connection>>,
    readers_cv: Condvar,
    n_readers: usize,
    pub fts_available: bool,
    pub read_only: bool,
    /// Sticky "l0_fts has rows" (rows are never deleted by this server).
    l0_nonempty: AtomicBool,
    /// Bumped on every write this process commits.
    write_gen: AtomicU64,
}

/// A pooled read connection; goes back to the pool on drop.
pub struct ReadConn<'a> {
    store: &'a Store,
    conn: Option<Connection>,
    writer: Option<MutexGuard<'a, Connection>>,
}

impl std::ops::Deref for ReadConn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        match (&self.conn, &self.writer) {
            (Some(c), _) => c,
            (None, Some(w)) => w,
            (None, None) => unreachable!("ReadConn always holds a connection"),
        }
    }
}

impl Drop for ReadConn<'_> {
    fn drop(&mut self) {
        if let Some(c) = self.conn.take() {
            let mut pool = self.store.readers.lock().unwrap_or_else(|e| e.into_inner());
            pool.push(c);
            self.store.readers_cv.notify_one();
        }
    }
}

/// One page of a keyset-paginated listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Opaque cursor for the next (older) page; None at the end.
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct L0Row {
    pub record_id: String,
    pub session_key: String,
    pub session_id: String,
    pub role: String,
    pub message_text: String,
    pub recorded_at: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct L1Row {
    pub record_id: String,
    pub content: String,
    pub kind: String,
    pub priority: i64,
    pub scene_name: String,
    pub session_key: String,
    pub session_id: String,
    pub timestamp_str: String,
    pub updated_time: String,
}

/// Cursor = hex of `<sort key>\n<record_id>` (opaque to clients).
pub fn encode_cursor(key: &str, id: &str) -> String {
    format!("{key}\n{id}")
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn decode_cursor(c: &str) -> Option<(String, String)> {
    if c.len() & 1 == 1 || c.len() > 4096 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..c.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(c.get(i..i + 2)?, 16).ok())
        .collect();
    let s = String::from_utf8(bytes?).ok()?;
    let (k, id) = s.split_once('\n')?;
    Some((k.to_string(), id.to_string()))
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

/// Default number of pooled read connections.
pub const DEFAULT_READERS: usize = 3;

impl Store {
    pub fn open(path: &Path, read_only: bool) -> rusqlite::Result<Store> {
        Self::open_with(path, read_only, DEFAULT_READERS)
    }

    fn open_conn(path: &Path, read_only: bool) -> rusqlite::Result<Connection> {
        let conn = if read_only {
            Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        } else {
            Connection::open(path)?
        };
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        // Small page cache per connection: low RSS on cheap devices.
        conn.execute_batch("PRAGMA cache_size = -2048; PRAGMA temp_store = MEMORY;")?;
        Ok(conn)
    }

    /// `readers` read-only connections are pooled next to the single writer
    /// (0 = everything on the writer, the v0 behaviour).
    pub fn open_with(path: &Path, read_only: bool, readers: usize) -> rusqlite::Result<Store> {
        if !read_only {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
        }
        let conn = Self::open_conn(path, read_only)?;
        if !read_only {
            let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
            // WAL + NORMAL: still crash-consistent, no fsync per commit.
            conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
            conn.execute_batch(SCHEMA_SQL)?;
            // Pre-v3 DBs lack l0_conversations.timestamp (gateway runs the same ALTER).
            let _ = conn.execute_batch(
                "ALTER TABLE l0_conversations ADD COLUMN timestamp INTEGER DEFAULT 0",
            );
            if let Err(e) = conn.execute_batch(GROWTH_INDEX_SQL) {
                crate::log::warn(&format!("growth indexes not created (non-fatal): {e}"));
            }
            let _ = conn.execute_batch("PRAGMA optimize;");
        }
        let fts_available = Self::has_v2_fts(&conn);
        let mut pool = Vec::new();
        for _ in 0..readers {
            match Self::open_conn(path, true) {
                Ok(c) => {
                    let _ = c.execute_batch("PRAGMA query_only = 1;");
                    pool.push(c)
                }
                Err(e) => {
                    crate::log::warn(&format!("reader connection failed (non-fatal): {e}"));
                    break;
                }
            }
        }
        let n_readers = pool.len();
        let st = Store {
            writer: Mutex::new(conn),
            readers: Mutex::new(pool),
            readers_cv: Condvar::new(),
            n_readers,
            fts_available,
            read_only,
            l0_nonempty: AtomicBool::new(false),
            write_gen: AtomicU64::new(0),
        };
        st.l0_has_rows();
        Ok(st)
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

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A read connection from the pool (waits for one), or the writer when
    /// the pool is empty.
    pub fn reader(&self) -> ReadConn<'_> {
        if self.n_readers == 0 {
            return ReadConn {
                store: self,
                conn: None,
                writer: Some(self.lock()),
            };
        }
        let mut pool = self.readers.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(c) = pool.pop() {
                return ReadConn {
                    store: self,
                    conn: Some(c),
                    writer: None,
                };
            }
            pool = self
                .readers_cv
                .wait(pool)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn readers(&self) -> usize {
        self.n_readers
    }

    /// Changes whenever the database content may have changed: own writes
    /// bump `write_gen`; commits by other processes change `data_version`.
    /// Like [`Store::generation`] but never waits: None while a write holds
    /// the writer.
    pub fn try_generation(&self) -> Option<(u64, i64)> {
        let g = match self.writer.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        let dv = g
            .query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))
            .ok()?;
        Some((self.write_gen.load(Ordering::SeqCst), dv))
    }

    pub fn generation(&self) -> (u64, i64) {
        let dv = self
            .lock()
            .query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))
            .unwrap_or(-1);
        (self.write_gen.load(Ordering::SeqCst), dv)
    }

    pub fn count(&self, table: &str) -> i64 {
        if !matches!(
            table,
            "l1_records" | "l0_conversations" | "l0_fts" | "l1_fts"
        ) {
            return 0;
        }
        self.reader()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Whether l0_fts has any row: `EXISTS` (O(1)), sticky once true.
    /// (v0 ran `COUNT(*)` over the FTS table on every /recall.)
    pub fn l0_has_rows(&self) -> bool {
        if self.l0_nonempty.load(Ordering::Relaxed) {
            return true;
        }
        if !self.fts_available {
            return false;
        }
        let yes = self
            .reader()
            .query_row("SELECT EXISTS(SELECT 1 FROM l0_fts)", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|v| v != 0)
            .unwrap_or(false);
        if yes {
            self.l0_nonempty.store(true, Ordering::Relaxed);
        }
        yes
    }

    /// `searchL1Fts`: BM25 over l1_fts, best first. Empty on any error.
    pub fn search_l1_fts(&self, fts_query: &str, limit: usize) -> Vec<L1Hit> {
        if !self.fts_available {
            return Vec::new();
        }
        let conn = self.reader();
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
        let conn = self.reader();
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
        if self.fts_available && self.l0_has_rows() {
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
        let conn = self.reader();
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

    fn upsert_l0_tx(&self, tx: &rusqlite::Transaction, r: &L0Record) -> rusqlite::Result<()> {
        // l0_fts.record_id is UNINDEXED, so `DELETE FROM l0_fts WHERE record_id=?`
        // is a full FTS scan. Only run it when the row really existed (PK lookup);
        // fresh ids (the normal case) skip it. Same end state as v0.
        let existed: bool = tx
            .prepare_cached("SELECT 1 FROM l0_conversations WHERE record_id = ?")?
            .exists(params![r.id])?;
        tx.prepare_cached(
            "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(record_id) DO UPDATE SET \
             message_text=excluded.message_text, recorded_at=excluded.recorded_at, timestamp=excluded.timestamp",
        )?
        .execute(params![r.id, r.session_key, r.session_id, r.role, r.message_text, r.recorded_at, r.timestamp])?;
        if self.fts_available {
            if existed {
                tx.prepare_cached("DELETE FROM l0_fts WHERE record_id = ?")?
                    .execute(params![r.id])?;
            }
            tx.prepare_cached(
                "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, session_id, role, recorded_at, timestamp) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )?
            .execute(params![tokenize_for_fts(&r.message_text), r.message_text, r.id, r.session_key, r.session_id, r.role, r.recorded_at, r.timestamp])?;
        }
        Ok(())
    }

    /// `upsertL0` (metadata + FTS, no vectors), in one transaction.
    pub fn upsert_l0(&self, r: &L0Record) -> rusqlite::Result<()> {
        self.upsert_l0_batch(std::slice::from_ref(r)).map(|_| ())
    }

    /// Many L0 rows in ONE transaction (one WAL commit). All or nothing.
    pub fn upsert_l0_batch(&self, rows: &[L0Record]) -> rusqlite::Result<usize> {
        if self.read_only || rows.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for r in rows {
            self.upsert_l0_tx(&tx, r)?;
        }
        tx.commit()?;
        self.write_gen.fetch_add(1, Ordering::SeqCst);
        if self.fts_available {
            self.l0_nonempty.store(true, Ordering::Relaxed);
        }
        Ok(rows.len())
    }

    /// Newest-first page of L0 rows, optionally for one session.
    /// Keyset pagination on (timestamp, record_id): stable under inserts.
    pub fn list_l0(
        &self,
        session_key: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<L0Row>, String> {
        let limit = limit.clamp(1, 500);
        let after = match cursor {
            None => None,
            Some(c) => {
                let (k, id) = decode_cursor(c).ok_or("invalid cursor")?;
                Some((k.parse::<i64>().map_err(|_| "invalid cursor")?, id))
            }
        };
        let mut sql = String::from(
            "SELECT record_id, session_key, session_id, role, message_text, recorded_at, timestamp \
             FROM l0_conversations WHERE 1=1",
        );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(sk) = session_key {
            sql.push_str(" AND session_key = ?");
            args.push(sk.to_string().into());
        }
        if let Some((ts, id)) = &after {
            sql.push_str(" AND (timestamp < ? OR (timestamp = ? AND record_id < ?))");
            args.push((*ts).into());
            args.push((*ts).into());
            args.push(id.clone().into());
        }
        sql.push_str(" ORDER BY timestamp DESC, record_id DESC LIMIT ?");
        args.push(((limit + 1) as i64).into());
        let conn = self.reader();
        let mut rows: Vec<L0Row> = conn
            .prepare_cached(&sql)
            .and_then(|mut st| {
                let it = st.query_map(rusqlite::params_from_iter(args.iter()), |r| {
                    Ok(L0Row {
                        record_id: s(r, 0),
                        session_key: s(r, 1),
                        session_id: s(r, 2),
                        role: s(r, 3),
                        message_text: s(r, 4),
                        recorded_at: s(r, 5),
                        timestamp: n(r, 6),
                    })
                })?;
                Ok(it.filter_map(Result::ok).collect())
            })
            .map_err(|e| e.to_string())?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next = if more {
            rows.last()
                .map(|r| encode_cursor(&r.timestamp.to_string(), &r.record_id))
        } else {
            None
        };
        Ok(Page { items: rows, next })
    }

    /// Newest-first page of L1 records (by updated_time, record_id).
    pub fn list_l1(
        &self,
        session_key: Option<&str>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<L1Row>, String> {
        let limit = limit.clamp(1, 500);
        let after = match cursor {
            None => None,
            Some(c) => Some(decode_cursor(c).ok_or("invalid cursor")?),
        };
        let mut sql = String::from(
            "SELECT record_id, content, type, priority, scene_name, session_key, session_id, \
             timestamp_str, updated_time FROM l1_records WHERE 1=1",
        );
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(sk) = session_key {
            sql.push_str(" AND session_key = ?");
            args.push(sk.to_string().into());
        }
        if let Some((k, id)) = &after {
            sql.push_str(" AND (updated_time < ? OR (updated_time = ? AND record_id < ?))");
            args.push(k.clone().into());
            args.push(k.clone().into());
            args.push(id.clone().into());
        }
        sql.push_str(" ORDER BY updated_time DESC, record_id DESC LIMIT ?");
        args.push(((limit + 1) as i64).into());
        let conn = self.reader();
        let mut rows: Vec<L1Row> = conn
            .prepare_cached(&sql)
            .and_then(|mut st| {
                let it = st.query_map(rusqlite::params_from_iter(args.iter()), |r| {
                    Ok(L1Row {
                        record_id: s(r, 0),
                        content: s(r, 1),
                        kind: s(r, 2),
                        priority: n(r, 3),
                        scene_name: s(r, 4),
                        session_key: s(r, 5),
                        session_id: s(r, 6),
                        timestamp_str: s(r, 7),
                        updated_time: s(r, 8),
                    })
                })?;
                Ok(it.filter_map(Result::ok).collect())
            })
            .map_err(|e| e.to_string())?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next = if more {
            rows.last()
                .map(|r| encode_cursor(&r.updated_time, &r.record_id))
        } else {
            None
        };
        Ok(Page { items: rows, next })
    }

    /// Query plan of a statement (EXPLAIN QUERY PLAN), for tests/profiling.
    pub fn explain(&self, sql: &str) -> Vec<String> {
        let conn = self.reader();
        let res = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .and_then(|mut st| {
                let rows = st.query_map([], |r| r.get::<_, String>(3))?;
                Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
            });
        res.unwrap_or_default()
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
        self.write_gen.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
