// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! inotify source: task/state files and dirs (swarm slot records, the
//! POSEIDON queue log, `/var/lib/<svc>` state dirs, cert dirs). One blocking
//! read per batch of kernel records; one wake per batch.
//!
//! - Watched DIR: a file closed after write / moved in / created / deleted.
//!   `*.done|*.ok` -> `task.done`, `*.failed|*.fail|*.error` -> `task.failed`,
//!   anything else -> `file.changed` (low).
//! - Watched FILE (append-only log): new lines since the last offset;
//!   `FAILED`/`FAIL:` -> `slot.failed` (high), `DONE` -> `slot.done`,
//!   `START`/`RESUME` -> `slot.started`; other lines are ignored.
//! - Cert files: any change -> the runtime re-checks expiry.
//!
//! The nerve's own data dir must not be watched (it writes there).

use super::sys::{self, Inotify};
use super::{NerveEvent, Severity};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

const DIR_MASK: u32 = sys::IN_CLOSE_WRITE | sys::IN_MOVED_TO | sys::IN_CREATE | sys::IN_DELETE;
const FILE_MASK: u32 = sys::IN_MODIFY
    | sys::IN_CLOSE_WRITE
    | sys::IN_DELETE_SELF
    | sys::IN_MOVED_FROM
    | sys::IN_ATTRIB;

/// Classify a file name seen in a watched dir.
pub fn classify_name(dir: &Path, name: &str, mask: u32) -> NerveEvent {
    let low = name.to_ascii_lowercase();
    let p = dir.join(name).display().to_string();
    let (kind, sev) = if mask & sys::IN_DELETE != 0 {
        ("file.deleted", Severity::Low)
    } else if low.ends_with(".done") || low.ends_with(".ok") || low.ends_with("-done") {
        ("task.done", Severity::Medium)
    } else if low.ends_with(".failed")
        || low.ends_with(".fail")
        || low.ends_with(".error")
        || low.ends_with("-failed")
    {
        ("task.failed", Severity::High)
    } else {
        ("file.changed", Severity::Low)
    };
    NerveEvent::new("inotify", kind, sev, format!("{kind} {p}"))
        .group(group_for_path(&p))
        .key(format!("{kind}:{p}"))
        .data(json!({"path": p, "mask": mask}))
}

fn group_for_path(p: &str) -> &'static str {
    if p.contains("swarm") {
        "swarm"
    } else if p.contains("openmind-queue") {
        "gaia"
    } else if p.contains("hermes") {
        "hermes"
    } else if p.contains("nerve") {
        "nerve"
    } else {
        "other"
    }
}

/// Slot id from a record line ("slot 203 ..." / "slot-15").
fn slot_id(l: &str) -> Option<String> {
    let low = l.to_ascii_lowercase();
    for pat in ["slot ", "slot-"] {
        if let Some(i) = low.find(pat) {
            let d: String = low[i + pat.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if !d.is_empty() {
                return Some(d);
            }
        }
    }
    None
}

/// Classify one appended line of a task/record file.
pub fn classify_line(file: &str, line: &str) -> Option<NerveEvent> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }
    let (kind, sev) = if l.contains("FAILED") || l.contains("FAIL:") || l.contains(" failed:") {
        ("slot.failed", Severity::High)
    } else if l.contains(" DONE") || l.contains("DONE:") || l.contains("DONE ") {
        ("slot.done", Severity::Medium)
    } else if l.contains(" START") || l.contains(" RESUME") {
        ("slot.started", Severity::Info)
    } else {
        return None;
    };
    let id = slot_id(l);
    let msg: String = l.chars().take(240).collect();
    Some(
        NerveEvent::new("inotify", kind, sev, msg)
            .group(group_for_path(file))
            .key(format!(
                "{kind}:{file}:{}",
                id.clone().unwrap_or_else(|| l.chars().take(60).collect())
            ))
            .data(json!({"file": file, "slot": id})),
    )
}

