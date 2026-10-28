// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! SQLite helpers, moved verbatim in behaviour from ws/foundation e43d7ab
//! `crates/galion-backup/src/main.rs` (snapshot / verify / schema /
//! migrate-baseline), now callable in-process by `galion-backup run`.

use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

/// Online backup API copy (consistent with live WAL writers); the copy is a
/// self-contained single file (rollback journal, not WAL).
pub fn snapshot(src: &Path, dst: &Path) -> Result<(), String> {
    let src = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open src: {e}"))?;
    src.busy_timeout(Duration::from_secs(10)).ok();
    let mut dst = Connection::open(dst).map_err(|e| format!("open dst: {e}"))?;
    Backup::new(&src, &mut dst)
        .and_then(|b| b.run_to_completion(256, Duration::from_millis(20), None))
        .map_err(|e| format!("backup: {e}"))?;
    dst.execute_batch("PRAGMA journal_mode=DELETE;").ok();
    Ok(())
}

/// `PRAGMA integrity_check` result and table count (-1 if the count failed).
pub fn verify(db: &Path) -> Result<(String, i64), String> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open: {e}"))?;
    let r: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let t: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    Ok((r, t))
}

/// Lines printed by `schema <db>`.
pub fn schema(db: &Path) -> Result<Vec<String>, String> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open: {e}"))?;
    let v: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(-1);
    let mut out = vec![format!("user_version={v}")];
    match c.prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version") {
        Ok(mut st) => {
            let rows = st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            for r in rows.flatten() {
                out.push(format!("migration {} {} {}", r.0, r.1, r.2));
            }
        }
        Err(_) => out.push("schema_migrations: missing".into()),
    }
    Ok(out)
}

/// Create schema_migrations if missing and record a baseline; true if a new
/// row was recorded.
pub fn migrate_baseline(db: &Path, ver: i64, name: &str) -> Result<bool, String> {
    let c = Connection::open(db).map_err(|e| format!("open: {e}"))?;
    c.busy_timeout(Duration::from_secs(10)).ok();
    c.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')));")
        .map_err(|e| e.to_string())?;
    let n = c
        .execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name) VALUES (?1, ?2)",
            rusqlite::params![ver, name],
        )
        .map_err(|e| e.to_string())?;
    Ok(n == 1)
}

/// Tables that only describe a store, never hold its content (wave-4 slot
/// 427): a fresh openmind-memory DB may fill them on first open.
const META_TABLES: &[&str] = &["schema_migrations", "embedding_meta"];

/// Does the (snapshot) DB hold any content rows? Counts rows of every
/// ordinary table except `sqlite_*`, [`META_TABLES`] and the shadow tables of
/// virtual (FTS5) tables (`<vtab>_data`, `_config`, ...: FTS5 writes rows
/// there on CREATE). Read-only open, never creates the file. Errors are
/// returned so the caller can fail safe (keep the backup).
pub fn has_content(db: &Path) -> Result<bool, String> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open: {e}"))?;
    let mut st = c
        .prepare("SELECT name, type, coalesce(sql, '') FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let vtabs: Vec<String> = rows
        .iter()
        .filter(|(_, _, sql)| {
            sql.trim_start()
                .to_ascii_uppercase()
                .starts_with("CREATE VIRTUAL")
        })
        .map(|(n, _, _)| format!("{n}_"))
        .collect();
    for (name, _, sql) in &rows {
        let virt = sql
            .trim_start()
            .to_ascii_uppercase()
            .starts_with("CREATE VIRTUAL");
        if virt
            || name.starts_with("sqlite_")
            || META_TABLES.contains(&name.as_str())
            || vtabs.iter().any(|v| name.starts_with(v.as_str()))
        {
            continue;
        }
        let q = format!(
            "SELECT EXISTS (SELECT 1 FROM \"{}\")",
            name.replace('"', "\"\"")
        );
        let any: i64 = c
            .query_row(&q, [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if any != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}
