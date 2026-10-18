// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! galion-guardian: Rust port of galion-light `scripts/guardian-tick.py`
//! (beat, store, incidents, tool-loop warning), the alerting half of
//! `scripts/health-tick.py` (activity rows) and keep.sh's daily digest row.
//! It records and suggests. It never kills, restarts, seals or reads secrets.

pub mod config;
pub mod probe;
pub mod store;

use config::Settings;
use openmind_survival::decide::{decide, Observation, Policy, Suggestion};
use openmind_survival::{nerve::nerve_line, ProbeResult};
use probe::{Env, Row};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use store::{Event, Store};

/// Write via a temp file + rename so readers never see half a file.
pub fn write_atomic(path: &Path, body: &str) -> Result<(), String> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn append_line(path: &Path, line: &str) -> Result<(), String> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// guardian-tick.py tool-warn line format (when-stamp in Zagreb time).
pub fn tool_warn_format(mid: i64, tool: Option<&str>, ts: f64, content: &str) -> Option<String> {
    let mark = "[Tool loop warning:";
    let start = content.find(mark)?;
    let end = start + content[start..].find(']')?;
    let body = content[start + mark.len()..end].trim();
    let parts: Vec<&str> = body.split(';').map(str::trim).collect();
    let code = parts
        .first()
        .copied()
        .filter(|s| !s.is_empty())
        .unwrap_or("warn");
    let count = parts
        .iter()
        .find_map(|p| p.strip_prefix("count="))
        .unwrap_or("");
    let tool = tool.filter(|t| !t.is_empty()).unwrap_or("-");
    let when = zagreb_time::from_unix(ts as i64);
    let line = format!("WARN id={mid} code={code} count={count} tool={tool} when={when}");
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(openmind_survival::truncate(&line, 120).to_string())
}

/// Write the tool-warn file once per message id (guardian-tick.py rule).
pub fn write_tool_warn(out: &Path, line: &str) -> Result<bool, String> {
    let id = line.split_whitespace().nth(1).unwrap_or("");
    if let Ok(old) = std::fs::read_to_string(out) {
        if old.starts_with(&format!("WARN {id} ")) {
            return Ok(false);
        }
    }
    write_atomic(out, &format!("{line}\n")).map(|_| true)
}

pub struct BeatOut {
    pub stamp: String,
    pub rows: Vec<Row>,
    pub events: Vec<Event>,
    pub suggestions: Vec<Suggestion>,
    pub report: Value,
}

impl BeatOut {
    /// guardian-tick.py stdout line: `<stamp> watch` or `<stamp> down a,b`.
    pub fn line(&self) -> String {
        let down: Vec<&str> = self
            .rows
            .iter()
            .filter(|r| !r.ok())
            .map(|r| r.name.as_str())
            .collect();
        if down.is_empty() {
            format!("{} watch ", self.stamp)
        } else {
            format!("{} down {}", self.stamp, down.join(","))
        }
    }
}

fn streak_path(s: &Settings) -> PathBuf {
    s.state_dir.join("streak.json")
}