/// What a watch batch produced.
pub enum WatchOut {
    Events(Vec<NerveEvent>),
    CertChanged,
    Error(String),
}

struct FileTail {
    path: PathBuf,
    off: u64,
}

impl FileTail {
    fn new(path: PathBuf) -> Self {
        let off = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        FileTail { path, off }
    }
    fn new_lines(&mut self) -> Vec<String> {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return vec![];
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.off {
            self.off = 0; // truncated / rotated
        }
        if f.seek(SeekFrom::Start(self.off)).is_err() {
            return vec![];
        }
        let mut buf = Vec::new();
        let _ = f.by_ref().take(1 << 20).read_to_end(&mut buf);
        // only complete lines
        let end = buf
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        self.off += end as u64;
        String::from_utf8_lossy(&buf[..end])
            .lines()
            .map(str::to_string)
            .collect()
    }
}

/// Start the inotify thread. `paths`: task/state paths; `certs`: cert files.
pub fn start(
    paths: Vec<PathBuf>,
    certs: Vec<PathBuf>,
    tx: Sender<super::runtime::Input>,
) -> Result<usize, String> {
    if paths.is_empty() && certs.is_empty() {
        return Ok(0);
    }
    let ino = Inotify::new().map_err(|e| format!("inotify_init1: {e}"))?;
    let mut dirs: HashMap<i32, PathBuf> = HashMap::new();
    let mut files: HashMap<i32, FileTail> = HashMap::new();
    let mut cert_wds: HashMap<i32, Vec<String>> = HashMap::new();
    let mut errors = Vec::new();
    for p in &paths {
        let r = if p.is_dir() {
            ino.add_watch(p, DIR_MASK)
                .map(|wd| dirs.insert(wd, p.clone()))
                .map(|_| ())
        } else {
            ino.add_watch(p, FILE_MASK)
                .map(|wd| files.insert(wd, FileTail::new(p.clone())))
                .map(|_| ())
        };
        if let Err(e) = r {
            errors.push(format!("watch {}: {e}", p.display()));
        }
    }
    for c in &certs {
        let parent = c.parent().unwrap_or(Path::new("/")).to_path_buf();
        let name = c
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match ino.add_watch(
            &parent,
            sys::IN_CLOSE_WRITE | sys::IN_MOVED_TO | sys::IN_CREATE,
        ) {
            Ok(wd) => cert_wds.entry(wd).or_default().push(name),
            Err(e) => errors.push(format!("watch cert dir {}: {e}", parent.display())),
        }
    }
    let n = dirs.len() + files.len() + cert_wds.len();
    for e in errors {
        let _ = tx.send(super::runtime::Input::Watch(WatchOut::Error(e)));
    }
    if n == 0 {
        return Err("no watch could be added".into());
    }
    std::thread::Builder::new()
        .name("nerve-evt-inotify".into())
        .spawn(move || loop {
            let recs = match ino.read_blocking() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(super::runtime::Input::Watch(WatchOut::Error(format!(
                        "inotify read: {e}"
                    ))));
                    return;
                }
            };
            let mut evs = Vec::new();
            let mut cert = false;
            let mut tails_touched: Vec<i32> = Vec::new();
            for r in recs {
                if r.mask & sys::IN_Q_OVERFLOW != 0 {
                    evs.push(NerveEvent::new(
                        "inotify",
                        "nerve.watch_overflow",
                        Severity::Medium,
                        "inotify queue overflow",
                    ));
                    continue;
                }
                if r.mask & sys::IN_IGNORED != 0 {
                    continue;
                }
                if let Some(names) = cert_wds.get(&r.wd) {
                    if names.contains(&r.name) {
                        cert = true;
                    }
                    continue;
                }
                if let Some(d) = dirs.get(&r.wd) {
                    if r.mask & sys::IN_CREATE != 0
                        && r.mask & sys::IN_CLOSE_WRITE == 0
                        && !r.name.is_empty()
                    {
                        // creation is followed by CLOSE_WRITE; count the file once
                        continue;
                    }
                    evs.push(classify_name(d, &r.name, r.mask));
                } else if files.contains_key(&r.wd) && !tails_touched.contains(&r.wd) {
                    tails_touched.push(r.wd);
                }
            }
            for wd in tails_touched {
                if let Some(t) = files.get_mut(&wd) {
                    let f = t.path.display().to_string();
                    evs.extend(t.new_lines().iter().filter_map(|l| classify_line(&f, l)));
                }
            }
            if cert
                && tx
                    .send(super::runtime::Input::Watch(WatchOut::CertChanged))
                    .is_err()
            {
                return;
            }
            // Always deliver (even empty) so the wake is counted honestly.
            if tx
                .send(super::runtime::Input::Watch(WatchOut::Events(evs)))
                .is_err()
            {
                return;
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_lines() {
        let f = "/workspace/swarm/CALL-RECORD.md";
        let d = classify_line(f, "- 2026-10-21 17:35:00 +0200 — slot 203 docs-cutover-missing DONE (LOCAL only: push-branch.sh blocked").unwrap();
        assert_eq!(
            (d.kind.as_str(), d.severity, d.group.as_str()),
            ("slot.done", Severity::Medium, "swarm")
        );
        assert_eq!(d.data["slot"], "203");
        let s = classify_line(
            f,
            "- 2026-10-21 17:53:46 +0200 — slot 199 gaia-fault-followups RESUME (retry 1)",
        )
        .unwrap();
        assert_eq!(s.kind, "slot.started");
        let x = classify_line(f, "- slot-15 build FAILED: clippy").unwrap();
        assert_eq!(
            (x.kind.as_str(), x.severity),
            ("slot.failed", Severity::High)
        );
        assert!(classify_line(f, "- 2026-10-21 routine run: COUNT running 3").is_none());
        assert!(classify_line(f, "").is_none());
    }

    #[test]
    fn dir_names() {
        let d = Path::new("/var/lib/openmind-queue");
        assert_eq!(
            classify_name(d, "job-7.done", sys::IN_CLOSE_WRITE).kind,
            "task.done"
        );
        assert_eq!(
            classify_name(d, "job-7.failed", sys::IN_MOVED_TO).severity,
            Severity::High
        );
        assert_eq!(
            classify_name(d, "queue.log", sys::IN_CLOSE_WRITE).kind,
            "file.changed"
        );
        assert_eq!(classify_name(d, "x", sys::IN_DELETE).kind, "file.deleted");
        assert_eq!(classify_name(d, "x", sys::IN_DELETE).group, "gaia");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_inotify_dir_and_file() {
        let base = std::env::temp_dir().join(format!("nerve-evt-watch-{}", std::process::id()));
        let dir = base.join("swarm-tasks");
        std::fs::create_dir_all(&dir).unwrap();
        let rec = base.join("swarm-record.md");
        std::fs::write(&rec, "old line DONE\n").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            start(vec![dir.clone(), rec.clone()], vec![], tx).unwrap(),
            2
        );
        std::fs::write(dir.join("slot-9.done"), "ok").unwrap();
        let mut got = Vec::new();
        while got.is_empty() {
            if let super::super::runtime::Input::Watch(WatchOut::Events(v)) =
                rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
            {
                got = v;
            }
        }
        assert_eq!(got[0].kind, "task.done");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&rec).unwrap();
        writeln!(f, "- slot 77 thing FAILED: tests").unwrap();
        drop(f);
        let mut kinds = Vec::new();
        while !kinds.contains(&"slot.failed".to_string()) {
            if let super::super::runtime::Input::Watch(WatchOut::Events(v)) =
                rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
            {
                kinds.extend(v.into_iter().map(|e| e.kind));
            }
        }
        // the pre-existing "old line DONE" was not re-read
        assert!(!kinds.contains(&"slot.done".to_string()));
        let _ = std::fs::remove_dir_all(&base);
    }
}
