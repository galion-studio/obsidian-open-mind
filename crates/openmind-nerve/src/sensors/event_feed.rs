// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `event-feed` sensor (cortex plan T2.10): the nerve's own `POST /event`
//! JSONL log (and other JSONL event files, e.g. the survival guardian's)
//! read back as cortex findings.
//!
//! Gaia heals, Gaia service probes, survival probe lines and pushed B33
//! cortex events arrive as JSONL records. This sensor tails those files
//! (read-only, only when [`Input::live`]), keeps a small per-subject state
//! (latest problem per service / probe / source) and reports it as findings
//! every cycle until a resolving event arrives or the window expires.
//!
//! | event | finding | severity | clears on |
//! |---|---|---|---|
//! | `gaia.heal.capped` | bleeding `heal-capped-<svc>` (+ signal `gaia:<svc>`) | critical | `gaia.heal.applied`, `service.probe` up |
//! | `gaia.heal.failed` | bleeding `heal-failed-<svc>` (+ signal) | error | same |
//! | `gaia.heal.refused` | bleeding `heal-refused-<svc>` | warn | same |
//! | `service.probe` not up | bleeding `service-down-<svc>` | warn | `service.probe` up, `gaia.heal.applied` |
//! | `probe` with `ok:false` (survival) | bleeding `probe-failed-<name>` | warn | same probe `ok:true` |
//! | B33 `vulnerability`/`bleeding`/`ddos` | same kind, `ev-<source>-<id>` | its severity (B41 or nerve spelling) | same source+kind at `info` |
//! | any other kind at error/critical | bleeding `event-errors-<source>-<kind>` | error (capped: unauthenticated input) | window expiry |
//!
//! Settings (sensor off unless the file list is set):
//! - `NERVE_EVENT_FEED_FILE`: comma-separated JSONL paths (max 4), usually
//!   the same path as `NERVE_EVENT_LOG` / `--log`.
//! - `NERVE_EVENT_FEED_WINDOW_S` (3600): a problem not refreshed for this
//!   long is dropped; older records are ignored on first read.
//! - `NERVE_EVENT_FEED_SOURCES`: optional comma list; only these event
//!   `source`s are read (others counted as `skipped`).
//!
//! Safety: `POST /event` has no authentication yet (cortex plan section 8
//! item 6), so anything on the box can write records. The sensor therefore
//! caps unknown kinds at error, keeps at most [`MAX_KEYS`] subjects, reads at
//! most [`MAX_READ`] bytes per file per cycle, cleans every name it puts in
//! an id, component or title (no raw detail text or addresses leave in
//! titles; event detail stays in loopback-only evidence), and never acts.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Most subjects kept; the oldest is dropped beyond this.
pub const MAX_KEYS: usize = 128;
/// Most bytes read from one file in one cycle.
pub const MAX_READ: u64 = 1024 * 1024;
/// First read starts at most this far before the end of the file.
pub const BACKFILL: u64 = 256 * 1024;
/// Longer lines are skipped (the nerve writer caps bodies far lower).
pub const MAX_LINE: usize = 64 * 1024;
const MAX_FILES: usize = 4;
const DEFAULT_WINDOW_S: u64 = 3600;

/// Where one file was read up to.
#[derive(Debug, Default)]
struct Tail {
    path: PathBuf,
    /// None until the first read.
    offset: Option<u64>,
    ino: u64,
    partial: Vec<u8>,
}

/// One remembered problem.
#[derive(Debug, Clone, PartialEq)]
struct Problem {
    kind: FindingKind,
    id: String,
    sev: Sev,
    component: String,
    title: String,
    fix: &'static str,
    source: String,
    cause: Option<String>,
    first: u64,
    last: u64,
    count: u64,
    detail: String,
}

/// What one record does to the state.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    Set(String, Problem),
    Clear(Vec<String>),
    Ignore,
}

/// Counters shown in the evidence of nothing; kept for tests and `debug`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Stats {
    pub lines: u64,
    pub bad: u64,
    pub skipped: u64,
    pub old: u64,
    pub rotations: u64,
}

/// The `event-feed` sensor.
pub struct EventFeed {
    tails: Vec<Tail>,
    window_s: u64,
    sources: Option<Vec<String>>,
    state: BTreeMap<String, Problem>,
    pub stats: Stats,
}

