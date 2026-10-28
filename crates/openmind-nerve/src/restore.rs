// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Restore-test freshness/result component (wave-4 slot 378, gap found by
//! slot 361: the live nerve had no idea whether a backup restore test ran or
//! passed).
//!
//! A component of `"kind": "restore"` READS ONLY the restore-test log file
//! (`"log"`): the stdout+stderr of one `galion-backup restore-test` /
//! `galion-restore-test.sh` run, e.g. written by the staged drop-in
//! `StandardOutput=truncate:/var/log/galion/restore-test.log` +
//! `StandardError=inherit`. It never runs the restore test, never opens a
//! backup, never restarts anything (restarts are forced to 0, never
//! hard-down). The verdict only ever degrades (warn):
//!
//! * no log yet (never recorded) or log unreadable,
//! * last run FAILED (any `NAME: FAILED`, `check FAILED`, `NOT verified`,
//!   `did NOT match`, `integrity=` other than `ok`, `galion-backup:
//!   restore-test: ...` error line or an `error` line; also when the old .sh
//!   still printed `restore test ok` after a FAILED line, F-326-1),
//! * last run has no verdict line (interrupted / crashed),
//! * `restore test ok` but a `MISSING: /path` presence line,
//! * stale: the log is older than `max_age_h` (default 26 h for the staged
//!   daily 03:47 timer + 10 min jitter).
//!
//! The log is read from its last [`TAIL_BYTES`] bytes. When several runs were
//! appended to one file, the last finished run counts; an unfinished tail
//! younger than [`RUNNING_GRACE_S`] is a run in progress (previous verdict
//! kept), older than that it is an interrupted run.

use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Bytes read from the end of the log (one run prints ~10 lines).
pub const TAIL_BYTES: u64 = 64 * 1024;
/// An unfinished last run younger than this is "running", not "interrupted"
/// (the staged unit has TimeoutStartSec=15min).
pub const RUNNING_GRACE_S: u64 = 900;
/// Default `max_age_h`.
pub const DEFAULT_MAX_AGE_H: u64 = 26;
/// Max length of a log line copied into reasons / JSON.
const LINE_MAX: usize = 200;

/// Verdict of one run (one segment of the log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// "ok" | "failed" | "incomplete" | "empty"
    pub state: &'static str,
    /// The deciding line (sanitized): the first failure line, else the
    /// verdict line, else the last line.
    pub line: String,
    /// Archive named by `restore test ok: <archive>`.
    pub archive: Option<String>,
    /// `MISSING: /path` presence lines (paths only).
    pub missing: Vec<String>,
    /// Number of failure lines in the run.
    pub failures: usize,
    /// The run ended with a verdict line.
    pub finished: bool,
}

fn clean(l: &str) -> String {
    let s: String = l
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let s = s.trim();
    if s.chars().count() > LINE_MAX {
        let mut t: String = s.chars().take(LINE_MAX).collect();
        t.push('…');
        t
    } else {
        s.to_string()
    }
}

/// A line that ends a run.
fn is_terminal(l: &str) -> bool {
    l.starts_with("restore test ok") || l.starts_with("galion-backup: restore-test:")
}

/// A line that means the run failed.
pub fn is_failure(l: &str) -> bool {
    if l.starts_with("present: ") || l.starts_with("MISSING: ") {
        return false;
    }
    if l.starts_with("galion-backup: restore-test:") {
        return true;
    }
    if let Some(rest) = l.split_once(" integrity=").map(|x| x.1) {
        if !rest.starts_with("ok") {
            return true;
        }
    }
    let low = l.to_ascii_lowercase();
    l.contains(": FAILED")
        || l.contains("check FAILED")
        || l.contains("NOT verified")
        || l.contains("did NOT match")
        || low.contains("error")
        || low.contains("interrupted by signal")
}

