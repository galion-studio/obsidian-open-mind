// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! `openmind-memory freshness`: read-only "when was this memory store last
//! written?" report (wave-4 slot 388, memory-db-stale-diag).
//!
//! Why: on 2026-10-28 a probe flagged the live memory DB as "not written
//! since Oct 13". It had read the legacy root `vectors.db` (the `default`
//! instance path), while the live server writes
//! `<data_dir>/instances/<OPENMIND_MEMORY_INSTANCE_ID>/vectors.db`. This
//! report always names the DB it read, says which instance path that is,
//! and lists a stray root DB separately, so the two can't be mixed up again.
//!
//! Prints counts, file names, mtimes and Europe/Zagreb times only; never
//! memory content or session keys. Opens SQLite read-only. Exit code: 0
//! fresh, 3 stale, 1 error (main.rs).

use crate::config::Config;
use crate::util::now_ms;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// One finished report.
#[derive(Debug, Clone)]
pub struct Report {
    pub lines: Vec<String>,
    /// Newest write seen anywhere in the instance store (DB rows, DB/WAL
    /// mtime, conversations/records JSONL), unix ms; `None` = nothing found.
    pub newest_ms: Option<i64>,
    pub stale: bool,
}

fn mtime_ms(p: &Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64)
}

fn label_ms(ms: i64) -> String {
    zagreb_time::from_unix(ms.div_euclid(1000))
}

/// Newest `*.jsonl` (by name, which is the UTC date) in `dir`.
fn newest_jsonl(dir: &Path) -> Option<(String, i64)> {
    let mut best: Option<(String, i64)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let Some(ms) = mtime_ms(&e.path()) else {
            continue;
        };
        if best.as_ref().map(|(n, _)| name > *n).unwrap_or(true) {
            best = Some((name, ms));
        }
    }
    best
}

fn max_text(c: &Connection, sql: &str) -> Option<String> {
    c.query_row(sql, [], |r| r.get::<_, Option<String>>(0))
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
}

fn count(c: &Connection, table: &str) -> Option<i64> {
    c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .ok()
}

fn has_table(c: &Connection, table: &str) -> bool {
    c.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Parse an RFC 3339 stamp with `Z` or `+hh:mm` to unix ms (no chrono here;
/// zagreb-time owns the display side).
fn rfc3339_ms(ts: &str) -> Option<i64> {
    let t = ts.trim();
    let b = t.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |s: &str| s.parse::<i64>().ok();
    let (y, mo, d) = (num(&t[0..4])?, num(&t[5..7])?, num(&t[8..10])?);
    let (h, mi, s) = (num(&t[11..13])?, num(&t[14..16])?, num(&t[17..19])?);
    let mut rest = &t[19..];
    let mut frac_ms = 0i64;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(|c| c.is_ascii_digit()).collect();
        let mut ms3 = digits.clone();
        ms3.truncate(3);
        while ms3.len() < 3 {
            ms3.push('0');
        }
        frac_ms = num(&ms3)?;
        rest = &r[digits.len()..];
    }
    let off_min = match rest {
        "Z" | "z" => 0,
        o if o.len() == 6 && (o.starts_with('+') || o.starts_with('-')) => {
            let v = num(&o[1..3])? * 60 + num(&o[4..6])?;
            if o.starts_with('-') {
                -v
            } else {
                v
            }
        }
        _ => return None,
    };
    // days_from_civil (Howard Hinnant, public domain algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 + s - off_min * 60;
    Some(secs * 1000 + frac_ms)
}

fn db_lines(db: &Path, role: &str, lines: &mut Vec<String>, newest: &mut Vec<i64>) {
    lines.push(format!("db[{role}]={}", db.display()));
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    for (what, p) in [("db_mtime", db), ("wal_mtime", wal.as_path())] {
        if let Some(ms) = mtime_ms(p) {
            lines.push(format!("  {what}={}", label_ms(ms)));
            if role == "instance" {
                newest.push(ms);
            }
        }
    }
    let c = match Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            lines.push(format!("  open_error={e}"));
            return;
        }
    };
    if has_table(&c, "l0_conversations") {
        let n = count(&c, "l0_conversations").unwrap_or(-1);
        let last = max_text(&c, "SELECT MAX(recorded_at) FROM l0_conversations");
        let last_ms = last.as_deref().and_then(rfc3339_ms);
        lines.push(format!(
            "  l0_rows={n} l0_last={}",
            last_ms.map(label_ms).unwrap_or_else(|| "none".into())
        ));
        if let (Some(ms), "instance") = (last_ms, role) {
            newest.push(ms);
        }
    } else {
        lines.push("  l0_conversations=absent".into());
    }
    if has_table(&c, "l1_records") {
        let n = count(&c, "l1_records").unwrap_or(-1);
        let last = max_text(
            &c,
            "SELECT MAX(x) FROM (SELECT MAX(updated_time) AS x FROM l1_records \
             UNION ALL SELECT MAX(created_time) FROM l1_records)",
        );
        let last_ms = last.as_deref().and_then(rfc3339_ms);
        lines.push(format!(
            "  l1_rows={n} l1_last={}",
            last_ms.map(label_ms).unwrap_or_else(|| "none".into())
        ));
        if let (Some(ms), "instance") = (last_ms, role) {
            newest.push(ms);
        }
    } else {
        lines.push("  l1_records=absent".into());
    }
}

