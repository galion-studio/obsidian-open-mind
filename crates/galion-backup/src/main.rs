// SPDX-License-Identifier: MIT
//! galion-backup <cmd>:
//!   snapshot <src.db> <dst.db>   online backup API copy (consistent with live WAL writers)
//!   verify <db>                  PRAGMA integrity_check + table count; exit 1 if not "ok"
//!   schema <db>                  print user_version and schema_migrations rows (read-only)
//!   migrate-baseline <db> <ver> <name>   create schema_migrations if missing, record baseline (idempotent)
use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::time::Duration;

fn die(m: &str) -> ! { eprintln!("galion-backup: {m}"); std::process::exit(1) }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(|s| s.as_str()) {
        Some("snapshot") if a.len() == 4 => {
            let src = Connection::open_with_flags(&a[2], OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap_or_else(|e| die(&format!("open src: {e}")));
            src.busy_timeout(Duration::from_secs(10)).ok();
            let mut dst = Connection::open(&a[3]).unwrap_or_else(|e| die(&format!("open dst: {e}")));
            Backup::new(&src, &mut dst).and_then(|b| b.run_to_completion(256, Duration::from_millis(20), None))
                .unwrap_or_else(|e| die(&format!("backup: {e}")));
            // Self-contained single file: rollback journal, not WAL.
            dst.execute_batch("PRAGMA journal_mode=DELETE;").ok();
            println!("snapshot ok {} -> {}", a[2], a[3]);
        }
        Some("verify") if a.len() == 3 => {
            let c = Connection::open_with_flags(&a[2], OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap_or_else(|e| die(&format!("open: {e}")));
            let r: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_else(|e| die(&e.to_string()));
            let t: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table'", [], |r| r.get(0)).unwrap_or(-1);
            println!("verify {} integrity={} tables={}", a[2], r, t);
            if r != "ok" { std::process::exit(1) }
        }
        Some("schema") if a.len() == 3 => {
            let c = Connection::open_with_flags(&a[2], OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap_or_else(|e| die(&format!("open: {e}")));
            let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap_or(-1);
            println!("user_version={v}");
            let _ = match c.prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version") {
                Ok(mut st) => {
                    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).unwrap();
                    let v: Vec<_> = rows.flatten().collect(); for r in v { println!("migration {} {} {}", r.0, r.1, r.2); }
                }
                Err(_) => println!("schema_migrations: missing"),
            };
        }
        Some("migrate-baseline") if a.len() == 5 => {
            let ver: i64 = a[3].parse().unwrap_or_else(|_| die("bad version"));
            let c = Connection::open(&a[2]).unwrap_or_else(|e| die(&format!("open: {e}")));
            c.busy_timeout(Duration::from_secs(10)).ok();
            c.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')));")
                .unwrap_or_else(|e| die(&e.to_string()));
            let n = c.execute("INSERT OR IGNORE INTO schema_migrations(version, name) VALUES (?1, ?2)", rusqlite::params![ver, a[4]])
                .unwrap_or_else(|e| die(&e.to_string()));
            println!("schema_migrations baseline v{ver} {}", if n == 1 { "recorded" } else { "already present" });
        }
        _ => die("usage: snapshot <src> <dst> | verify <db> | schema <db> | migrate-baseline <db> <ver> <name>"),
    }
}
