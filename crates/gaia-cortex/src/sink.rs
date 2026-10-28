// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Append-only JSONL logs: decisions, alerts, the escalate-to-human queue,
//! and the optional alert outbox. In-memory rings back the API.

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

const RING: usize = 200;

pub struct Jsonl {
    pub path: Option<PathBuf>,
    pub ring: VecDeque<Value>,
    pub total: u64,
}

impl Jsonl {
    pub fn new(path: Option<PathBuf>) -> Self {
        Jsonl {
            path,
            ring: VecDeque::new(),
            total: 0,
        }
    }

    pub fn push(&mut self, v: Value) {
        if let Some(p) = &self.path {
            let _ = append(p, &v);
        }
        self.total += 1;
        self.ring.push_back(v);
        while self.ring.len() > RING {
            self.ring.pop_front();
        }
    }

    pub fn recent(&self, n: usize) -> Value {
        Value::Array(self.ring.iter().rev().take(n).cloned().collect())
    }
}

pub fn append(p: &Path, v: &Value) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)?;
    writeln!(f, "{v}")
}

/// Escalate-to-human queue: `escalations.jsonl` + `GET /escalations`,
/// acknowledged with `POST /escalations/ack?id=`.
///
/// Wave-4 slot 417: before this, `ack` was the ONLY way an item left
/// `open` (besides the 200-item ring cap and a process restart, which
/// silently drops the whole in-memory queue). The live cortex never got an
/// ack, so `escalations_open` only grew (1 at 05:47 -> 11 at 15:37
/// +01:00 on 2026-10-28). [`Escalations::close_incident`] and
/// [`Escalations::expire`] close items when their incident resolves or when
/// they are older than a configured age; both are driven by the engine and
/// OFF by default (`escalation_autoclose`, `escalation_max_age_s`). A closed
/// item moves to the `closed` ring with `closed_why` / `closed_ts` and one
/// `{"close": id, ...}` line is appended to the log, the same way an ack is.
pub struct Escalations {
    pub log: Jsonl,
    pub open: VecDeque<Value>,
    /// Recently closed items (newest last, ring of [`RING`]).
    pub closed: VecDeque<Value>,
    /// Items closed by incident resolution or age since start.
    pub closed_total: u64,
    /// Escalation id -> unix seconds it was opened (for [`Escalations::expire`]).
    opened_at: HashMap<String, u64>,
}

impl Escalations {
    pub fn new(path: Option<PathBuf>) -> Self {
        Escalations {
            log: Jsonl::new(path),
            open: VecDeque::new(),
            closed: VecDeque::new(),
            closed_total: 0,
            opened_at: HashMap::new(),
        }
    }

    /// Open an item with no known open time (never expires by age).
    pub fn push(&mut self, v: Value) {
        self.log.push(v.clone());
        self.open.push_back(v);
        self.trim_open();
    }

    /// Open an item at unix second `now` (expires by age when enabled).
    pub fn push_at(&mut self, v: Value, now: u64) {
        if let Some(id) = v["id"].as_str() {
            self.opened_at.insert(id.to_string(), now);
        }
        self.push(v);
    }

    fn trim_open(&mut self) {
        while self.open.len() > RING {
            if let Some(old) = self.open.pop_front() {
                if let Some(id) = old["id"]
                    .as_str()
                    .filter(|id| !self.open.iter().any(|v| v["id"] == *id))
                {
                    self.opened_at.remove(id);
                }
            }
        }
    }

    pub fn ack(&mut self, id: &str, now_ts: &str) -> bool {
        let before = self.open.len();
        self.open.retain(|v| v["id"] != id);
        let hit = self.open.len() != before;
        if hit {
            self.opened_at.remove(id);
            if let Some(p) = &self.log.path {
                let _ = append(p, &json!({"ack": id, "ts": now_ts}));
            }
        }
        hit
    }

    /// Close every open item whose `incident` is `incident` (its incident
    /// resolved). Returns how many closed.
    pub fn close_incident(&mut self, incident: &str, why: &str, now_ts: &str) -> usize {
        self.close_where(|v, _| v["incident"] == incident, why, now_ts)
    }