/// Judge one run's lines.
pub fn judge(lines: &[&str]) -> Run {
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty())
        .collect();
    let finished = lines.last().is_some_and(|l| is_terminal(l));
    let failures: Vec<&&str> = lines.iter().filter(|l| is_failure(l)).collect();
    let missing: Vec<String> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("MISSING: "))
        .map(clean)
        .collect();
    let archive = lines
        .last()
        .and_then(|l| l.strip_prefix("restore test ok:"))
        .map(clean);
    let (state, line) = if lines.is_empty() {
        ("empty", String::new())
    } else if let Some(f) = failures.first() {
        // The last stderr line of the Rust tool says the most; prefer it.
        let best = failures
            .iter()
            .rev()
            .find(|l| l.starts_with("galion-backup: restore-test:"))
            .unwrap_or(f);
        ("failed", clean(best))
    } else if finished {
        ("ok", clean(lines[lines.len() - 1]))
    } else {
        ("incomplete", clean(lines[lines.len() - 1]))
    };
    Run {
        state,
        line,
        archive: if state == "ok" { archive } else { None },
        missing,
        failures: failures.len(),
        finished,
    }
}

/// Split the log into runs: each finished run ends at a verdict line; an
/// unfinished tail (if any) is the last element.
pub fn runs(text: &str) -> Vec<Run> {
    let all: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    let mut start = 0;
    for (i, l) in all.iter().enumerate() {
        if is_terminal(l.trim_end_matches('\r')) {
            out.push(judge(&all[start..=i]));
            start = i + 1;
        }
    }
    let tail = judge(&all[start..]);
    if tail.state != "empty" || out.is_empty() {
        out.push(tail);
    }
    out
}

/// Read the log (last [`TAIL_BYTES`]) and describe it as the `extra.restore`
/// JSON of a sample. `now` is unix seconds. Never fails: errors become
/// `state: "never"` / `"unreadable"`.
pub fn read_log(path: &Path, now: u64, max_age_h: u64) -> Value {
    let base = json!({"log": path.display().to_string(), "max_age_h": max_age_h});
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => {
            return with(
                base,
                json!({"state": "unreadable", "error": "not a regular file"}),
            )
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return with(base, json!({"state": "never"}))
        }
        Err(e) => {
            return with(
                base,
                json!({"state": "unreadable", "error": e.kind().to_string()}),
            )
        }
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let text = match tail(path, meta.len()) {
        Ok(t) => t,
        Err(e) => {
            return with(
                base,
                json!({"state": "unreadable", "error": e.kind().to_string(), "mtime": mtime}),
            )
        }
    };
    let age = now.saturating_sub(mtime);
    let rs = runs(&text);
    let last = rs.last().cloned().expect("runs() is never empty");
    // An unfinished tail younger than the grace is a run in progress: keep
    // the previous finished verdict (if any).
    let (run, running) = if !last.finished && last.state == "incomplete" && age < RUNNING_GRACE_S {
        match rs.iter().rev().nth(1) {
            Some(prev) => (prev.clone(), true),
            None => (last, true),
        }
    } else {
        (last, false)
    };
    let state = if running && !run.finished {
        "running"
    } else {
        run.state
    };
    with(
        base,
        json!({
            "state": state,
            "running": running,
            "mtime": mtime,
            "last_run": crate::util::ts(mtime),
            "age_s": age,
            "line": run.line,
            "archive": run.archive,
            "missing": run.missing,
            "failures": run.failures,
            "runs_in_tail": rs.iter().filter(|r| r.finished).count(),
            "bytes": meta.len(),
        }),
    )
}

fn with(mut a: Value, b: Value) -> Value {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
    a
}

fn tail(path: &Path, len: u64) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let from = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(from))?;
    let mut b = Vec::with_capacity((len - from) as usize);
    f.take(TAIL_BYTES).read_to_end(&mut b)?;
    let mut s = String::from_utf8_lossy(&b).into_owned();
    if from > 0 {
        // Drop the partial first line.
        if let Some(i) = s.find('\n') {
            s.drain(..=i);
        }
    }
    Ok(s)
}

