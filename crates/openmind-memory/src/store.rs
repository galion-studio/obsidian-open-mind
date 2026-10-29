// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! SQLite store, same schema as the gateway's `core/store/sqlite.ts` (v2 FTS
//! schema). Only the tables used by keyword recall are touched; vec0 tables
//! (sqlite-vec) are never created or read: the live gateway runs with no
//! embedding provider (`/health` reports `embeddingService:false`).

use crate::fts::{bm25_rank_to_score, build_fts_query, tokenize_for_fts};
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, OpenFlags, Row, TransactionBehavior};
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

/// Index on the v3 column. Runs only after `migrate` has made sure the column
/// exists: putting it in SCHEMA_SQL made pre-v3 databases fail to open.
pub const SCHEMA_V3_SQL: &str =
    "CREATE INDEX IF NOT EXISTS idx_l0_timestamp ON l0_conversations(timestamp);";

/// v4 (openmind-memory only, additive): lookup index for the import-mode
/// dedupe key (session_key, timestamp, message_text). The gateway ignores
/// indexes it did not create.
pub const SCHEMA_V4_SQL: &str =
    "CREATE INDEX IF NOT EXISTS idx_l0_session_ts ON l0_conversations(session_key, timestamp);";

/// `PRAGMA user_version` this binary writes. 0 = never stamped (every
/// gateway-created DB, the live one included), 3 = gateway baseline
/// (`l0_conversations.timestamp` + v2 FTS), 4 = + `idx_l0_session_ts`.
pub const SCHEMA_VERSION: i64 = 4;

/// What one `Store::migrate` did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MigrateReport {
    pub from: i64,
    pub to: i64,
    /// `user_version` is newer than this binary: no DDL was run.
    pub newer: bool,
    pub fts_v1_dropped: bool,
    pub l1_fts_rebuilt: Option<usize>,
    pub l0_fts_rebuilt: Option<usize>,
}

/// Read-only schema facts for `openmind-memory schema <db>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaInfo {
    pub user_version: i64,
    pub binary_version: i64,
    pub l0_timestamp: bool,
    pub fts_v2: bool,
    pub tables: Vec<String>,
    /// Rows of a `schema_migrations` table if one exists (the VPS DB has a
    /// baseline row from galion-backup); informational only, never written.
    pub schema_migrations: Option<Vec<(i64, String)>>,
    pub l0_rows: i64,
    pub l0_fts_rows: i64,
    pub l1_rows: i64,
    pub l1_fts_rows: i64,
}

/// Outcome of one import batch.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportOutcome {
    /// Indexes (into the input) of the records actually inserted.
    pub inserted: Vec<usize>,
    pub duplicates: usize,
}

