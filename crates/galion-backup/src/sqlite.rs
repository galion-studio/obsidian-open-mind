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
