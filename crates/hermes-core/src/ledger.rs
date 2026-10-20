// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The durable ledger and audit log: one JSON object per line.
//!
//! * Every line carries `ts`, Europe/Zagreb RFC 3339 with an explicit offset
//!   (`+02:00` CEST, `+01:00` CET from 2026-10-25 03:00), never `Z`, plus the
//!   Unix second it was taken from.
//! * Every line carries `prev`, the SHA-256 of the previous line's bytes
//!   (`genesis` for the first), so an edited or deleted line breaks the
//!   chain and [`Ledger::open`] reports where.
//! * Appends are `O_APPEND` writes followed by `fdatasync`; a line is either
//!   on disk whole or not at all. A torn last line (crash mid-write) is
//!   reported and skipped; the next append starts on a fresh line and chains
//!   to the last whole line.
//! * One writer at a time: the file is locked exclusively while open for
//!   writing. Readers ([`read_all`]) take no lock.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const GENESIS: &str = "genesis";

/// One ledger line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub ts: String,
    pub unix: i64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    pub prev: String,
}

/// What a caller fills in; the ledger adds seq, time and chain.
#[derive(Clone, Debug, Default)]
pub struct NewEvent {
    pub kind: String,
    pub task: Option<String>,
    pub sector: Option<String>,
    pub agent: Option<String>,
    pub session: Option<String>,
    pub attempt: Option<u32>,
    pub slots: Option<u32>,
    pub state: Option<String>,
    pub detail: String,
}

impl NewEvent {
    pub fn new(kind: &str) -> NewEvent {
        NewEvent {
            kind: kind.into(),
            ..NewEvent::default()
        }
    }
    pub fn task(mut self, key: &str, sector: &str) -> Self {
        self.task = Some(key.into());
        self.sector = Some(sector.into());
        self
    }
    pub fn agent(mut self, a: &str) -> Self {
        self.agent = Some(a.into());
        self
    }
    pub fn session(mut self, s: &str) -> Self {
        self.session = Some(s.into());
        self
    }
    pub fn attempt(mut self, n: u32) -> Self {
        self.attempt = Some(n);
        self
    }
    pub fn slots(mut self, n: u32) -> Self {
        self.slots = Some(n);
        self
    }
    pub fn state(mut self, s: &str) -> Self {
        self.state = Some(s.into());
        self
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = d.into();
        self
    }
}

/// Result of reading a ledger file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replay {
    pub events: Vec<Event>,
    /// A last line without a newline or not valid JSON (crash mid-write).
    pub torn_tail: bool,
    /// 1-based line numbers where `prev` does not match the line before.
    pub chain_breaks: Vec<usize>,
    /// 1-based line numbers with a timestamp that is not Zagreb RFC 3339.
    pub bad_stamps: Vec<usize>,
    /// Hash of the last whole line (what the next `prev` must be).
    pub head: String,
    /// Byte length up to and including the last whole line.
    pub valid_len: u64,
}

impl Replay {
    pub fn sound(&self) -> bool {
        self.chain_breaks.is_empty() && self.bad_stamps.is_empty()
    }
}

pub fn line_hash(line: &[u8]) -> String {
    let d = Sha256::digest(line);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Read and verify a ledger without locking it. A missing file is empty.
///
/// # Errors
/// I/O errors other than "not found", or a broken line in the middle.
pub fn read_all(path: &Path) -> Result<Replay, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Replay {
                head: GENESIS.into(),
                ..Replay::default()
            })
        }
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let mut rep = Replay {
        head: GENESIS.into(),
        ..Replay::default()
    };
    let ends_clean = bytes.last() == Some(&b'\n');
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    let n = parts.len();
    let mut offset: u64 = 0;
    for (i, raw) in parts.iter().enumerate() {
        let last_piece = i + 1 == n;
        let line_end = offset + raw.len() as u64 + u64::from(!last_piece);
        offset = line_end;
        if raw.is_empty() {
            if !last_piece {
                rep.valid_len = line_end;
            }
            continue;
        }
        let parsed: Result<Event, _> = serde_json::from_slice(raw);
        match parsed {
            Ok(ev) => {
                if last_piece && !ends_clean {
                    // Whole JSON but no newline: the write did not finish.
                    rep.torn_tail = true;
                    continue;
                }
                if ev.prev != rep.head {
                    rep.chain_breaks.push(i + 1);
                }
                if zagreb_time::require_rfc3339(&ev.ts).is_err() {
                    rep.bad_stamps.push(i + 1);
                }
                rep.head = line_hash(raw);
                rep.events.push(ev);
                rep.valid_len = line_end;
            }
            Err(_) if last_piece || (i + 2 == n && ends_clean) => {
                rep.torn_tail = true;
            }
            Err(e) => {
                return Err(format!(
                    "{}: line {} is not a ledger event: {e}",
                    path.display(),
                    i + 1
                ));
            }
        }
    }
    Ok(rep)
}

/// An open, locked ledger.
pub struct Ledger {
    path: PathBuf,
    file: File,
    head: String,
    next_seq: u64,
    needs_newline: bool,
}

impl std::fmt::Debug for Ledger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ledger")
            .field("path", &self.path)
            .field("next_seq", &self.next_seq)
            .finish()
    }
}