/// Build the report for `data_dir` + `instance_id` (same path rule as the
/// server, `Config::db_path`). `now` is unix ms (tests pin it).
pub fn report(data_dir: &Path, instance_id: &str, max_age_h: u64, now: i64) -> Report {
    let cfg = Config::from_env(&|k| match k {
        "OPENMIND_MEMORY_DATA_DIR" => Some(data_dir.display().to_string()),
        "OPENMIND_MEMORY_INSTANCE_ID" => Some(instance_id.to_string()),
        _ => None,
    });
    let db = cfg.db_path();
    let mut lines = vec![
        format!("data_dir={}", data_dir.display()),
        format!(
            "instance_id={}",
            if instance_id.is_empty() {
                "default"
            } else {
                instance_id
            }
        ),
    ];
    let mut newest = Vec::new();
    if db.is_file() {
        db_lines(&db, "instance", &mut lines, &mut newest);
    } else {
        lines.push(format!("db[instance]={} missing", db.display()));
    }
    for sub in ["conversations", "records"] {
        match newest_jsonl(&data_dir.join(sub)) {
            Some((name, ms)) => {
                lines.push(format!("{sub}_last_file={name} mtime={}", label_ms(ms)));
                newest.push(ms);
            }
            None => lines.push(format!("{sub}_last_file=none")),
        }
    }
    let root = data_dir.join("vectors.db");
    if root != db && root.is_file() {
        // Legacy default-instance DB next to an instance store: report it,
        // but it never counts toward freshness.
        lines.push("stray_root_db=yes (not the live instance; ignored for freshness)".into());
        db_lines(&root, "legacy-root", &mut lines, &mut Vec::new());
    }
    let newest_ms = newest.into_iter().max();
    let limit = (max_age_h as i64) * 3_600_000;
    let stale = match newest_ms {
        Some(ms) => now - ms > limit,
        None => true,
    };
    match newest_ms {
        Some(ms) => lines.push(format!(
            "newest_write={} age_h={:.1}",
            label_ms(ms),
            (now - ms) as f64 / 3_600_000.0
        )),
        None => lines.push("newest_write=none".into()),
    }
    lines.push(format!(
        "state={} (max_age_h={max_age_h})",
        if stale { "stale" } else { "fresh" }
    ));
    Report {
        lines,
        newest_ms,
        stale,
    }
}