fn load_streak(s: &Settings) -> BTreeMap<String, u32> {
    std::fs::read_to_string(streak_path(s))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Seconds since a stamp file's number, or None when absent.
fn since_stamp(path: &Path) -> Option<u64> {
    let last: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    let now = (now_ms() / 1000) as u64;
    Some(now.saturating_sub(last))
}

/// One beat: probe, store, streaks, suggestions, report, beat file and the
/// optional sinks (tool-warn, activity, nerve, digest).
pub fn beat(
    s: &Settings,
    specs: &[config::Spec],
    env: &Env,
    store: &mut dyn Store,
) -> Result<BeatOut, String> {
    std::fs::create_dir_all(&s.state_dir).map_err(|e| format!("{}: {e}", s.state_dir.display()))?;
    let rows: Vec<Row> = specs.iter().map(|sp| probe::run(sp, env)).collect();
    let stamp = zagreb_time::now_rfc3339();
    let events = store.record(&rows, &stamp)?;

    let mut streak = load_streak(s);
    let mut obs = Vec::new();
    for r in &rows {
        let n = if r.ok() {
            0
        } else {
            streak.get(&r.name).copied().unwrap_or(0) + 1
        };
        streak.insert(r.name.clone(), n);
        obs.push(Observation {
            name: r.name.clone(),
            ok: r.ok(),
            fails_in_row: n,
        });
    }
    write_atomic(
        &streak_path(s),
        &serde_json::to_string(&streak).map_err(|e| e.to_string())?,
    )?;
    let suggestions = decide(
        &obs,
        Policy {
            warn_after: s.warn_after,
            restart_after: s.restart_after,
        },
    );

    let report = json!({
        "ts": stamp,
        "host_ok": rows.iter().all(Row::ok),
        "results": rows.iter().map(|r| json!({
            "name": r.name, "ok": r.ok(), "status": r.status, "ms": r.ms, "detail": r.detail,
        })).collect::<Vec<_>>(),
        "events": events.iter().map(|e| json!({
            "service": e.service, "event": if e.opened {"opened"} else {"resolved"}, "severity": e.severity,
        })).collect::<Vec<_>>(),
        "suggestions": suggestions.iter().map(|g| json!({
            "service": g.service, "level": g.level.as_str(), "text": g.text,
        })).collect::<Vec<_>>(),
    });
    let body = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    write_atomic(&s.state_dir.join("report.json"), &(body + "\n"))?;
    write_atomic(&s.state_dir.join("beat"), &format!("{stamp}\n"))?;
    if !s.beat.is_empty() {
        write_atomic(Path::new(&s.beat), &format!("{}\n", now_ms() / 1000))?;
    }

    if !s.nerve.is_empty() {
        let changed: Vec<&Row> = rows
            .iter()
            .filter(|r| events.iter().any(|e| e.service == r.name))
            .collect();
        for r in changed {
            let pr = ProbeResult {
                name: r.name.clone(),
                ok: r.ok(),
                detail: r.detail.clone(),
                ts: stamp.clone(),
            };
            append_line(Path::new(&s.nerve), &nerve_line("guardian", &pr))?;
        }
    }

    if !s.activity.is_empty() {
        let fails: Vec<&str> = rows
            .iter()
            .filter(|r| !r.ok())
            .map(|r| r.name.as_str())
            .collect();
        let stampf = s.state_dir.join("last-activity.ts");
        let due = since_stamp(&stampf).is_none_or(|age| age >= s.activity_every_s);
        if !fails.is_empty() && due {
            let row = json!({"at": now_ms() as u64, "kind": "health", "text": format!("health fail {}", fails.join(","))});
            append_line(Path::new(&s.activity), &row.to_string())?;
            write_atomic(&stampf, &format!("{}\n", now_ms() / 1000))?;
        }
    }

    if !s.activity.is_empty() && !s.digest_text.is_empty() {
        let day = stamp.get(..10).unwrap_or("").to_string();
        let f = s.state_dir.join("last-digest.day");
        if std::fs::read_to_string(&f)
            .map(|t| t.trim().to_string())
            .ok()
            != Some(day.clone())
        {
            write_atomic(&f, &format!("{day}\n"))?;
            let text = s.digest_text.replace("{day}", &day);
            let row = json!({"at": now_ms() as u64, "kind": "digest", "text": text});
            append_line(Path::new(&s.activity), &row.to_string())?;
        }
    }

    if !s.hermes_state_db.is_empty() {
        tool_warn(s);
    }

    Ok(BeatOut {
        stamp,
        rows,
        events,
        suggestions,
        report,
    })
}

#[cfg(feature = "sqlite")]
fn tool_warn(s: &Settings) {
    let out = if s.tool_warn_out.is_empty() {
        s.state_dir.join("tool-warn.txt")
    } else {
        PathBuf::from(&s.tool_warn_out)
    };
    if let Some(line) = store::sqlite::tool_warn_line(Path::new(&s.hermes_state_db)) {
        let _ = write_tool_warn(&out, &line);
    }
}

#[cfg(not(feature = "sqlite"))]
fn tool_warn(_s: &Settings) {}

/// Build the configured store.
pub fn open_store(s: &Settings) -> Result<Box<dyn Store>, String> {
    match s.store.as_str() {
        "sqlite" => open_sqlite(s),
        _ => Ok(Box::new(store::JsonlStore {
            dir: s.state_dir.clone(),
            keep_rows: s.keep_rows,
            keep_resolved: 500,
        })),
    }
}

#[cfg(feature = "sqlite")]
fn open_sqlite(s: &Settings) -> Result<Box<dyn Store>, String> {
    let db = if s.db.is_empty() {
        s.state_dir.join("guardian.db")
    } else {
        PathBuf::from(&s.db)
    };
    Ok(Box::new(store::sqlite::SqliteStore::open(
        &db,
        s.keep_rows,
    )?))
}

#[cfg(not(feature = "sqlite"))]
fn open_sqlite(_s: &Settings) -> Result<Box<dyn Store>, String> {
    Err("store = \"sqlite\" needs a build with --features sqlite".into())
}