impl EventFeed {
    /// A feed over `files` with a `window_s` expiry and an optional source
    /// allow-list.
    pub fn new(files: Vec<PathBuf>, window_s: u64, sources: Option<Vec<String>>) -> Self {
        EventFeed {
            tails: files
                .into_iter()
                .take(MAX_FILES)
                .map(|path| Tail {
                    path,
                    ..Tail::default()
                })
                .collect(),
            window_s: window_s.max(60),
            sources,
            state: BTreeMap::new(),
            stats: Stats::default(),
        }
    }

    /// `NERVE_EVENT_FEED_FILE` (+ `_WINDOW_S`, `_SOURCES`); None = off.
    pub fn from_env(env: Env) -> Option<Self> {
        let files: Vec<PathBuf> = env("NERVE_EVENT_FEED_FILE")?
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .collect();
        if files.is_empty() {
            return None;
        }
        let window = env("NERVE_EVENT_FEED_WINDOW_S")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_WINDOW_S);
        let sources = env("NERVE_EVENT_FEED_SOURCES").map(|v| {
            v.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        });
        Some(Self::new(files, window, sources))
    }

    /// Fold one JSONL line (already read) into the state. Public for
    /// replay-style tests; the live path calls it for every new line.
    pub fn ingest_line(&mut self, line: &str, now: u64) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.stats.lines += 1;
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            self.stats.bad += 1;
            return;
        };
        if !v.is_object() {
            self.stats.bad += 1;
            return;
        }
        let src = clean(v["source"].as_str().unwrap_or(""), 48);
        if let Some(allow) = &self.sources {
            if !allow.contains(&src) {
                self.stats.skipped += 1;
                return;
            }
        }
        let t = v["ts"]
            .as_str()
            .and_then(parse_rfc3339)
            .map(|t| t.min(now))
            .unwrap_or(now);
        if t + self.window_s < now {
            self.stats.old += 1;
            return;
        }
        match classify(&v, &src, t) {
            Action::Ignore => {}
            Action::Clear(keys) => {
                for k in keys {
                    // An older clear must not undo a newer problem.
                    if self.state.get(&k).is_some_and(|p| p.last <= t) {
                        self.state.remove(&k);
                    }
                }
            }
            Action::Set(k, mut p) => {
                if let Some(old) = self.state.get(&k) {
                    if old.last > t {
                        return;
                    }
                    p.first = old.first.min(t);
                    p.count = old.count + 1;
                }
                self.state.insert(k, p);
                while self.state.len() > MAX_KEYS {
                    let oldest = self
                        .state
                        .iter()
                        .min_by_key(|(_, p)| p.last)
                        .map(|(k, _)| k.clone());
                    match oldest {
                        Some(k) => self.state.remove(&k),
                        None => break,
                    };
                }
            }
        }
    }

    /// Read new bytes of every file (live cycles only).
    fn read_files(&mut self, now: u64) {
        let mut lines = Vec::new();
        for t in self.tails.iter_mut() {
            let r = read_new(t, &mut lines);
            self.stats.rotations += r;
        }
        for l in lines {
            self.ingest_line(&l, now);
        }
    }

    /// Drop problems not refreshed within the window.
    fn expire(&mut self, now: u64) {
        let w = self.window_s;
        self.state.retain(|_, p| p.last + w >= now);
    }

    fn report(&self, now: u64, out: &mut Sink<'_>) {
        for p in self.state.values() {
            let mut ev = vec![
                format!(
                    "{} event(s); first {}, last {} ({} ago)",
                    p.count,
                    zagreb_time::from_unix(p.first as i64),
                    zagreb_time::from_unix(p.last as i64),
                    crate::cortex::dur(now.saturating_sub(p.last))
                ),
                format!("event source `{}`", p.source),
            ];
            if !p.detail.is_empty() {
                ev.push(format!("detail: {}", p.detail));
            }
            let mut f = Finding::new(p.kind, &p.id, p.sev, &p.component, &p.title)
                .with_evidence(ev)
                .with_fix(p.fix)
                .with_source(format!("event:{}", p.source));
            if let Some(c) = &p.cause {
                f = f.with_cause(c);
                out.signal(Signal::new(
                    c,
                    &p.component,
                    p.sev,
                    format!("{} ({} event(s) in the window)", p.title, p.count),
                ));
            }
            out.finding(f);
        }
    }
}

impl Sensor for EventFeed {
    fn name(&self) -> &'static str {
        "event-feed"
    }

    fn sense(&mut self, input: &Input<'_>, out: &mut Sink<'_>) {
        if input.live {
            self.read_files(input.now);
        }
        self.expire(input.now);
        self.report(input.now, out);
    }
}

