// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Read-only port of `galion-light/one/defence.py`.
//!
//! Opens a SQLite file with a `file:...?mode=ro` URI and
//! `SQLITE_OPEN_READ_ONLY`, then returns the newest `health_checks` row per
//! service. The SQL is the defence.py statement. This crate never writes a
//! database. [`decide`] turns those rows into a [`DefenceDecision`] note. The
//! note does not apply a firewall change, spawn a firewall tool, or open a
//! socket.

mod decide;

pub use decide::{decide, DefenceDecision, ServiceNote};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::fmt::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Same statement as defence.py.
pub const QUERY: &str = concat!(
    "SELECT service, status, CAST(response_time_ms AS INT) ",
    "FROM health_checks ",
    "WHERE id IN (SELECT MAX(id) FROM health_checks GROUP BY service)"
);

/// One newest health row. `ms` is 0 when `response_time_ms` is NULL, matching
/// Python `int(ms or 0)` in defence.py. NULL service or status becomes an
/// empty string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthRow {
    pub service: String,
    pub status: String,
    pub ms: i64,
}

/// `file:<path>?mode=ro`, with the path percent-encoded. Absolute paths use
/// `file:///<abs>?mode=ro`.
fn sqlite_ro_uri(path: &Path) -> String {
    let bytes = path.as_os_str().as_bytes();
    let mut enc = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b'-' => {
                enc.push(b as char);
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                enc.push('%');
                enc.push(HEX[(b >> 4) as usize] as char);
                enc.push(HEX[(b & 0x0f) as usize] as char);
            }
        }
    }
    if bytes.first() == Some(&b'/') {
        format!("file://{enc}?mode=ro")
    } else {
        format!("file:{enc}?mode=ro")
    }
}

/// Open `path` read-only. The SQLite URI query `mode=ro` is mandatory; the
/// open flags repeat that so a non-URI fallback cannot create or write.
pub fn open_readonly(path: &Path) -> Result<Connection, rusqlite::Error> {
    let uri = sqlite_ro_uri(path);
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
}

/// Run [`QUERY`] on an already read-only connection.
pub fn latest_checks(conn: &Connection) -> Result<Vec<HealthRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(QUERY)?;
    let rows = stmt.query_map([], |row| {
        let service: Option<String> = row.get(0)?;
        let status: Option<String> = row.get(1)?;
        let ms: Option<i64> = row.get(2)?;
        Ok(HealthRow {
            service: service.unwrap_or_default(),
            status: status.unwrap_or_default(),
            // defence.py: int(ms or 0). Only NULL becomes 0; a real 0 stays 0.
            ms: ms.unwrap_or(0),
        })
    })?;
    rows.collect()
}

/// Open `path` read-only and return the newest row per service.
pub fn query_path(path: &Path) -> Result<Vec<HealthRow>, rusqlite::Error> {
    let conn = open_readonly(path)?;
    latest_checks(&conn)
}

/// JSON array of `{service, status, ms}` objects, compact, no trailing newline.
pub fn to_json(rows: &[HealthRow]) -> Result<String, serde_json::Error> {
    serde_json::to_string(rows)
}

/// Text form matching defence.py: one `service status ms` line per row.
pub fn to_text(rows: &[HealthRow]) -> String {
    let mut out = String::new();
    for row in rows {
        writeln!(out, "{} {} {}", row.service, row.status, row.ms).expect("write to String");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/health.db")
    }

    fn scratch() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("one-defence-lib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dst = dir.join(format!("health-{n}.db"));
        std::fs::copy(fixture(), &dst).unwrap();
        dst
    }

    #[test]
    fn uri_absolute_encodes_space_and_sets_mode_ro() {
        let uri = sqlite_ro_uri(Path::new("/tmp/a b.db"));
        assert_eq!(uri, "file:///tmp/a%20b.db?mode=ro");
    }

    #[test]
    fn uri_relative_has_no_extra_slash() {
        let uri = sqlite_ro_uri(Path::new("tests/fixtures/health.db"));
        assert_eq!(uri, "file:tests/fixtures/health.db?mode=ro");
    }

    #[test]
    fn fixture_keeps_latest_row_and_truncates_ms() {
        let rows = query_path(&fixture()).unwrap();
        assert_eq!(rows.len(), 4, "{rows:?}");
        let get = |name: &str| rows.iter().find(|r| r.service == name).unwrap().clone();
        // 45.7 -> 45, not the older critical/999 row.
        assert_eq!(
            get("console"),
            HealthRow {
                service: "console".into(),
                status: "healthy".into(),
                ms: 45,
            }
        );
        // NULL response_time_ms -> 0 (defence.py int(ms or 0)).
        assert_eq!(
            get("whisper"),
            HealthRow {
                service: "whisper".into(),
                status: "degraded".into(),
                ms: 0,
            }
        );
        // 12.9 -> 12, not the older 10.
        assert_eq!(get("hermes").ms, 12);
        assert_eq!(get("hermes").status, "healthy");
        // 0.4 -> 0, not the older 80.
        assert_eq!(get("vision").ms, 0);
        assert_eq!(get("vision").status, "healthy");
    }

    #[test]
    fn connection_is_read_only_and_insert_does_not_change_bytes() {
        let path = scratch();
        let before = std::fs::read(&path).unwrap();
        let conn = open_readonly(&path).unwrap();
        assert!(conn.is_readonly(rusqlite::DatabaseName::Main).unwrap());
        let err = conn.execute(
            "INSERT INTO health_checks (service, status, response_time_ms) VALUES ('nope', 'healthy', 1)",
            [],
        );
        assert!(err.is_err(), "insert must fail on mode=ro: {err:?}");
        drop(conn);
        let after = std::fs::read(&path).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn text_and_json_round_trip_shape() {
        let rows = query_path(&fixture()).unwrap();
        let text = to_text(&rows);
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), 4);
        for line in text.lines() {
            assert_eq!(line.split_whitespace().count(), 3, "{line}");
        }
        let json = to_json(&rows).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        for obj in arr {
            assert!(obj.get("service").unwrap().is_string());
            assert!(obj.get("status").unwrap().is_string());
            assert!(obj.get("ms").unwrap().is_i64());
        }
    }
}
