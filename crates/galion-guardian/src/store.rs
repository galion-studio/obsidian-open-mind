// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Health rows + incidents. Same semantics as guardian-tick.py: one row per
//! probe per beat; open an incident when a service is not healthy and has
//! none open (severity high if critical, else low); resolve with
//! `beat healthy` when it is healthy again; keep the newest N rows.
//!
//! Backends: [`JsonlStore`] (portable default) and, with the `sqlite`
//! feature, [`sqlite::SqliteStore`] writing guardian.db with the original
//! schema (Galion One reads its `health_checks`). Stamps are Zagreb +02:00.

use crate::probe::Row;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub service: String,
    pub opened: bool,
    pub severity: &'static str,
}

pub trait Store {
    fn record(&mut self, rows: &[Row], stamp: &str) -> Result<Vec<Event>, String>;
    fn open_incidents(&self) -> Result<Vec<String>, String>;
}

/// guardian-tick.py id: compact stamp + first 4 chars of the service.
pub fn incident_id(stamp: &str, service: &str) -> String {
    let compact: String = stamp
        .split('+')
        .next()
        .unwrap_or(stamp)
        .chars()
        .filter(|c| *c != '-' && *c != ':')
        .collect();
    let short: String = service.chars().take(4).collect();
    format!("{compact}{short}")
}