impl Ledger {
    /// Open for appending (creating parent dirs), lock, and replay.
    ///
    /// # Errors
    /// I/O, another writer holds the lock, or the chain is broken (a broken
    /// chain is refused for writing: Hermes does not extend a tampered log).
    pub fn open(path: &Path) -> Result<(Ledger, Replay), String> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("create {}: {e}", dir.display()))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        file.try_lock()
            .map_err(|_| format!("{} is locked by another Hermes writer", path.display()))?;
        let rep = read_all(path)?;
        if !rep.chain_breaks.is_empty() {
            return Err(format!(
                "{}: hash chain broken at line(s) {:?}; refusing to append",
                path.display(),
                rep.chain_breaks
            ));
        }
        if rep.torn_tail {
            // The torn record was never acknowledged (append returns only
            // after fdatasync of the whole line). Cut it off so the file is
            // whole lines only again.
            file.set_len(rep.valid_len)
                .and_then(|()| file.sync_data())
                .map_err(|e| format!("truncate torn tail of {}: {e}", path.display()))?;
        }
        let needs_newline = rep.valid_len > 0 && !ends_with_newline(path);
        let next_seq = rep.events.last().map_or(1, |e| e.seq + 1);
        Ok((
            Ledger {
                path: path.to_path_buf(),
                file,
                head: rep.head.clone(),
                next_seq,
                needs_newline,
            },
            rep,
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one event, durably.
    ///
    /// # Errors
    /// Serialisation or I/O failure; on failure nothing is acknowledged.
    pub fn append(&mut self, ne: NewEvent) -> Result<Event, String> {
        let unix = now_unix();
        let ev = Event {
            seq: self.next_seq,
            ts: zagreb_time::from_unix(unix),
            unix,
            kind: ne.kind,
            task: ne.task,
            sector: ne.sector,
            agent: ne.agent,
            session: ne.session,
            attempt: ne.attempt,
            slots: ne.slots,
            state: ne.state,
            detail: ne.detail,
            prev: self.head.clone(),
        };
        let line = serde_json::to_vec(&ev).map_err(|e| format!("encode event: {e}"))?;
        let mut buf = Vec::with_capacity(line.len() + 2);
        if self.needs_newline {
            buf.push(b'\n');
        }
        buf.extend_from_slice(&line);
        buf.push(b'\n');
        self.file
            .write_all(&buf)
            .and_then(|()| self.file.sync_data())
            .map_err(|e| format!("append {}: {e}", self.path.display()))?;
        self.needs_newline = false;
        self.head = line_hash(&line);
        self.next_seq += 1;
        Ok(ev)
    }
}

fn ends_with_newline(path: &Path) -> bool {
    std::fs::read(path)
        .map(|b| b.last() == Some(&b'\n'))
        .unwrap_or(true)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "hermes-core-ledger-{name}-{}-{}",
            std::process::id(),
            now_unix()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("ledger.jsonl")
    }

    #[test]
    fn append_reopen_replay_and_chain() {
        let p = tmp("chain");
        {
            let (mut l, rep) = Ledger::open(&p).unwrap();
            assert!(rep.events.is_empty());
            l.append(NewEvent::new("queued").task("a:S1.1", "a"))
                .unwrap();
            l.append(NewEvent::new("dispatched").task("a:S1.1", "a").attempt(1))
                .unwrap();
        }
        let (mut l, rep) = Ledger::open(&p).unwrap();
        assert_eq!(rep.events.len(), 2);
        assert!(rep.sound() && !rep.torn_tail);
        assert_eq!(rep.events[0].prev, GENESIS);
        let ev = l.append(NewEvent::new("completed")).unwrap();
        assert_eq!(ev.seq, 3);
        assert!(
            ev.ts.ends_with("+02:00") || ev.ts.ends_with("+01:00"),
            "{}",
            ev.ts
        );
    }

    #[test]
    fn second_writer_is_refused() {
        let p = tmp("lock");
        let (_l, _) = Ledger::open(&p).unwrap();
        let e = Ledger::open(&p).unwrap_err();
        assert!(e.contains("locked"), "{e}");
    }

    #[test]
    fn torn_tail_is_skipped_and_next_append_chains_to_last_whole_line() {
        let p = tmp("torn");
        {
            let (mut l, _) = Ledger::open(&p).unwrap();
            l.append(NewEvent::new("queued")).unwrap();
        }
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"{\"seq\":2,\"ts\":\"2026-10-").unwrap();
        drop(f);
        {
            let (mut l, rep) = Ledger::open(&p).unwrap();
            assert!(rep.torn_tail);
            assert_eq!(rep.events.len(), 1);
            l.append(NewEvent::new("queued")).unwrap();
        }
        let rep = read_all(&p).unwrap();
        assert!(rep.sound(), "{rep:?}");
        assert_eq!(rep.events.len(), 2);
    }

    #[test]
    fn edited_line_breaks_the_chain_and_blocks_writing() {
        let p = tmp("tamper");
        {
            let (mut l, _) = Ledger::open(&p).unwrap();
            for k in ["a", "b", "c"] {
                l.append(NewEvent::new("queued").detail(k)).unwrap();
            }
        }
        let text = std::fs::read_to_string(&p).unwrap();
        std::fs::write(&p, text.replacen("\"detail\":\"b\"", "\"detail\":\"B\"", 1)).unwrap();
        let rep = read_all(&p).unwrap();
        assert_eq!(rep.chain_breaks, [3]);
        assert!(Ledger::open(&p).unwrap_err().contains("chain broken"));
    }
}