    /// Close every open item opened at least `max_age_s` seconds before
    /// `now`. Items without a known open time are kept. 0 = never.
    pub fn expire(&mut self, now: u64, max_age_s: u64, now_ts: &str) -> usize {
        if max_age_s == 0 {
            return 0;
        }
        self.close_where(
            |_, at| at.is_some_and(|t| now.saturating_sub(t) >= max_age_s),
            "expired",
            now_ts,
        )
    }

    fn close_where(
        &mut self,
        pick: impl Fn(&Value, Option<u64>) -> bool,
        why: &str,
        now_ts: &str,
    ) -> usize {
        let mut keep = VecDeque::with_capacity(self.open.len());
        let mut n = 0;
        for mut v in std::mem::take(&mut self.open) {
            let id = v["id"].as_str().unwrap_or("").to_string();
            if !pick(&v, self.opened_at.get(&id).copied()) {
                keep.push_back(v);
                continue;
            }
            n += 1;
            if let Some(p) = &self.log.path {
                let _ = append(
                    p,
                    &json!({"close": id, "incident": v["incident"], "why": why, "ts": now_ts}),
                );
            }
            v["closed_why"] = json!(why);
            v["closed_ts"] = json!(now_ts);
            self.closed.push_back(v);
            while self.closed.len() > RING {
                self.closed.pop_front();
            }
        }
        self.open = keep;
        // Forget open times only for ids no longer open: a legacy log can
        // hold one `esc-<N>` per process (slot 431 reload), and those share
        // the newest open time (never expire early).
        let still: std::collections::HashSet<&str> =
            self.open.iter().filter_map(|v| v["id"].as_str()).collect();
        self.opened_at.retain(|k, _| still.contains(k.as_str()));
        self.closed_total += n as u64;
        n
    }
}

/// An escalation id, either shape (wave-4 slot 431, D-417-3).
///
/// - `esc-<N>`: up to slot 417. `N` restarts at 1 in every process, so the
///   same id appears once per process in `escalations.jsonl` (`esc-3` x4 in
///   the live file of 2026-10-28).
/// - `esc-<start-unix>-<N>`: from slot 431. `<start-unix>` is the process
///   epoch (unix second the cortex started, bumped past every epoch already
///   in the log), so ids are unique across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscId {
    Legacy(u64),
    Unique { start: u64, n: u64 },
}

/// Parse an escalation id of either shape; `None` for anything else.
pub fn parse_esc_id(id: &str) -> Option<EscId> {
    let rest = id.strip_prefix("esc-")?;
    let num = |s: &str| -> Option<u64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    match rest.split_once('-') {
        None => num(rest).map(EscId::Legacy),
        Some((a, b)) => Some(EscId::Unique {
            start: num(a)?,
            n: num(b)?,
        }),
    }
}

/// What [`Escalations::load`] found in `escalations.jsonl`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reload {
    /// Non-empty lines read.
    pub lines: usize,
    /// Escalation records (lines with an `id`).
    pub opened: usize,
    /// `{"ack":id}` / `{"close":id}` lines that matched an open item.
    pub acked: usize,
    pub closed: usize,
    /// ack/close lines whose id was not open (already gone / never seen).
    pub unmatched: usize,
    /// Lines that are not JSON objects (a torn last line after a crash).
    pub skipped: usize,
    /// The file did not end in a newline; one was appended so the next
    /// record starts on its own line.
    pub repaired_newline: bool,
    /// Open items after the replay (after the ring cap).
    pub open: usize,
    /// Largest `<start-unix>` of a new-shape id in the log.
    pub max_epoch: Option<u64>,
}