/// Read what was appended to `t.path` since the last call into `lines`.
/// Returns 1 when a rotation or truncation was seen.
fn read_new(t: &mut Tail, lines: &mut Vec<String>) -> u64 {
    let Ok(mut f) = File::open(&t.path) else {
        return 0;
    };
    let Ok(md) = f.metadata() else {
        return 0;
    };
    let (len, ino) = (md.len(), inode(&md));
    let mut rotated = 0;
    let start = match t.offset {
        None => {
            let s = len.saturating_sub(BACKFILL);
            if s > 0 {
                t.partial.clear();
                // Started mid-file: the first (partial) line is dropped below.
                t.partial.extend_from_slice(b"\0skip");
            }
            s
        }
        Some(off) if ino != t.ino || len < off => {
            rotated = 1;
            // Finish the rotated file first (the writer renames to `.1`).
            let r1 = rotated_sibling(&t.path);
            if let Ok(mut old) = File::open(&r1) {
                if old.metadata().map(|m| inode(&m)).ok() == Some(t.ino) {
                    let mut tmp = Tail {
                        path: r1,
                        offset: Some(off),
                        ino: t.ino,
                        partial: std::mem::take(&mut t.partial),
                    };
                    let _ = read_range(&mut old, &mut tmp, off, lines);
                    flush_partial(&mut tmp, lines);
                }
            }
            t.partial.clear();
            0
        }
        Some(off) => off,
    };
    t.ino = ino;
    match read_range(&mut f, t, start, lines) {
        Ok(end) => t.offset = Some(end),
        Err(_) => t.offset = Some(start),
    }
    rotated
}

/// Read `[start, start + MAX_READ)` and split complete lines; the trailing
/// partial line is kept in `t.partial`. Returns the new offset.
fn read_range(
    f: &mut File,
    t: &mut Tail,
    start: u64,
    lines: &mut Vec<String>,
) -> std::io::Result<u64> {
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    let n = f.take(MAX_READ).read_to_end(&mut buf)? as u64;
    let mut rest: &[u8] = &buf;
    while let Some(i) = rest.iter().position(|&b| b == b'\n') {
        let mut line = std::mem::take(&mut t.partial);
        let skip = line.starts_with(b"\0skip");
        line.extend_from_slice(&rest[..i]);
        rest = &rest[i + 1..];
        if !skip && line.len() <= MAX_LINE {
            lines.push(String::from_utf8_lossy(&line).into_owned());
        }
    }
    if t.partial.len() + rest.len() <= MAX_LINE {
        t.partial.extend_from_slice(rest);
    } else {
        // Oversized line: remember to drop it when its newline arrives.
        t.partial.clear();
        t.partial.extend_from_slice(b"\0skip");
    }
    Ok(start + n)
}

/// A rotated file ends at its last newline; a leftover is a whole line.
fn flush_partial(t: &mut Tail, lines: &mut Vec<String>) {
    let p = std::mem::take(&mut t.partial);
    if !p.is_empty() && !p.starts_with(b"\0skip") {
        lines.push(String::from_utf8_lossy(&p).into_owned());
    }
}

fn rotated_sibling(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".1");
    PathBuf::from(s)
}

#[cfg(unix)]
fn inode(md: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(md)
}

#[cfg(not(unix))]
fn inode(_md: &std::fs::Metadata) -> u64 {
    0
}

/// Keep `[A-Za-z0-9._-]`, map anything else to `_`, cap at `max` chars.
fn clean(s: &str, max: usize) -> String {
    let c: String = s
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(max)
        .collect();
    if c.is_empty() {
        "unknown".into()
    } else {
        c
    }
}