/// Deterministic L0 record id for import mode: SHA-256 over the dedupe key
/// (session_key, timestamp, message_text), NUL-separated.
pub fn import_record_id(session_key: &str, timestamp: i64, message_text: &str) -> String {
    let mut key = Vec::with_capacity(session_key.len() + message_text.len() + 24);
    key.extend_from_slice(session_key.as_bytes());
    key.push(0);
    key.extend_from_slice(timestamp.to_string().as_bytes());
    key.push(0);
    key.extend_from_slice(message_text.as_bytes());
    format!("l0_imp_{}", &crate::hash::sha256_hex(&key)[..32])
}

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
    /// `PRAGMA user_version` after open (and migration, when writable).
    pub schema_version: i64,
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
        let mut conn = if read_only {
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
            let rep = Self::migrate(&mut conn)?;
            if rep.newer {
                crate::log::warn(&format!(
                    "schema user_version {} is newer than this binary ({SCHEMA_VERSION}): no migration run",
                    rep.from
                ));
            } else if rep
                != (MigrateReport {
                    from: SCHEMA_VERSION,
                    to: SCHEMA_VERSION,
                    ..MigrateReport::default()
                })
            {
                crate::log::info(&format!("schema migrated: {rep:?}"));
            }
        }
        let fts_available = Self::has_v2_fts(&conn);
        let schema_version = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(Store {
            conn: Mutex::new(conn),
            fts_available,
            read_only,
            schema_version,
        })
    }

    /// Bring a database up to [`SCHEMA_VERSION`] in ONE immediate transaction
    /// (SQLite DDL and `PRAGMA user_version` are both transactional), so a
    /// kill -9 mid-migration leaves the old schema and old version intact and
    /// the next start simply retries.
    ///
    /// Version bookkeeping is `PRAGMA user_version` only (a 4-byte header
    /// field the Node gateway never reads or writes; see docs/memory-schema.md).
    /// The baseline steps are idempotent and run on EVERY writable open, even
    /// when `user_version` already says current: the gateway shares this file,
    /// knows nothing about the version and may have changed the schema since
    /// (for example dropped v1 FTS tables). Numbered steps above the baseline
    /// are additive only (new indexes), so the gateway keeps working on a
    /// database this binary migrated. A database whose `user_version` is
    /// newer than this binary gets no DDL at all.
    pub fn migrate(conn: &mut Connection) -> rusqlite::Result<MigrateReport> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let from: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if from > SCHEMA_VERSION {
            tx.commit()?;
            return Ok(MigrateReport {
                from,
                to: from,
                newer: true,
                ..MigrateReport::default()
            });
        }
        let exists = |t: &str| -> rusqlite::Result<bool> {
            Ok(tx.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
                [t],
                |r| r.get::<_, i64>(0),
            )? > 0)
        };
        let has_col = |t: &str, c: &str| -> rusqlite::Result<bool> {
            Ok(tx.query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{t}') WHERE name = ?"),
                [c],
                |r| r.get::<_, i64>(0),
            )? > 0)
        };
        let rows = |t: &str| -> rusqlite::Result<i64> {
            if !exists(t)? {
                return Ok(0);
            }
            tx.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
        };

        // Baseline v3 (gateway `initSchema`): v1 FTS -> drop both (FTS5 has
        // no ALTER ADD COLUMN), the tables, the pre-v3 timestamp column, then
        // repopulate any FTS table that was missing or dropped.
        let (l1_fts_had, l0_fts_had) = (exists("l1_fts")?, exists("l0_fts")?);
        let fts_v1 = (l1_fts_had && !has_col("l1_fts", "content_original")?)
            || (l0_fts_had && !has_col("l0_fts", "message_text_original")?);
        if fts_v1 {
            tx.execute_batch("DROP TABLE IF EXISTS l1_fts; DROP TABLE IF EXISTS l0_fts;")?;
        }
        tx.execute_batch(SCHEMA_SQL)?;
        if !has_col("l0_conversations", "timestamp")? {
            tx.execute_batch(
                "ALTER TABLE l0_conversations ADD COLUMN timestamp INTEGER DEFAULT 0",
            )?;
        }
        tx.execute_batch(SCHEMA_V3_SQL)?;
        let mut rep = MigrateReport {
            from,
            to: SCHEMA_VERSION,
            fts_v1_dropped: fts_v1,
            ..MigrateReport::default()
        };
        if (fts_v1 || !l1_fts_had) && rows("l1_records")? > 0 {
            rep.l1_fts_rebuilt = Some(Self::rebuild_l1_fts(&tx)?);
        }
        if (fts_v1 || !l0_fts_had) && rows("l0_conversations")? > 0 {
            rep.l0_fts_rebuilt = Some(Self::rebuild_l0_fts(&tx)?);
        }

        // v4: (session_key, timestamp) index for the import-mode dedupe lookup.
        if from < 4 {
            tx.execute_batch(SCHEMA_V4_SQL)?;
        }
        if from != SCHEMA_VERSION {
            tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        }
        tx.commit()?;
        Ok(rep)
    }

    /// Port of the L1 half of the gateway's `rebuildFtsIndex`. Returns rows indexed.
    fn rebuild_l1_fts(tx: &Connection) -> rusqlite::Result<usize> {
        tx.execute_batch("DELETE FROM l1_fts")?;
        let mut st = tx.prepare(
            "SELECT record_id, content, type, priority, scene_name, session_key, session_id, \
             timestamp_str, timestamp_start, timestamp_end, metadata_json FROM l1_records",
        )?;
        let mut ins = tx.prepare(
            "INSERT INTO l1_fts (content, content_original, record_id, type, priority, scene_name, \
             session_key, session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;
        let mut q = st.query([])?;
        let mut n_rows = 0;
        while let Some(r) = q.next()? {
            let content = s(r, 1);
            ins.execute(params![
                tokenize_for_fts(&content),
                content,
                s(r, 0),
                s(r, 2),
                n(r, 3),
                s(r, 4),
                s(r, 5),
                s(r, 6),
                s(r, 7),
                s(r, 8),
                s(r, 9),
                s(r, 10)
            ])?;
            n_rows += 1;
        }
        Ok(n_rows)
    }

    /// Port of the L0 half of the gateway's `rebuildFtsIndex`. Returns rows indexed.
    fn rebuild_l0_fts(tx: &Connection) -> rusqlite::Result<usize> {
        tx.execute_batch("DELETE FROM l0_fts")?;
        let mut st = tx.prepare(
            "SELECT record_id, message_text, session_key, session_id, role, recorded_at, timestamp \
             FROM l0_conversations",
        )?;
        let mut ins = tx.prepare(
            "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, \
             session_id, role, recorded_at, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )?;
        let mut q = st.query([])?;
        let mut n_rows = 0;
        while let Some(r) = q.next()? {
            let text = s(r, 1);
            ins.execute(params![
                tokenize_for_fts(&text),
                text,
                s(r, 0),
                s(r, 2),
                s(r, 3),
                s(r, 4),
                s(r, 5),
                n(r, 6)
            ])?;
            n_rows += 1;
        }
        Ok(n_rows)
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

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
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
        self.search_l1_fts_filtered(fts_query, limit, limit, &|_| true)
    }

    /// BM25 over l1_fts, keeping only hits that pass `keep`, best first.
    /// Filters run before the limit (ported from the VPS `vps-main-20261020`
    /// memory rewrite: a type/scene filter never starves the result list the
    /// way "take limit*3, then filter" can). At most `scan` ranked rows are
    /// read. (A record_id tie-break in SQL was tried and dropped: it
    /// doubled p50 search latency on 10k rows, see docs/real-tests/memory-real.md.)
    pub fn search_l1_fts_filtered(
        &self,
        fts_query: &str,
        limit: usize,
        scan: usize,
        keep: &dyn Fn(&L1Hit) -> bool,
    ) -> Vec<L1Hit> {
        if !self.fts_available || limit == 0 {
            return Vec::new();
        }
        let conn = self.lock();
        let sql = "SELECT record_id, content_original AS content, type, priority, scene_name, \
                   session_key, session_id, timestamp_str, timestamp_start, timestamp_end, metadata_json, \
                   bm25(l1_fts) AS rank FROM l1_fts WHERE l1_fts MATCH ? \
                   ORDER BY rank ASC LIMIT ?";
        let res = conn.prepare_cached(sql).and_then(|mut st| {
            let rows = st.query_map(params![fts_query, scan.max(limit) as i64], |r| {
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
            Ok(rows
                .filter_map(Result::ok)
                .filter(|h| keep(h))
                .take(limit)
                .collect::<Vec<_>>())
        });
        res.unwrap_or_else(|e| {
            crate::log::warn(&format!("[L1-fts-search] failed (non-fatal): {e}"));
            Vec::new()
        })
    }

    /// `searchL0Fts`: BM25 over l0_fts, best first. Empty on any error.
    pub fn search_l0_fts(&self, fts_query: &str, limit: usize) -> Vec<L0Hit> {
        self.search_l0_fts_session(fts_query, None, limit)
    }

    /// BM25 over l0_fts, optionally limited to one session_key in SQL, so the
    /// session filter runs before the limit (VPS rewrite semantics).
    pub fn search_l0_fts_session(
        &self,
        fts_query: &str,
        session_key: Option<&str>,
        limit: usize,
    ) -> Vec<L0Hit> {
        if !self.fts_available || limit == 0 {
            return Vec::new();
        }
        let conn = self.lock();
        let base = "SELECT record_id, message_text_original AS message_text, session_key, session_id, role, \
                    recorded_at, timestamp, bm25(l0_fts) AS rank FROM l0_fts WHERE l0_fts MATCH ?1";
        let sql = match session_key {
            Some(_) => {
                format!("{base} AND session_key = ?3 ORDER BY rank ASC LIMIT ?2")
            }
            None => format!("{base} ORDER BY rank ASC LIMIT ?2"),
        };
        let map = |r: &Row| {
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
        };
        let res = conn.prepare_cached(&sql).and_then(|mut st| {
            let rows: Vec<L0Hit> = match session_key {
                Some(sk) => st
                    .query_map(params![fts_query, limit as i64, sk], map)?
                    .filter_map(Result::ok)
                    .collect(),
                None => st
                    .query_map(params![fts_query, limit as i64], map)?
                    .filter_map(Result::ok)
                    .collect(),
            };
            Ok(rows)
        });
        res.unwrap_or_else(|e| {
            crate::log::warn(&format!("[L0-fts-search] failed (non-fatal): {e}"));
            Vec::new()
        })
    }

    /// `searchL0TextFallback`: l0_fts when it has rows, else a LIKE scan.
    pub fn l0_text_fallback(&self, raw: &str, limit: usize) -> Vec<String> {
        self.l0_text_fallback_scoped(raw, None, limit)
    }

    /// [`Self::l0_text_fallback`] limited to one `session_key` (slot 426,
    /// `/recall` `scope: "session"`). The filter runs in SQL before the
    /// limit, so other sessions can neither leak in nor starve the list.
    pub fn l0_text_fallback_scoped(
        &self,
        raw: &str,
        session_key: Option<&str>,
        limit: usize,
    ) -> Vec<String> {
        let q = raw.trim();
        if q.is_empty() {
            return Vec::new();
        }
        if self.fts_available && self.count("l0_fts") > 0 {
            let Some(fq) = build_fts_query(q) else {
                return Vec::new();
            };
            return self
                .search_l0_fts_session(&fq, session_key, limit)
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
        let sql = if session_key.is_some() {
            "SELECT message_text FROM l0_conversations WHERE message_text LIKE ?1 ESCAPE '\\' \
             AND session_key = ?3 ORDER BY timestamp DESC LIMIT ?2"
        } else {
            "SELECT message_text FROM l0_conversations WHERE message_text LIKE ?1 ESCAPE '\\' \
             ORDER BY timestamp DESC LIMIT ?2"
        };
        let pat = format!("%{escaped}%");
        let res = conn.prepare_cached(sql).and_then(|mut st| {
            let rows: Vec<String> = match session_key {
                Some(sk) => st
                    .query_map(params![pat, limit as i64, sk], |r| Ok(s(r, 0)))?
                    .filter_map(Result::ok)
                    .collect(),
                None => st
                    .query_map(params![pat, limit as i64], |r| Ok(s(r, 0)))?
                    .filter_map(Result::ok)
                    .collect(),
            };
            Ok(rows
                .into_iter()
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

    /// Import mode (opt-in, see `Engine::capture`): insert L0 records that are
    /// not already stored, in ONE transaction. A record is a duplicate when a
    /// row with the same (session_key, timestamp, message_text) exists, or its
    /// deterministic id does; so replaying the same batch any number of times,
    /// or replaying messages that live capture already stored, adds nothing.
    pub fn import_l0(&self, recs: &[L0Record]) -> rusqlite::Result<ImportOutcome> {
        let mut out = ImportOutcome::default();
        if self.read_only {
            return Ok(out);
        }
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut seen = tx.prepare_cached(
                "SELECT COUNT(*) FROM l0_conversations WHERE record_id = ?1 \
                 OR (session_key = ?2 AND timestamp = ?3 AND message_text = ?4)",
            )?;
            let mut ins = tx.prepare_cached(
                "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )?;
            let mut fts = tx.prepare_cached(
                "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, session_id, role, recorded_at, timestamp) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            for (i, r) in recs.iter().enumerate() {
                let hits: i64 = seen.query_row(
                    params![r.id, r.session_key, r.timestamp, r.message_text],
                    |row| row.get(0),
                )?;
                if hits > 0 {
                    out.duplicates += 1;
                    continue;
                }
                ins.execute(params![
                    r.id,
                    r.session_key,
                    r.session_id,
                    r.role,
                    r.message_text,
                    r.recorded_at,
                    r.timestamp
                ])?;
                if self.fts_available {
                    fts.execute(params![
                        tokenize_for_fts(&r.message_text),
                        r.message_text,
                        r.id,
                        r.session_key,
                        r.session_id,
                        r.role,
                        r.recorded_at,
                        r.timestamp
                    ])?;
                }
                out.inserted.push(i);
            }
        }
        tx.commit()?;
        Ok(out)
    }

    /// Read-only schema report for a database file (never migrates, never
    /// creates the file).
    pub fn schema_info(path: &Path) -> rusqlite::Result<SchemaInfo> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        let user_version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let mut st =
            conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
        let tables: Vec<String> = st
            .query_map([], |r| r.get(0))?
            .filter_map(Result::ok)
            .collect();
        drop(st);
        let has = |t: &str| tables.iter().any(|x| x == t);
        let col = |t: &str, c: &str| -> bool {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{t}') WHERE name = ?"),
                [c],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
                > 0
        };
        let rows = |t: &str| -> i64 {
            if !has(t) {
                return 0;
            }
            conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                .unwrap_or(-1)
        };
        let schema_migrations = if has("schema_migrations") {
            let mut st =
                conn.prepare("SELECT version, name FROM schema_migrations ORDER BY version")?;
            let v: Vec<(i64, String)> = st
                .query_map([], |r| Ok((n(r, 0), s(r, 1))))?
                .filter_map(Result::ok)
                .collect();
            Some(v)
        } else {
            None
        };
        Ok(SchemaInfo {
            user_version,
            binary_version: SCHEMA_VERSION,
            l0_timestamp: col("l0_conversations", "timestamp"),
            fts_v2: Self::has_v2_fts(&conn),
            l0_rows: rows("l0_conversations"),
            l0_fts_rows: rows("l0_fts"),
            l1_rows: rows("l1_records"),
            l1_fts_rows: rows("l1_fts"),
            schema_migrations,
            tables,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("om-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("vectors.db")
    }

    #[test]
    fn pre_v3_database_opens_and_gains_timestamp() {
        let p = tmp("prev3");
        Connection::open(&p)
            .unwrap()
            .execute_batch(
                "CREATE TABLE l0_conversations (record_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, \
                 session_id TEXT DEFAULT '', role TEXT NOT NULL DEFAULT '', message_text TEXT NOT NULL, \
                 recorded_at TEXT DEFAULT ''); INSERT INTO l0_conversations VALUES ('a','s','s','user','x','');",
            )
            .unwrap();
        let s = Store::open(&p, false).expect("pre-v3 db must open");
        assert!(s.fts_available);
        assert_eq!(s.count("l0_conversations"), 1);
        drop(s);
        // idempotent: a second open changes nothing and does not fail
        let s = Store::open(&p, false).unwrap();
        let ts: i64 = s
            .lock()
            .query_row("SELECT timestamp FROM l0_conversations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ts, 0);
    }

    #[test]
    fn filters_run_before_limit() {
        let s = Store::open(&tmp("filter"), false).unwrap();
        for i in 0..20 {
            s.insert_l1(
                &format!("e{i:02}"),
                "kiwi kiwi kiwi",
                "episodic",
                50,
                "",
                "k",
                "",
                "{}",
            )
            .unwrap();
        }
        s.insert_l1(
            "z",
            "kiwi and many other words here",
            "instruction",
            50,
            "",
            "k",
            "",
            "{}",
        )
        .unwrap();
        let q = crate::fts::build_fts_query("kiwi").unwrap();
        // The old path (take 3, then filter) found nothing here.
        let old: Vec<_> = s
            .search_l1_fts(&q, 3)
            .into_iter()
            .filter(|h| h.kind == "instruction")
            .collect();
        assert!(old.is_empty());
        let new = s.search_l1_fts_filtered(&q, 1, 5000, &|h| h.kind == "instruction");
        assert_eq!(new.len(), 1);
    }

    fn uv(p: &Path) -> i64 {
        Connection::open(p)
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    fn rec(id: &str, sk: &str, ts: i64, text: &str) -> L0Record {
        L0Record {
            id: id.into(),
            session_key: sk.into(),
            session_id: sk.into(),
            role: "user".into(),
            message_text: text.into(),
            recorded_at: "2026-10-20T18:00:00.000Z".into(),
            timestamp: ts,
        }
    }

    #[test]
    fn user_version_stamped_and_idempotent() {
        let p = tmp("uv");
        let s = Store::open(&p, false).unwrap();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        drop(s);
        assert_eq!(uv(&p), SCHEMA_VERSION);
        let mut c = Connection::open(&p).unwrap();
        let rep = Store::migrate(&mut c).unwrap();
        assert_eq!(
            rep,
            MigrateReport {
                from: SCHEMA_VERSION,
                to: SCHEMA_VERSION,
                ..MigrateReport::default()
            }
        );
        let idx: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='idx_l0_session_ts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn gateway_db_at_user_version_0_is_upgraded_and_fts_rebuilt() {
        // A gateway-created DB never stamps user_version (live: 0) and may
        // have rows but no FTS tables (pre-FTS) -> rebuild like the gateway.
        let p = tmp("gw0");
        Connection::open(&p)
            .unwrap()
            .execute_batch(
                "CREATE TABLE l0_conversations (record_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, \
                 session_id TEXT DEFAULT '', role TEXT NOT NULL DEFAULT '', message_text TEXT NOT NULL, \
                 recorded_at TEXT DEFAULT '');
                 INSERT INTO l0_conversations VALUES ('a','s','s','user','\u{8001}\u{670b}\u{53cb} hello 🦀','');
                 CREATE TABLE l1_records (record_id TEXT PRIMARY KEY, content TEXT NOT NULL, type TEXT DEFAULT '', \
                 priority INTEGER DEFAULT 50, scene_name TEXT DEFAULT '', session_key TEXT DEFAULT '', \
                 session_id TEXT DEFAULT '', timestamp_str TEXT DEFAULT '', timestamp_start TEXT DEFAULT '', \
                 timestamp_end TEXT DEFAULT '', created_time TEXT DEFAULT '', updated_time TEXT DEFAULT '', \
                 metadata_json TEXT DEFAULT '{}');
                 INSERT INTO l1_records (record_id, content) VALUES ('m1','\u{7528}\u{6237}\u{559c}\u{6b22}\u{559d}\u{5496}\u{5561} ☕');",
            )
            .unwrap();
        let mut c = Connection::open(&p).unwrap();
        let rep = Store::migrate(&mut c).unwrap();
        assert_eq!(
            (rep.from, rep.to, rep.l0_fts_rebuilt, rep.l1_fts_rebuilt),
            (0, SCHEMA_VERSION, Some(1), Some(1))
        );
        drop(c);
        let s = Store::open(&p, false).unwrap();
        let q = crate::fts::build_fts_query("\u{5496}\u{5561}").unwrap();
        let hits = s.search_l1_fts(&q, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].content,
            "\u{7528}\u{6237}\u{559c}\u{6b22}\u{559d}\u{5496}\u{5561} ☕"
        );
        let q = crate::fts::build_fts_query("\u{670b}\u{53cb}").unwrap();
        assert_eq!(
            s.search_l0_fts(&q, 5)[0].message_text,
            "\u{8001}\u{670b}\u{53cb} hello 🦀"
        );
    }

    #[test]
    fn v1_fts_is_dropped_and_rebuilt() {
        let p = tmp("v1");
        {
            let s = Store::open(&p, false).unwrap();
            s.upsert_l0(&rec("x", "s", 5, "rebuild zagreb")).unwrap();
        }
        Connection::open(&p)
            .unwrap()
            .execute_batch(
                "DROP TABLE l0_fts; DROP TABLE l1_fts; PRAGMA user_version = 0;
                 CREATE VIRTUAL TABLE l1_fts USING fts5(content, record_id UNINDEXED);
                 CREATE VIRTUAL TABLE l0_fts USING fts5(message_text, record_id UNINDEXED);",
            )
            .unwrap();
        let s = Store::open(&p, false).unwrap();
        assert!(s.fts_available);
        assert_eq!(s.count("l0_fts"), 1);
        assert_eq!(s.l0_text_fallback("zagreb", 5), vec!["rebuild zagreb"]);
    }

    #[test]
    fn newer_user_version_gets_no_ddl() {
        let p = tmp("newer");
        Connection::open(&p)
            .unwrap()
            .execute_batch("CREATE TABLE keep_me (x); PRAGMA user_version = 99;")
            .unwrap();
        let s = Store::open(&p, false).unwrap();
        assert_eq!(s.schema_version, 99);
        assert!(!s.fts_available);
        drop(s);
        let info = Store::schema_info(&p).unwrap();
        assert_eq!(info.tables, vec!["keep_me".to_string()]);
        assert_eq!(info.user_version, 99);
    }

    #[test]
    fn import_is_idempotent_and_dedupes_against_live_rows() {
        let s = Store::open(&tmp("import"), false).unwrap();
        // a row live capture stored earlier (random id), same key
        s.upsert_l0(&rec(
            "l0_live_1",
            "sess",
            1000,
            "\u{4e00}\u{6837}\u{7684}\u{6d88}\u{606f} 👋🏽",
        ))
        .unwrap();
        let batch: Vec<L0Record> = [
            (1000, "\u{4e00}\u{6837}\u{7684}\u{6d88}\u{606f} 👋🏽"), // dup of the live row
            (1001, "new one 👨‍👩‍👧"),
            (1001, "new one 👨‍👩‍👧"), // dup inside the batch
            (1002, "third"),
        ]
        .iter()
        .map(|(ts, t)| rec(&import_record_id("sess", *ts, t), "sess", *ts, t))
        .collect();
        let a = s.import_l0(&batch).unwrap();
        assert_eq!((a.inserted.clone(), a.duplicates), (vec![1, 3], 2));
        let b = s.import_l0(&batch).unwrap();
        assert_eq!((b.inserted.len(), b.duplicates), (0, 4));
        assert_eq!(s.count("l0_conversations"), 3);
        assert_eq!(s.count("l0_fts"), 3);
        // same text + ts in ANOTHER session is not a duplicate
        let other = rec(
            &import_record_id("other", 1002, "third"),
            "other",
            1002,
            "third",
        );
        assert_eq!(s.import_l0(&[other]).unwrap().inserted, vec![0]);
    }

    #[test]
    fn import_id_is_stable() {
        let a = import_record_id("s", 1, "x");
        assert_eq!(a, import_record_id("s", 1, "x"));
        assert_ne!(a, import_record_id("s", 2, "x"));
        assert_ne!(a, import_record_id("s2", 1, "x"));
        assert_eq!(a.len(), "l0_imp_".len() + 32);
    }

    /// The Node gateway's `initSchema` + statement preparation (tdai
    /// `src/core/store/sqlite.ts` 1.0.3, MIT; vec0 parts skipped because the
    /// live gateway runs without embeddings), replayed against a database this
    /// binary migrated to v4. Every statement must succeed, the blind ALTER
    /// must fail exactly the way the gateway expects (caught), and the
    /// gateway must leave `user_version` alone.
    #[test]
    fn gateway_init_sequence_tolerates_v4_database() {
        let p = tmp("gw-v4");
        {
            let s = Store::open(&p, false).unwrap();
            s.import_l0(&[rec(
                &import_record_id("s", 7, "\u{524d}"),
                "s",
                7,
                "\u{524d}",
            )])
            .unwrap();
        }
        let c = Connection::open(&p).unwrap();
        c.execute_batch("PRAGMA busy_timeout = 5000; PRAGMA journal_mode = WAL;")
            .unwrap();
        c.execute_batch(SCHEMA_SQL).unwrap(); // same CREATE ... IF NOT EXISTS set
                                              // gateway: try { ALTER ... ADD COLUMN timestamp } catch {}  -> must throw
        let alter =
            c.execute_batch("ALTER TABLE l0_conversations ADD COLUMN timestamp INTEGER DEFAULT 0");
        assert!(alter.unwrap_err().to_string().contains("duplicate column"));
        c.execute_batch(SCHEMA_V3_SQL).unwrap();
        // migrateFtsTablesIfNeeded: l1_fts exists and has content_original -> no drop
        let cols: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('l1_fts') WHERE name='content_original'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 1);
        for sql in [
            "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(record_id) DO UPDATE SET message_text=excluded.message_text, \
             recorded_at=excluded.recorded_at, timestamp=excluded.timestamp",
            "DELETE FROM l0_conversations WHERE record_id = ?",
            "SELECT session_key, session_id, role, message_text, recorded_at, timestamp FROM l0_conversations WHERE record_id = ?",
            "SELECT record_id, session_key, session_id, role, message_text, recorded_at, timestamp FROM l0_conversations \
             WHERE session_key = ? AND recorded_at > ? ORDER BY recorded_at ASC LIMIT ?",
            "SELECT record_id, session_key, session_id, role, message_text, recorded_at, timestamp FROM l0_conversations \
             WHERE record_id > ? ORDER BY record_id ASC LIMIT ?",
            "INSERT INTO l0_fts (message_text, message_text_original, record_id, session_key, session_id, role, recorded_at, timestamp) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            "SELECT record_id, message_text_original AS message_text, session_key, session_id, role, recorded_at, timestamp, \
             bm25(l0_fts) AS rank FROM l0_fts WHERE l0_fts MATCH ? ORDER BY rank ASC LIMIT ?",
            "SELECT key, value FROM embedding_meta",
        ] {
            c.prepare(sql).unwrap_or_else(|e| panic!("gateway statement failed: {e}\n{sql}"));
        }
        c.execute(
            "INSERT INTO l0_conversations (record_id, session_key, session_id, role, message_text, recorded_at, timestamp) \
             VALUES ('gw1', 's', 's', 'user', '\u{7f51}\u{5173}\u{5199}\u{5165}', '', 8) ON CONFLICT(record_id) DO UPDATE SET message_text=excluded.message_text",
            [],
        )
        .unwrap();
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            v, SCHEMA_VERSION,
            "gateway sequence must not touch user_version"
        );
        drop(c);
        // and this binary reopens it as current, with the gateway's row
        let s = Store::open(&p, false).unwrap();
        assert_eq!(
            (s.schema_version, s.count("l0_conversations")),
            (SCHEMA_VERSION, 2)
        );
    }

    /// The live VPS DB as of 2026-10-20: v3 schema written by openmind-memory
    /// 0.1.0 (user_version 0) plus galion-backup's `schema_migrations` table
    /// with a baseline row. Upgrade = one index + user_version 4; the
    /// `schema_migrations` table and row are left exactly as they were.
    #[test]
    fn live_like_db_with_schema_migrations_baseline() {
        let p = tmp("livelike");
        {
            let c = Connection::open(&p).unwrap();
            c.execute_batch(SCHEMA_SQL).unwrap();
            c.execute_batch(SCHEMA_V3_SQL).unwrap();
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, \
                 applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')));
                 INSERT INTO schema_migrations(version, name) VALUES (3, 'baseline');
                 INSERT INTO l0_conversations VALUES ('r','s','s','user','živjeli 🍻','',5);
                 INSERT INTO l0_fts VALUES ('živjeli 🍻','živjeli 🍻','r','s','s','user','',5);",
            )
            .unwrap();
        }
        let mut c = Connection::open(&p).unwrap();
        let rep = Store::migrate(&mut c).unwrap();
        assert_eq!(
            rep,
            MigrateReport {
                from: 0,
                to: SCHEMA_VERSION,
                ..MigrateReport::default()
            },
            "no FTS rebuild on a v2-FTS DB"
        );
        drop(c);
        let info = Store::schema_info(&p).unwrap();
        assert_eq!(info.user_version, SCHEMA_VERSION);
        assert_eq!(
            info.schema_migrations,
            Some(vec![(3, "baseline".to_string())])
        );
        assert_eq!((info.l0_rows, info.l0_fts_rows), (1, 1));
    }

    /// Slot 426: the scoped L0 fallback keeps other sessions out on both
    /// paths (FTS and LIKE), with the session filter before the limit.
    #[test]
    fn l0_text_fallback_scoped_keeps_other_sessions_out() {
        let s = Store::open(&tmp("scoped"), false).unwrap();
        // Ten newer rows of session b would fill a limit-1 global list.
        s.upsert_l0(&rec("a1", "sa", 1, "code word heliotrope one"))
            .unwrap();
        for i in 0..10 {
            s.upsert_l0(&rec(
                &format!("b{i}"),
                "sb",
                10 + i,
                "code word heliotrope two",
            ))
            .unwrap();
        }
        for pass in ["fts", "like"] {
            if pass == "like" {
                s.lock().execute("DELETE FROM l0_fts", []).unwrap();
                assert_eq!(s.count("l0_fts"), 0);
            }
            let all = s.l0_text_fallback("heliotrope", 20);
            assert_eq!(all.len(), 11, "{pass}: {all:?}");
            let a = s.l0_text_fallback_scoped("heliotrope", Some("sa"), 1);
            assert_eq!(a, vec!["code word heliotrope one".to_string()], "{pass}");
            let b = s.l0_text_fallback_scoped("heliotrope", Some("sb"), 20);
            assert_eq!(b.len(), 10, "{pass}");
            assert!(b.iter().all(|t| t.ends_with("two")), "{pass}: {b:?}");
            assert!(s
                .l0_text_fallback_scoped("heliotrope", Some("sc"), 20)
                .is_empty());
            // LIKE metacharacters stay literal with the filter on.
            assert!(s.l0_text_fallback_scoped("%", Some("sa"), 20).is_empty());
        }
    }
}