impl Escalations {
    /// Open the queue and rebuild `open` (and the `closed` ring) from
    /// `path`: every escalation record minus the ones a later
    /// `{"ack":id}` / `{"close":id}` line removed (wave-4 slot 431, F-417-2).
    /// An ack/close removes the most recent open item with that id, so a
    /// legacy log with one `esc-3` per process still replays correctly.
    /// A missing file is an empty queue. Unparsable lines are skipped; a
    /// missing final newline is repaired so appends stay line-aligned.
    /// `log.total` and `closed_total` stay counters since this start.
    pub fn load(path: Option<PathBuf>) -> (Self, Reload) {
        let mut q = Escalations::new(path);
        let mut r = Reload::default();
        let Some(p) = q.log.path.clone() else {
            return (q, r);
        };
        let Ok(bytes) = std::fs::read(&p) else {
            return (q, r);
        };
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            r.lines += 1;
            let Ok(Value::Object(m)) = serde_json::from_str::<Value>(line) else {
                r.skipped += 1;
                continue;
            };
            let v = Value::Object(m);
            if let Some(id) = v["ack"].as_str() {
                match q.take_latest(id) {
                    Some(_) => r.acked += 1,
                    None => r.unmatched += 1,
                }
            } else if let Some(id) = v["close"].as_str() {
                match q.take_latest(id) {
                    Some(mut item) => {
                        r.closed += 1;
                        item["closed_why"] = v["why"].clone();
                        item["closed_ts"] = v["ts"].clone();
                        q.closed.push_back(item);
                        while q.closed.len() > RING {
                            q.closed.pop_front();
                        }
                    }
                    None => r.unmatched += 1,
                }
            } else if let Some(id) = v["id"].as_str().map(str::to_string) {
                r.opened += 1;
                if let Some(EscId::Unique { start, .. }) = parse_esc_id(&id) {
                    r.max_epoch = Some(r.max_epoch.map_or(start, |m| m.max(start)));
                }
                // Open time from the record's own Zagreb stamp, so
                // `escalation_max_age_s` keeps counting across restarts.
                if let Some(t) = v["ts"].as_str().and_then(zagreb_time::to_unix) {
                    if let Ok(t) = u64::try_from(t) {
                        q.opened_at.insert(id, t);
                    }
                }
                q.open.push_back(v);
                q.trim_open();
            } else {
                r.skipped += 1;
            }
        }
        if bytes.last().is_some_and(|b| *b != b'\n') {
            if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&p) {
                r.repaired_newline = f.write_all(b"\n").is_ok();
            }
        }
        r.open = q.open.len();
        (q, r)
    }

    /// Remove and return the most recently opened item with this id.
    fn take_latest(&mut self, id: &str) -> Option<Value> {
        let i = self.open.iter().rposition(|v| v["id"] == id)?;
        let item = self.open.remove(i)?;
        if !self.open.iter().any(|v| v["id"] == id) {
            self.opened_at.remove(id);
        }
        Some(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(id: &str, inc: &str) -> Value {
        json!({"id": id, "incident": inc, "subject": "unit:x.service"})
    }

    fn tmp(tag: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("gaia-cortex-417-431-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn close_incident_moves_only_matching_items_and_logs_once() {
        let p = tmp("inc");
        let mut q = Escalations::new(Some(p.clone()));
        q.push_at(esc("esc-1", "inc-a"), 100);
        q.push_at(esc("esc-2", "inc-b"), 100);
        q.push_at(esc("esc-3", "inc-a"), 110);
        assert_eq!(q.close_incident("inc-a", "incident-cleared", "T"), 2);
        assert_eq!(q.open.len(), 1);
        assert_eq!(q.open[0]["id"], "esc-2");
        assert_eq!(q.closed.len(), 2);
        assert_eq!(q.closed[0]["closed_why"], "incident-cleared");
        assert_eq!(q.closed_total, 2);
        assert_eq!(q.close_incident("inc-a", "incident-cleared", "T"), 0);
        let log = std::fs::read_to_string(&p).unwrap();
        assert_eq!(log.lines().filter(|l| l.contains("\"close\"")).count(), 2);
        assert_eq!(log.lines().count(), 5);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn expire_uses_open_time_and_zero_is_off() {
        let mut q = Escalations::new(None);
        q.push_at(esc("esc-1", "inc-a"), 1_000);
        q.push_at(esc("esc-2", "inc-b"), 5_000);
        q.push(esc("esc-3", "inc-c")); // no open time: never expires by age
        assert_eq!(q.expire(1_000_000, 0, "T"), 0);
        assert_eq!(q.expire(4_999, 4_000, "T"), 0);
        assert_eq!(q.expire(5_000, 4_000, "T"), 1);
        assert_eq!(q.open.len(), 2);
        assert_eq!(q.expire(9_000, 4_000, "T"), 1);
        assert_eq!(q.open.len(), 1);
        assert_eq!(q.open[0]["id"], "esc-3");
        assert_eq!(q.closed[1]["closed_why"], "expired");
    }

    #[test]
    fn ack_still_works_and_forgets_open_time() {
        let mut q = Escalations::new(None);
        q.push_at(esc("esc-1", "inc-a"), 10);
        assert!(q.ack("esc-1", "T"));
        assert!(!q.ack("esc-1", "T"));
        assert!(q.opened_at.is_empty());
        assert_eq!(q.expire(1_000, 1, "T"), 0);
        assert_eq!(q.closed_total, 0);
    }

    // ---- slot 431: reload across restarts + process-unique ids ----

    fn esc_at(id: &str, inc: &str, ts: &str) -> Value {
        json!({"id": id, "incident": inc, "subject": "unit:x.service", "ts": ts})
    }

    #[test]
    fn parse_esc_id_reads_both_shapes() {
        assert_eq!(parse_esc_id("esc-42"), Some(EscId::Legacy(42)));
        assert_eq!(
            parse_esc_id("esc-1793224800-7"),
            Some(EscId::Unique {
                start: 1_793_224_800,
                n: 7
            })
        );
        for bad in [
            "esc-",
            "esc-x",
            "esc-1-",
            "esc--1",
            "esc-1-2-3",
            "al-3",
            "dec-1",
            "esc-+1",
            "",
        ] {
            assert_eq!(parse_esc_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn restart_keeps_open_items_and_their_open_time() {
        let p = tmp("restart");
        {
            let mut q = Escalations::new(Some(p.clone()));
            q.push_at(esc_at("esc-100-1", "inc-a", "2026-10-28T06:25:10+01:00"), 1);
            q.push_at(esc_at("esc-100-2", "inc-b", "2026-10-28T06:25:40+01:00"), 2);
        } // process "dies"
        let (q, r) = Escalations::load(Some(p.clone()));
        assert_eq!(q.open.len(), 2);
        assert_eq!(q.open[0]["id"], "esc-100-1");
        assert_eq!(r.opened, 2);
        assert_eq!(r.open, 2);
        assert_eq!(r.max_epoch, Some(100));
        assert!(!r.repaired_newline);
        // Open time comes from the record's ts, not from the restart.
        let t = zagreb_time::to_unix("2026-10-28T06:25:10+01:00").unwrap() as u64;
        assert_eq!(q.opened_at.get("esc-100-1"), Some(&t));
        assert_eq!(q.log.total, 0, "total counts pushes since this start");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn closed_and_acked_items_stay_closed_after_restart() {
        let p = tmp("closed");
        {
            let mut q = Escalations::new(Some(p.clone()));
            q.push_at(esc_at("esc-100-1", "inc-a", "2026-10-28T06:00:00+01:00"), 1);
            q.push_at(esc_at("esc-100-2", "inc-b", "2026-10-28T06:00:01+01:00"), 2);
            q.push_at(esc_at("esc-100-3", "inc-c", "2026-10-28T06:00:02+01:00"), 3);
            assert!(q.ack("esc-100-1", "2026-10-28T07:00:00+01:00"));
            assert_eq!(
                q.close_incident("inc-b", "incident-cleared", "2026-10-28T07:00:01+01:00"),
                1
            );
        }
        let (mut q, r) = Escalations::load(Some(p.clone()));
        assert_eq!((r.opened, r.acked, r.closed, r.unmatched), (3, 1, 1, 0));
        assert_eq!(q.open.len(), 1);
        assert_eq!(q.open[0]["id"], "esc-100-3");
        assert_eq!(q.closed.len(), 1);
        assert_eq!(q.closed[0]["id"], "esc-100-2");
        assert_eq!(q.closed[0]["closed_why"], "incident-cleared");
        assert_eq!(q.closed_total, 0, "closed_total counts since this start");
        assert!(!q.opened_at.contains_key("esc-100-1"));
        // A second restart changes nothing (load itself writes nothing).
        let before = std::fs::read_to_string(&p).unwrap();
        q.ack("esc-100-3", "2026-10-28T08:00:00+01:00");
        drop(q);
        let (q, _) = Escalations::load(Some(p.clone()));
        assert!(q.open.is_empty());
        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after.lines().count(), before.lines().count() + 1);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn torn_last_line_is_skipped_and_the_file_is_realigned() {
        let p = tmp("torn");
        let good = esc_at("esc-100-1", "inc-a", "2026-10-28T06:00:00+01:00");
        let torn = r#"{"id":"esc-100-2","incident":"inc-b","subj"#;
        std::fs::write(&p, format!("{good}\n{torn}")).unwrap();
        let (mut q, r) = Escalations::load(Some(p.clone()));
        assert_eq!((r.lines, r.opened, r.skipped), (2, 1, 1));
        assert!(r.repaired_newline);
        assert_eq!(q.open.len(), 1);
        // The next record lands on its own line, so the restart after
        // that still reads it.
        q.push_at(esc_at("esc-200-1", "inc-c", "2026-10-28T07:00:00+01:00"), 5);
        drop(q);
        let (q, r) = Escalations::load(Some(p.clone()));
        assert_eq!((r.opened, r.skipped, r.repaired_newline), (2, 1, false));
        let ids: Vec<&str> = q.open.iter().map(|v| v["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["esc-100-1", "esc-200-1"]);
        assert_eq!(r.max_epoch, Some(200));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn legacy_duplicate_ids_ack_only_the_latest_and_missing_file_is_empty() {
        let p = tmp("legacy");
        let lines = [
            esc_at("esc-3", "inc-1", "2026-10-21T19:06:00+02:00").to_string(),
            esc_at("esc-3", "inc-2", "2026-10-22T10:00:00+02:00").to_string(),
            json!({"ack": "esc-3", "ts": "2026-10-22T10:05:00+02:00"}).to_string(),
            json!({"ack": "esc-99", "ts": "2026-10-22T10:05:00+02:00"}).to_string(),
            "not json".into(),
            "[1,2]".into(),
            "{\"neither\":1}".into(),
            String::new(),
        ];
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        let (q, r) = Escalations::load(Some(p.clone()));
        assert_eq!(q.open.len(), 1);
        assert_eq!(q.open[0]["incident"], "inc-1", "ack hit the newest esc-3");
        assert_eq!(
            (r.acked, r.unmatched, r.skipped, r.max_epoch),
            (1, 1, 3, None)
        );
        let _ = std::fs::remove_file(&p);
        let (q, r) = Escalations::load(Some(p.clone()));
        assert!(q.open.is_empty());
        assert_eq!(r, Reload::default());
        assert!(!p.exists(), "load never creates the file");
        let (q, _) = Escalations::load(None);
        assert!(q.open.is_empty());
    }

    #[test]
    fn reload_respects_the_ring_cap() {
        let p = tmp("cap");
        let body: String = (0..RING + 25)
            .map(|i| {
                format!(
                    "{}\n",
                    esc_at(&format!("esc-100-{i}"), "inc", "2026-10-28T06:00:00+01:00")
                )
            })
            .collect();
        std::fs::write(&p, body).unwrap();
        let (q, r) = Escalations::load(Some(p.clone()));
        assert_eq!(q.open.len(), RING);
        assert_eq!(r.open, RING);
        assert_eq!(q.open[0]["id"], "esc-100-25");
        assert_eq!(q.opened_at.len(), RING);
        let _ = std::fs::remove_file(&p);
    }
}