pub fn severity(status: &str) -> &'static str {
    if status == "critical" {
        "high"
    } else {
        "low"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Incident {
    pub id: String,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub service: String,
    pub description: Option<String>,
    pub detected_at: String,
    pub resolved_at: Option<String>,
    pub resolution: Option<String>,
}

pub struct JsonlStore {
    pub dir: PathBuf,
    pub keep_rows: usize,
    pub keep_resolved: usize,
}

impl JsonlStore {
    fn health(&self) -> PathBuf {
        self.dir.join("health.jsonl")
    }
    fn incidents_path(&self) -> PathBuf {
        self.dir.join("incidents.json")
    }
    pub fn incidents(&self) -> Vec<Incident> {
        std::fs::read_to_string(self.incidents_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    fn prune(&self) -> Result<(), String> {
        let p = self.health();
        let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let n = text.lines().count();
        if n <= self.keep_rows + self.keep_rows / 10 {
            return Ok(());
        }
        let keep: Vec<&str> = text.lines().skip(n - self.keep_rows).collect();
        crate::write_atomic(&p, &(keep.join("\n") + "\n"))
    }
}

impl Store for JsonlStore {
    fn record(&mut self, rows: &[Row], stamp: &str) -> Result<Vec<Event>, String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        for r in rows {
            let v = serde_json::json!({
                "service": r.name, "status": r.status, "response_time_ms": r.ms,
                "error": r.error, "checked_at": stamp,
            });
            buf.push_str(&v.to_string());
            buf.push('\n');
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.health())
            .and_then(|mut f| f.write_all(buf.as_bytes()))
            .map_err(|e| e.to_string())?;
        self.prune()?;
        let mut inc = self.incidents();
        let mut events = Vec::new();
        for r in rows {
            let open = inc
                .iter()
                .position(|i| i.service == r.name && i.status != "resolved");
            match (r.ok(), open) {
                (true, Some(ix)) => {
                    let i = &mut inc[ix];
                    i.status = "resolved".into();
                    i.resolved_at = Some(stamp.into());
                    i.resolution = Some("beat healthy".into());
                    events.push(Event {
                        service: r.name.clone(),
                        opened: false,
                        severity: severity(r.status),
                    });
                }
                (false, None) => {
                    inc.push(Incident {
                        id: incident_id(stamp, &r.name),
                        title: format!("{} {}", r.name, r.status),
                        severity: severity(r.status).into(),
                        status: "open".into(),
                        service: r.name.clone(),
                        description: r.error.clone(),
                        detected_at: stamp.into(),
                        resolved_at: None,
                        resolution: None,
                    });
                    events.push(Event {
                        service: r.name.clone(),
                        opened: true,
                        severity: severity(r.status),
                    });
                }
                _ => {}
            }
        }
        let resolved = inc.iter().filter(|i| i.status == "resolved").count();
        if resolved > self.keep_resolved {
            let mut drop = resolved - self.keep_resolved;
            inc.retain(|i| {
                if drop > 0 && i.status == "resolved" {
                    drop -= 1;
                    false
                } else {
                    true
                }
            });
        }
        let body = serde_json::to_string_pretty(&inc).map_err(|e| e.to_string())?;
        crate::write_atomic(&self.incidents_path(), &(body + "\n"))?;
        Ok(events)
    }

    fn open_incidents(&self) -> Result<Vec<String>, String> {
        Ok(self
            .incidents()
            .into_iter()
            .filter(|i| i.status != "resolved")
            .map(|i| i.service)
            .collect())
    }
}

#[cfg(feature = "sqlite")]
pub mod sqlite {
    //! guardian.db with the guardian-tick.py schema and SQL.
    use super::{incident_id, severity, Event, Store};
    use crate::probe::Row;
    use rusqlite::{params, Connection};

    pub struct SqliteStore {
        pub conn: Connection,
        pub keep_rows: usize,
    }

    impl SqliteStore {
        pub fn open(path: &std::path::Path, keep_rows: usize) -> Result<Self, String> {
            if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            let conn = Connection::open(path).map_err(|e| e.to_string())?;
            conn.execute_batch(
                "create table if not exists health_checks (
                    id integer primary key,
                    service text, status text, response_time_ms real, error text, checked_at text);
                 create table if not exists incidents (
                    id text primary key, title text, severity text, status text,
                    service text, description text, detected_at text, resolved_at text, resolution text);",
            )
            .map_err(|e| e.to_string())?;
            Ok(SqliteStore { conn, keep_rows })
        }
    }

    impl Store for SqliteStore {
        fn record(&mut self, rows: &[Row], stamp: &str) -> Result<Vec<Event>, String> {
            let tx = self.conn.transaction().map_err(|e| e.to_string())?;
            let mut events = Vec::new();
            for r in rows {
                tx.execute(
                    "insert into health_checks (service, status, response_time_ms, error, checked_at) values (?1, ?2, ?3, ?4, ?5)",
                    params![r.name, r.status, r.ms, r.error, stamp],
                )
                .map_err(|e| e.to_string())?;
                if r.ok() {
                    let n = tx
                        .execute(
                            "update incidents set status = 'resolved', resolved_at = ?1, resolution = 'beat healthy' where service = ?2 and status != 'resolved'",
                            params![stamp, r.name],
                        )
                        .map_err(|e| e.to_string())?;
                    if n > 0 {
                        events.push(Event {
                            service: r.name.clone(),
                            opened: false,
                            severity: severity(r.status),
                        });
                    }
                } else {
                    let open: Option<String> = tx
                        .query_row(
                            "select id from incidents where service = ?1 and status != 'resolved' limit 1",
                            params![r.name],
                            |row| row.get(0),
                        )
                        .ok();
                    if open.is_none() {
                        tx.execute(
                            "insert or ignore into incidents (id, title, severity, status, service, description, detected_at) values (?1, ?2, ?3, 'open', ?4, ?5, ?6)",
                            params![incident_id(stamp, &r.name), format!("{} {}", r.name, r.status), severity(r.status), r.name, r.error, stamp],
                        )
                        .map_err(|e| e.to_string())?;
                        events.push(Event {
                            service: r.name.clone(),
                            opened: true,
                            severity: severity(r.status),
                        });
                    }
                }
            }
            tx.execute(
                "delete from health_checks where id < (select max(id) - ?1 from health_checks)",
                params![self.keep_rows as i64],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(events)
        }

        fn open_incidents(&self) -> Result<Vec<String>, String> {
            let mut st = self
                .conn
                .prepare("select service from incidents where status != 'resolved'")
                .map_err(|e| e.to_string())?;
            let rows = st
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            Ok(rows.flatten().collect())
        }
    }

    /// guardian-tick.py `tool_warn()`: newest `[Tool loop warning:` row from
    /// Hermes state.db, opened read-only. Returns the one-line summary.
    pub fn tool_warn_line(state_db: &std::path::Path) -> Option<String> {
        let conn = Connection::open_with_flags(
            state_db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;
        let (mid, tool, ts, content): (i64, Option<String>, f64, String) = conn
            .query_row(
                "select id, tool_name, timestamp, content from messages where instr(content, ?1) > 0 order by id desc limit 1",
                ["[Tool loop warning:"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok()?;
        crate::tool_warn_format(mid, tool.as_deref(), ts, &content)
    }
}