/// A human title from untrusted text: printable, digits-and-dots runs that
/// look like addresses masked, capped.
fn clean_title(s: &str, max: usize) -> String {
    let printable: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let masked: Vec<String> = printable
        .split_whitespace()
        .map(|w| {
            let dots = w.matches('.').count() + w.matches(':').count();
            let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
            if dots >= 2 && digits >= 4 {
                "<addr>".to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    let t = masked.join(" ");
    if t.chars().count() > max {
        format!("{}...", t.chars().take(max).collect::<String>())
    } else {
        t
    }
}

fn short(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(200).collect();
    s
}

/// `data.<k>` or top-level `<k>` (survival lines have no `data`).
fn field<'v>(v: &'v Value, k: &str) -> &'v Value {
    let d = &v["data"][k];
    if d.is_null() {
        &v[k]
    } else {
        d
    }
}

fn problem(
    kind: FindingKind,
    id: String,
    sev: Sev,
    component: &str,
    title: String,
    src: &str,
    t: u64,
) -> Problem {
    Problem {
        kind,
        id,
        sev,
        component: component.to_string(),
        title,
        fix: "",
        source: src.to_string(),
        cause: None,
        first: t,
        last: t,
        count: 1,
        detail: String::new(),
    }
}

/// Map one record to a state change.
fn classify(v: &Value, src: &str, t: u64) -> Action {
    let kind = v["kind"].as_str().unwrap_or("");
    let sev_raw = v["severity"].as_str().unwrap_or("");
    let sev = Sev::parse(&sev_raw.to_ascii_lowercase());
    if let Some(what) = kind.strip_prefix("gaia.heal.") {
        let svc = clean(field(v, "service").as_str().unwrap_or(""), 48);
        let heal = format!("heal:{svc}");
        let down = format!("down:{svc}");
        let (id, sev, title, fix) = match what {
            "applied" => return Action::Clear(vec![heal, down]),
            "capped" => {
                let n = field(v, "restarts_last_hour").as_u64().unwrap_or(0);
                (
                    format!("heal-capped-{svc}"),
                    Sev::Critical,
                    format!("{svc}: Gaia restart cap reached ({n} restarts in the last hour), still down"),
                    "the service crash-loops: read its log under the Gaia state dir and fix the cause; Gaia will not restart it again until the cap window passes",
                )
            }
            "failed" => (
                format!("heal-failed-{svc}"),
                Sev::Error,
                format!("{svc}: Gaia heal failed (restart did not bring it back)"),
                "read the service log under the Gaia state dir; the start command or a dependency is broken",
            ),
            "refused" => {
                let stage = clean(field(v, "stage").as_str().unwrap_or(""), 24);
                (
                    format!("heal-refused-{svc}"),
                    Sev::Warn,
                    format!("{svc}: Gaia heal refused at stage {stage}"),
                    "the heal needs a valid HADES approval or the target is out of scope; approving a restart is an owner decision",
                )
            }
            _ => return generic(kind, sev, src, t),
        };
        let mut p = problem(FindingKind::Bleeding, id, sev, &svc, title, src, t);
        p.fix = fix;
        p.detail = short(field(v, "reason").as_str().unwrap_or(""));
        if sev >= Sev::Error {
            p.cause = Some(format!("gaia:{svc}"));
        }
        return Action::Set(heal, p);
    }
    if kind == "service.probe" {
        let svc = clean(field(v, "service").as_str().unwrap_or(""), 48);
        let state = field(v, "state").as_str().unwrap_or("");
        let key = format!("down:{svc}");
        if state == "up" || (state.is_empty() && sev == Some(Sev::Info)) {
            return Action::Clear(vec![key, format!("heal:{svc}")]);
        }
        let st = clean(if state.is_empty() { "down" } else { state }, 24);
        let mut p = problem(
            FindingKind::Bleeding,
            format!("service-down-{svc}"),
            Sev::Warn,
            &svc,
            format!("{svc} is {st} (Gaia service probe)"),
            src,
            t,
        );
        p.fix = "Gaia's heal loop owns the restart (HADES-approved); check the service log if it stays down";
        p.detail = short(field(v, "detail").as_str().unwrap_or(""));
        return Action::Set(key, p);
    }
    if kind == "probe" {
        let name = clean(field(v, "name").as_str().unwrap_or(""), 48);
        let key = format!("probe:{src}:{name}");
        return match field(v, "ok").as_bool() {
            Some(true) => Action::Clear(vec![key]),
            Some(false) => {
                let mut p = problem(
                    FindingKind::Bleeding,
                    format!("probe-failed-{name}"),
                    Sev::Warn,
                    &name,
                    format!("{src} probe {name} is failing"),
                    src,
                    t,
                );
                p.fix = "read the probe detail in the evidence; the survival guardian reports, it does not repair";
                p.detail = short(field(v, "detail").as_str().unwrap_or(""));
                Action::Set(key, p)
            }
            None => Action::Ignore,
        };
    }
    if let Some(fk) = FindingKind::parse(kind) {
        // A B33 NerveEvent (one finding, or a cortex `nerve_events` rollup).
        if src.starts_with("event_") {
            return Action::Ignore; // our own re-emitted findings: no loop
        }
        let key = format!("b33:{src}:{kind}");
        let Some(sev) = sev else {
            return Action::Ignore;
        };
        if sev == Sev::Info {
            return Action::Clear(vec![key]);
        }
        let d = if v["data"]["top"].is_object() {
            &v["data"]["top"]
        } else {
            &v["data"]
        };
        let id = clean(d["id"].as_str().unwrap_or(kind), 64);
        let comp = clean(d["component"].as_str().unwrap_or(src), 48);
        let title = clean_title(d["title"].as_str().unwrap_or(""), 120);
        let title = if title.is_empty() {
            format!("{src} reported a {kind} finding")
        } else {
            format!("{src}: {title}")
        };
        let mut p = problem(fk, format!("ev-{src}-{id}"), sev, &comp, title, src, t);
        p.fix = "see the pushing service's own report; the nerve only relays it";
        if fk == FindingKind::Ddos && sev >= Sev::Error {
            p.cause = Some(format!("gaia:{src}"));
        }
        return Action::Set(key, p);
    }
    generic(kind, sev, src, t)
}

/// Unknown kinds count only at error or above, capped at error.
fn generic(kind: &str, sev: Option<Sev>, src: &str, t: u64) -> Action {
    match sev {
        Some(s) if s >= Sev::Error => {
            let k = clean(kind, 48);
            let mut p = problem(
                FindingKind::Bleeding,
                format!("event-errors-{src}-{k}"),
                Sev::Error,
                src,
                format!("{src} pushed {k} event(s) at {}", s.as_str()),
                src,
                t,
            );
            p.fix = "read the events with GET /api/v1/events and the pushing service's log";
            Action::Set(format!("ev:{src}:{k}"), p)
        }
        _ => Action::Ignore,
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|+HH:MM|-HH:MM)` -> unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> { s.get(a..z)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    let off = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        c @ (b'+' | b'-') if i + 6 == b.len() && b[i + 3] == b':' => {
            let v = num(i + 1, i + 3)? * 3600 + num(i + 4, i + 6)? * 60;
            if *c == b'-' {
                -v
            } else {
                v
            }
        }
        _ => return None,
    };
    // Days from civil (H. Hinnant's algorithm, public domain description).
    let (yy, mm) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let t = days * 86_400 + h * 3600 + mi * 60 + se - off;
    u64::try_from(t).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortex::Cortex;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;
    use std::io::Write;

    // 2026-10-21 01:00:00+02:00
    const NOW: u64 = 1_792_537_200;

    fn ts(t: u64) -> String {
        zagreb_time::from_unix(t as i64)
    }

    fn ev(t: u64, src: &str, kind: &str, sev: &str, data: &str) -> String {
        format!(
            r#"{{"ts":"{}","source":"{src}","kind":"{kind}","severity":"{sev}","data":{data}}}"#,
            ts(t)
        )
    }

    fn run(f: EventFeed, now: u64) -> (Cortex, EventFeed) {
        let mut s = Sensors::empty();
        s.add(f);
        let logs = LogInsights::default();
        let cx = s.run(&Input::new(now, &[], &logs));
        // Take the sensor back out (tests only): rebuild from the registry is
        // not possible, so tests use `feed_cycle` below for multi-cycle work.
        (cx, EventFeed::new(vec![], 3600, None))
    }

    /// One cycle over an existing feed (live flag as given).
    fn feed_cycle(f: &mut EventFeed, now: u64, live: bool) -> Cortex {
        let mut cx = Cortex::default();
        let logs = LogInsights::default();
        let mut i = Input::new(now, &[], &logs);
        i.live = live;
        let mut sink = Sink::new(&mut cx, "event-feed");
        f.sense(&i, &mut sink);
        let order = |a: &Finding, b: &Finding| b.sev.cmp(&a.sev).then(a.id.cmp(&b.id));
        cx.bleeding.sort_by(order);
        cx.vulnerabilities.sort_by(order);
        cx
    }

    #[test]
    fn rfc3339_zagreb_offsets_and_dst_fold() {
        assert_eq!(parse_rfc3339("2026-10-21T01:00:00+02:00"), Some(NOW));
        assert_eq!(parse_rfc3339("2026-10-20T23:00:00Z"), Some(NOW));
        assert_eq!(parse_rfc3339("2026-10-20T23:00:00.123456Z"), Some(NOW));
        // DST fold 2026-10-25: 02:59:59+02:00 then 02:00:00+01:00.
        assert_eq!(
            parse_rfc3339("2026-10-25T02:00:00+01:00").unwrap()
                - parse_rfc3339("2026-10-25T02:59:59+02:00").unwrap(),
            1
        );
        for t in [NOW, 1_792_889_999, 1_792_890_000, 1_800_000_000] {
            assert_eq!(parse_rfc3339(&ts(t)), Some(t), "{}", ts(t));
        }
        for bad in [
            "",
            "2026-10-21",
            "2026-13-01T00:00:00Z",
            "2026-10-21T01:00:00+0200",
            "x026-10-21T01:00:00Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn gaia_heal_capped_then_applied_clears() {
        let mut f = EventFeed::new(vec![], 3600, None);
        f.ingest_line(&ev(NOW - 120, "gaia", "service.probe", "warn",
            r#"{"service":"t1","port":19761,"state":"down","detail":"connect 127.0.0.1:19761 refused","probe_id":"p"}"#), NOW);
        f.ingest_line(&ev(NOW - 60, "gaia", "gaia.heal.capped", "error",
            r#"{"service":"t1","port":19761,"restarts_last_hour":3,"cap":3,"retry_after_s":900,"probe_id":"p"}"#), NOW);
        let cx = feed_cycle(&mut f, NOW, false);
        let ids: Vec<&str> = cx.bleeding.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["heal-capped-t1", "service-down-t1"]);
        let cap = &cx.bleeding[0];
        assert_eq!(cap.sev, Sev::Critical);
        assert_eq!(cap.source, "event:gaia");
        assert_eq!(cap.cause.as_deref(), Some("gaia:t1"));
        assert!(cap.title.contains("3 restarts"), "{}", cap.title);
        assert_eq!(cx.signals.len(), 1);
        assert_eq!(cx.signals[0].cause, "gaia:t1");
        let down = &cx.bleeding[1];
        assert!(!down.title.contains("127.0.0.1"), "no address in titles");
        assert!(down.evidence.iter().any(|e| e.contains("refused")));
        // B33 out: one event per (source, kind), highest wins.
        let out = cx.nerve_events(NOW);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["severity"], "crit");
        assert!(!out[0].to_string().contains("127.0.0.1"));
        // Applied clears both.
        f.ingest_line(
            &ev(
                NOW + 5,
                "gaia",
                "gaia.heal.applied",
                "info",
                r#"{"service":"t1","pid":1}"#,
            ),
            NOW + 10,
        );
        let cx = feed_cycle(&mut f, NOW + 10, false);
        assert!(
            cx.bleeding.is_empty() && cx.signals.is_empty(),
            "{:?}",
            cx.bleeding
        );
    }

    #[test]
    fn out_of_order_clear_does_not_undo_newer_problem() {
        let mut f = EventFeed::new(vec![], 3600, None);
        f.ingest_line(
            &ev(
                NOW - 10,
                "gaia",
                "gaia.heal.failed",
                "error",
                r#"{"service":"a"}"#,
            ),
            NOW,
        );
        f.ingest_line(
            &ev(
                NOW - 20,
                "gaia",
                "service.probe",
                "info",
                r#"{"service":"a","state":"up"}"#,
            ),
            NOW,
        );
        f.ingest_line(
            &ev(
                NOW - 5,
                "gaia",
                "gaia.heal.failed",
                "error",
                r#"{"service":"a"}"#,
            ),
            NOW,
        );
        let cx = feed_cycle(&mut f, NOW, false);
        assert_eq!(cx.bleeding.len(), 1);
        assert_eq!(cx.bleeding[0].sev, Sev::Error);
        assert!(
            cx.bleeding[0].evidence[0].starts_with("2 event(s)"),
            "{:?}",
            cx.bleeding[0].evidence
        );
    }

    #[test]
    fn window_expiry_and_old_backfill() {
        let mut f = EventFeed::new(vec![], 600, None);
        f.ingest_line(
            &ev(
                NOW - 7200,
                "gaia",
                "gaia.heal.failed",
                "error",
                r#"{"service":"old"}"#,
            ),
            NOW,
        );
        assert_eq!(f.stats.old, 1);
        f.ingest_line(
            &ev(
                NOW,
                "gaia",
                "gaia.heal.refused",
                "warn",
                r#"{"service":"b","stage":"approval","reason":"no approval file given"}"#,
            ),
            NOW,
        );
        assert_eq!(feed_cycle(&mut f, NOW + 600, false).bleeding.len(), 1);
        assert!(feed_cycle(&mut f, NOW + 601, false).bleeding.is_empty());
    }

    #[test]
    fn survival_lines_b33_and_generic() {
        let mut f = EventFeed::new(vec![], 3600, None);
        // Survival guardian shape: no severity, no data.
        let surv = |ok: bool| {
            format!(
                r#"{{"ts":"{}","source":"guardian","kind":"probe","name":"disk","ok":{ok},"detail":"96% used"}}"#,
                ts(NOW)
            )
        };
        f.ingest_line(&surv(false), NOW);
        // A pushed B33 rollup (cortex nerve_events shape) from a phone node.
        f.ingest_line(&ev(NOW, "omnode", "vulnerability", "high",
            r#"{"findings":2,"ids":["a","b"],"top":{"id":"open-adb","component":"phone","title":"adb listens on 10.0.0.5:5555","fix":"","cause":null,"evidence_n":1}}"#), NOW);
        // Unknown kinds: warn ignored, critical capped at error.
        f.ingest_line(&ev(NOW, "hermes-rs", "spend", "warn", "{}"), NOW);
        f.ingest_line(&ev(NOW, "hermes-rs", "spend_cap", "critical", "{}"), NOW);
        // Our own re-emitted findings are not read back (no loop).
        f.ingest_line(
            &ev(NOW, "event:gaia", "bleeding", "crit", r#"{"id":"x"}"#),
            NOW,
        );
        let cx = feed_cycle(&mut f, NOW, false);
        let ids: Vec<&str> = cx.findings().map(|x| x.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "{ids:?}");
        let v = &cx.vulnerabilities[0];
        assert_eq!(
            (v.id.as_str(), v.sev, v.component.as_str()),
            ("ev-omnode-open-adb", Sev::Error, "phone")
        );
        assert!(
            v.title.contains("<addr>") && !v.title.contains("10.0.0.5"),
            "{}",
            v.title
        );
        let gen = cx
            .bleeding
            .iter()
            .find(|b| b.id == "event-errors-hermes-rs-spend_cap")
            .unwrap();
        assert_eq!(gen.sev, Sev::Error);
        assert!(cx.bleeding.iter().any(|b| b.id == "probe-failed-disk"));
        // ok:true and an info B33 event clear.
        f.ingest_line(&surv(true), NOW + 1);
        f.ingest_line(
            &ev(NOW + 1, "omnode", "vulnerability", "info", "{}"),
            NOW + 1,
        );
        let cx = feed_cycle(&mut f, NOW + 1, false);
        assert_eq!(cx.findings().count(), 1);
    }

    #[test]
    fn hostile_input_is_bounded_and_cleaned() {
        let mut f = EventFeed::new(vec![], 3600, Some(vec!["gaia".into()]));
        f.ingest_line("not json", NOW);
        f.ingest_line("[1,2]", NOW);
        f.ingest_line(
            &ev(
                NOW,
                "intruder",
                "gaia.heal.capped",
                "error",
                r#"{"service":"x"}"#,
            ),
            NOW,
        );
        assert_eq!((f.stats.bad, f.stats.skipped), (2, 1));
        for i in 0..(MAX_KEYS + 40) {
            f.ingest_line(
                &ev(
                    NOW - 1000 + i as u64,
                    "gaia",
                    "gaia.heal.failed",
                    "error",
                    &format!(r#"{{"service":"svc {i}/../\u001b[31m"}}"#),
                ),
                NOW,
            );
        }
        let cx = feed_cycle(&mut f, NOW, false);
        assert_eq!(cx.bleeding.len(), MAX_KEYS);
        assert!(cx.bleeding.iter().all(|b| b
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))));
        // Oldest dropped first.
        assert!(!cx.bleeding.iter().any(|b| b.component == "svc_0_.._31m"));
    }

    #[test]
    fn from_env_off_by_default_and_registry() {
        assert!(EventFeed::from_env(&|_| None).is_none());
        assert!(
            EventFeed::from_env(&|k| (k == "NERVE_EVENT_FEED_FILE").then(|| " , ".into()))
                .is_none()
        );
        let env = |k: &str| match k {
            "NERVE_EVENT_FEED_FILE" => Some("/a.jsonl,/b.jsonl".into()),
            "NERVE_EVENT_FEED_WINDOW_S" => Some("10".into()),
            "NERVE_EVENT_FEED_SOURCES" => Some("gaia, guardian".into()),
            _ => None,
        };
        let f = EventFeed::from_env(&env).unwrap();
        assert_eq!((f.tails.len(), f.window_s), (2, 60));
        assert_eq!(
            f.sources.as_deref(),
            Some(&["gaia".to_string(), "guardian".to_string()][..])
        );
        assert!(Sensors::from_env(&env).names().contains(&"event-feed"));
        assert!(!Sensors::from_env(&|_| None).names().contains(&"event-feed"));
        let (cx, _) = run(f, NOW);
        assert!(cx.findings().next().is_none(), "not live: no file read");
    }

    #[test]
    fn tails_file_partial_lines_and_rotation() {
        let dir = std::env::temp_dir().join(format!("event-feed-{}-{NOW}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("events.jsonl");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(rotated_sibling(&p));
        let mut w = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        let mut f = EventFeed::new(vec![p.clone()], 3600, None);
        let line = ev(
            NOW,
            "gaia",
            "gaia.heal.failed",
            "error",
            r#"{"service":"a"}"#,
        );
        // Half a line: nothing yet.
        w.write_all(&line.as_bytes()[..20]).unwrap();
        assert!(feed_cycle(&mut f, NOW, true).bleeding.is_empty());
        w.write_all(&line.as_bytes()[20..]).unwrap();
        w.write_all(b"\n").unwrap();
        assert_eq!(feed_cycle(&mut f, NOW, true).bleeding.len(), 1);
        // Not live: the file is not read (the failed heal stays from state).
        w.write_all(
            ev(
                NOW,
                "gaia",
                "gaia.heal.applied",
                "info",
                r#"{"service":"a"}"#,
            )
            .as_bytes(),
        )
        .unwrap();
        w.write_all(b"\n").unwrap();
        assert_eq!(feed_cycle(&mut f, NOW, false).bleeding.len(), 1);
        // A line appended before rotation is still read from `.1`.
        w.write_all(
            ev(
                NOW + 1,
                "gaia",
                "gaia.heal.failed",
                "error",
                r#"{"service":"b"}"#,
            )
            .as_bytes(),
        )
        .unwrap();
        w.write_all(b"\n").unwrap();
        drop(w);
        std::fs::rename(&p, rotated_sibling(&p)).unwrap();
        let mut w = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap();
        w.write_all(
            ev(
                NOW + 2,
                "gaia",
                "gaia.heal.failed",
                "error",
                r#"{"service":"c"}"#,
            )
            .as_bytes(),
        )
        .unwrap();
        w.write_all(b"\n").unwrap();
        let cx = feed_cycle(&mut f, NOW + 2, true);
        let ids: Vec<&str> = cx.bleeding.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            ["heal-failed-b", "heal-failed-c"],
            "a cleared, b from .1, c new"
        );
        assert_eq!(f.stats.rotations, 1);
        // Truncation restarts at 0.
        drop(w);
        std::fs::write(&p, "").unwrap();
        assert_eq!(feed_cycle(&mut f, NOW + 3, true).bleeding.len(), 2);
        std::fs::write(
            &p,
            format!(
                "{}\n",
                ev(
                    NOW + 3,
                    "gaia",
                    "gaia.heal.applied",
                    "info",
                    r#"{"service":"c"}"#
                )
            ),
        )
        .unwrap();
        let cx = feed_cycle(&mut f, NOW + 3, true);
        assert_eq!(cx.bleeding.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_read_backfills_only_the_tail() {
        let dir = std::env::temp_dir().join(format!("event-feed-bf-{}-{NOW}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("big.jsonl");
        let filler = ev(
            NOW,
            "gaia",
            "gaia.heal.failed",
            "error",
            r#"{"service":"early"}"#,
        );
        let mut s = String::new();
        while (s.len() as u64) < BACKFILL + 4096 {
            s.push_str(&filler);
            s.push('\n');
        }
        s.push_str(&ev(
            NOW,
            "gaia",
            "gaia.heal.applied",
            "info",
            r#"{"service":"early"}"#,
        ));
        s.push('\n');
        std::fs::write(&p, &s).unwrap();
        let mut f = EventFeed::new(vec![p], 3600, None);
        let cx = feed_cycle(&mut f, NOW, true);
        assert!(cx.bleeding.is_empty());
        assert!(f.stats.lines > 0 && f.stats.bad == 0, "{:?}", f.stats);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