/// CLI entry: `freshness <data_dir> [instance_id] [max_age_hours]`.
pub fn run(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        return 64;
    };
    let instance = args
        .get(1)
        .cloned()
        .or_else(|| std::env::var("OPENMIND_MEMORY_INSTANCE_ID").ok())
        .unwrap_or_else(|| "default".into());
    let max_age_h = match args.get(2).map(|s| s.parse::<u64>()) {
        None => 24,
        Some(Ok(v)) => v,
        Some(Err(_)) => return 64,
    };
    if !Path::new(dir).is_dir() {
        eprintln!("freshness: {dir} is not a directory");
        return 1;
    }
    let r = report(Path::new(dir), &instance, max_age_h, now_ms());
    for l in &r.lines {
        println!("{l}");
    }
    if r.stale {
        3
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{L0Record, Store};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "om-fresh-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rfc3339_parse_matches_known_instants() {
        // 2026-10-13T09:04:38.716Z (live VPS stamp, slot 367) = 1791882278716.
        assert_eq!(rfc3339_ms("2026-10-13T09:04:38.716Z"), Some(1_791_882_278_716));
        assert_eq!(
            rfc3339_ms("2026-10-13T11:04:38+02:00"),
            Some(1_791_882_278_000)
        );
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("2026-10-25T02:30:00+01:00"), rfc3339_ms("2026-10-25T01:30:00Z"));
        assert_eq!(rfc3339_ms("garbage"), None);
        assert_eq!(rfc3339_ms("2026-10-13 09:04:38Z"), None);
    }

    #[test]
    fn empty_dir_is_stale_and_says_missing() {
        let d = tmp("empty");
        let r = report(&d, "galion", 24, now_ms());
        assert!(r.stale);
        assert!(r.newest_ms.is_none());
        assert!(r.lines.iter().any(|l| l.contains("db[instance]=") && l.ends_with("missing")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn instance_db_counts_and_root_db_is_ignored() {
        let d = tmp("inst");
        // Legacy root DB with a "newer" row must NOT make the store fresh.
        let root = Store::open(&d.join("vectors.db"), false).unwrap();
        root.upsert_l0(&L0Record {
            id: "r".into(),
            session_key: "s".into(),
            session_id: "s".into(),
            role: "user".into(),
            message_text: "legacy".into(),
            recorded_at: "2026-10-28T12:00:00.000Z".into(),
            timestamp: 0,
        })
        .unwrap();
        drop(root);
        let idir = d.join("instances").join("galion");
        std::fs::create_dir_all(&idir).unwrap();
        let s = Store::open(&idir.join("vectors.db"), false).unwrap();
        s.upsert_l0(&L0Record {
            id: "a".into(),
            session_key: "s".into(),
            session_id: "s".into(),
            role: "user".into(),
            message_text: "hello".into(),
            recorded_at: "2026-10-19T12:57:55.447Z".into(),
            timestamp: 0,
        })
        .unwrap();
        drop(s);
        // Pin file mtimes out of the way by judging against the row only:
        // now = row + 1 h, mtimes are "now" on the box so newest is the file.
        let row = rfc3339_ms("2026-10-19T12:57:55.447Z").unwrap();
        let r = report(&d, "galion", 24, row + 3_600_000);
        assert!(r.lines.iter().any(|l| l == "  l0_rows=1 l0_last=2026-10-19T14:57:55+02:00"));
        assert!(r.lines.iter().any(|l| l.starts_with("stray_root_db=yes")));
        assert!(r.lines.iter().any(|l| l.starts_with("db[legacy-root]=")));
        // Content and session keys never appear.
        assert!(!r.lines.iter().any(|l| l.contains("hello") || l.contains("legacy\"")));
        // Default instance reads the root DB instead and reports no stray.
        let r2 = report(&d, "default", 24, now_ms());
        assert!(r2.lines.iter().any(|l| l == "  l0_rows=1 l0_last=2026-10-28T13:00:00+01:00"));
        assert!(!r2.lines.iter().any(|l| l.starts_with("stray_root_db")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn max_age_decides_state() {
        let d = tmp("age");
        std::fs::create_dir_all(d.join("conversations")).unwrap();
        std::fs::write(d.join("conversations").join("2026-10-19.jsonl"), "{}\n").unwrap();
        let ms = mtime_ms(&d.join("conversations").join("2026-10-19.jsonl")).unwrap();
        assert!(!report(&d, "default", 1, ms + 1_000).stale);
        assert!(report(&d, "default", 1, ms + 3_600_001).stale);
        assert!(!report(&d, "default", 0, ms).stale);
        let _ = std::fs::remove_dir_all(&d);
    }
}