/// Warn reasons for `extra.restore` at `now` (all "degrading"; empty = healthy).
pub fn reasons(r: &Value, now: u64) -> Vec<String> {
    let mut v = vec![];
    let log = r.get("log").and_then(Value::as_str).unwrap_or("?");
    let max_h = r
        .get("max_age_h")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_AGE_H);
    let line = r.get("line").and_then(Value::as_str).unwrap_or("");
    match r.get("state").and_then(Value::as_str).unwrap_or("never") {
        "never" => {
            v.push(format!(
                "restore test never recorded (no log at {log}); backups are unverified"
            ));
            return v;
        }
        "unreadable" => {
            let e = r.get("error").and_then(Value::as_str).unwrap_or("?");
            v.push(format!("restore-test log {log} unreadable ({e})"));
            return v;
        }
        "failed" => v.push(format!("restore test FAILED: {line}")),
        "incomplete" => v.push(format!(
            "restore test log has no verdict (interrupted run?); last line: {line}"
        )),
        "empty" => v.push(format!("restore-test log {log} is empty")),
        "running" => {}
        _ => {
            if let Some(m) = r.get("missing").and_then(Value::as_array) {
                if !m.is_empty() {
                    let names: Vec<&str> = m.iter().filter_map(Value::as_str).take(4).collect();
                    v.push(format!(
                        "restore test ok but {} path(s) MISSING: {}",
                        m.len(),
                        names.join(", ")
                    ));
                }
            }
        }
    }
    if let Some(mt) = r.get("mtime").and_then(Value::as_u64) {
        let age = now.saturating_sub(mt);
        if age > max_h * 3600 {
            v.push(format!(
                "restore test stale: last run {} h ago at {} (max {max_h} h)",
                age / 3600,
                crate::util::ts(mt)
            ));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "checksums ok\nverify db/hermes-rs-state.db integrity=ok tables=7\nverify db/memory-vectors.db integrity=ok tables=3\nfiles restored: 40\npresent: /var/lib/memory-tencentdb/persona.md\npresent: /var/lib/hermes-rs\npresent: /root/galion-secrets/admin-device.json\npresent: /etc/openmind-memory.env\nrestore test ok: galion-20261028-072141+0100.tar.gz\n";

    #[test]
    fn ok_run() {
        let r = runs(OK);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].state, "ok");
        assert!(r[0].finished);
        assert_eq!(
            r[0].archive.as_deref(),
            Some("galion-20261028-072141+0100.tar.gz")
        );
        assert!(r[0].missing.is_empty());
    }

    #[test]
    fn rust_hard_fail_and_sh_let_through() {
        // Rust tool (slot 361): FAILED lines on stdout, one stderr verdict.
        let rs = "./db/MAP: FAILED\ngalion-backup: restore-test: /var/backups/galion/galion-X.tar.gz: checksum check FAILED: 1 mismatched [./db/MAP], 0 unreadable, 0 malformed line(s), 0 unsafe name(s), 0 unlisted (5 of 6 listed files ok); this backup is NOT verified, do not restore from it\n";
        let r = runs(rs);
        assert_eq!(r.last().unwrap().state, "failed");
        assert!(r
            .last()
            .unwrap()
            .line
            .starts_with("galion-backup: restore-test:"));
        // Live .sh (F-326-1): FAILED + warning, then still `restore test ok`.
        let sh = "./db/MAP: FAILED\nsha256sum: WARNING: 1 computed checksum did NOT match\nverify db/a.db integrity=ok tables=1\nfiles restored: 3\nrestore test ok: galion-X.tar.gz\n";
        let r = runs(sh);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].state, "failed", "{r:?}");
        assert_eq!(r[0].failures, 2);
        assert_eq!(r[0].archive, None);
        // Integrity, gzip, signal.
        for bad in [
            "checksums ok\nverify db/a.db integrity=*** in database main *** tables=1\n",
            "galion-backup: restore-test: /x.tar.gz: gzip check FAILED: crc error; this backup is NOT verified\n",
            "galion-backup: restore-test: interrupted by signal 15\n",
            "gzip: stdin: invalid compressed data--crc error\ntar: Error is not recoverable: exiting now\n",
        ] {
            assert_eq!(runs(bad).last().unwrap().state, "failed", "{bad}");
        }
    }

    #[test]
    fn missing_incomplete_empty() {
        let m = OK.replace(
            "present: /etc/openmind-memory.env",
            "MISSING: /etc/openmind-memory.env",
        );
        let r = runs(&m);
        assert_eq!(r[0].state, "ok");
        assert_eq!(r[0].missing, ["/etc/openmind-memory.env"]);
        let cut = "checksums ok\nverify db/a.db integrity=ok tables=1\n";
        assert_eq!(runs(cut)[0].state, "incomplete");
        assert_eq!(runs("")[0].state, "empty");
        assert_eq!(runs("\n\n")[0].state, "empty");
    }

    #[test]
    fn appended_runs_last_counts() {
        let two = format!("{}./db/MAP: FAILED\ngalion-backup: restore-test: /x: checksum check FAILED: 1 mismatched; NOT verified\n{OK}", OK);
        let r = runs(&two);
        assert_eq!(
            r.iter().map(|r| r.state).collect::<Vec<_>>(),
            ["ok", "failed", "ok"]
        );
        let partial = format!("{OK}checksums ok\n");
        let r = runs(&partial);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].state, "incomplete");
    }

    #[test]
    fn line_sanitized_and_capped() {
        let long = format!("galion-backup: restore-test: \x1b[31m{}\n", "x".repeat(500));
        let r = runs(&long);
        assert!(!r[0].line.contains('\x1b'));
        assert!(r[0].line.chars().count() <= LINE_MAX + 1);
    }

    fn tmpd(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nerve-378-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn read_log_states_and_reasons() {
        let d = tmpd("unit");
        let p = d.join("restore-test.log");
        let now = crate::util::now_secs();
        let v = read_log(&p, now, 26);
        assert_eq!(v["state"], "never");
        assert!(reasons(&v, now)[0].contains("never recorded"));
        std::fs::write(&p, OK).unwrap();
        let v = read_log(&p, now, 26);
        assert_eq!(v["state"], "ok", "{v}");
        assert!(reasons(&v, now).is_empty(), "{v}");
        // Same log, 27 h later: stale.
        let r = reasons(&v, now + 27 * 3600);
        assert_eq!(r.len(), 1);
        assert!(
            r[0].starts_with("restore test stale: last run 27 h ago"),
            "{r:?}"
        );
        // A run in progress right after an ok run keeps ok ("running").
        std::fs::write(&p, format!("{OK}checksums ok\n")).unwrap();
        let v = read_log(&p, now, 26);
        assert_eq!(v["state"], "ok", "{v}");
        assert_eq!(v["running"], true);
        // ...but 20 min later the same tail is an interrupted run.
        let v = read_log(&p, now + 1200, 26);
        assert_eq!(v["state"], "incomplete", "{v}");
        // A fresh log with only a partial first run: running, no warn.
        std::fs::write(&p, "checksums ok\n").unwrap();
        let v = read_log(&p, now, 26);
        assert_eq!(v["state"], "running");
        assert!(reasons(&v, now).is_empty());
        // Directory instead of a file.
        let v = read_log(&d, now, 26);
        assert_eq!(v["state"], "unreadable");
        // Tail window: a huge log only reads the end.
        let mut big = "noise line\n".repeat(20_000);
        big.push_str(OK);
        std::fs::write(&p, &big).unwrap();
        let v = read_log(&p, now, 26);
        assert_eq!(v["state"], "ok", "{v}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
