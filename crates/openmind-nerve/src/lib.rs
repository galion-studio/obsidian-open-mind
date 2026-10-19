// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

//! Append-only JSONL writer for nerve events.
//!
//! The caller supplies the path. Each [`Writer::append`] adds one JSON object
//! and a newline. The `ts` field is always the current Europe/Zagreb RFC 3339
//! timestamp from [`zagreb_time`] (explicit numeric offset, never `Z`).
//!
//! When the active file is already at least `max_bytes` long, the next append
//! rotates it first. Bytes already written are never rewritten in place:
//! rotation renames or unlinks the whole file. A single line longer than
//! `max_bytes` is still written; the following append rotates.
//!
//! One writer owns a path. This crate does not open sockets.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Largest `keep` accepted by [`Writer::open`].
///
/// Rotation walks one directory entry per retained file. The cap keeps that
/// walk small.
pub const MAX_KEEP: usize = 1024;

/// One nerve record.
///
/// `ts` is not an input. [`Writer::append`] stamps it from `zagreb-time`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    ts: String,
    source: String,
    kind: String,
    severity: String,
    data: Value,
}

impl Event {
    /// Build a record. `data_json` must be one JSON value (object, array,
    /// string, number, bool, or null).
    pub fn new(
        source: impl Into<String>,
        kind: impl Into<String>,
        severity: impl Into<String>,
        data_json: &str,
    ) -> io::Result<Self> {
        let data = serde_json::from_str(data_json)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        Ok(Self {
            ts: String::new(),
            source: source.into(),
            kind: kind.into(),
            severity: severity.into(),
            data,
        })
    }

    /// Europe/Zagreb RFC 3339 stamp written on this line. Empty until the
    /// record has been read back from a file.
    pub fn ts(&self) -> &str {
        &self.ts
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn severity(&self) -> &str {
        &self.severity
    }

    /// Compact JSON text of the `data` value.
    pub fn data_json(&self) -> String {
        self.data.to_string()
    }
}

/// Append-only JSONL file at a caller-supplied path.
#[derive(Debug)]
pub struct Writer {
    path: PathBuf,
    max_bytes: u64,
    keep: usize,
    file: File,
}

impl Writer {
    /// Open `path` for append.
    ///
    /// Creates the file when it is missing. Does not truncate it and does not
    /// create missing parent directories. Opening a file that is already over
    /// the size limit does not rotate; the next [`Writer::append`] does.
    ///
    /// `max_bytes == 0` disables rotation. Otherwise an append rotates first
    /// when the active file's length is greater than or equal to `max_bytes`.
    ///
    /// `keep` is how many rotated siblings to retain: `<path>.1` is the newest
    /// and `<path>.<keep>` is the oldest. `keep == 0` unlinks the active file
    /// on rotation instead of renaming it. Values above [`MAX_KEEP`] are
    /// rejected.
    pub fn open(path: impl AsRef<Path>, max_bytes: u64, keep: usize) -> io::Result<Self> {
        if keep > MAX_KEEP {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("keep {keep} exceeds {MAX_KEEP}"),
            ));
        }
        let path = path.as_ref().to_path_buf();
        let file = open_append(&path)?;
        Ok(Self {
            path,
            max_bytes,
            keep,
            file,
        })
    }

    /// Path supplied by the caller.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn keep(&self) -> usize {
        self.keep
    }

    /// Append one JSON line. `event.ts` is ignored; the line is stamped with
    /// [`zagreb_time::now_rfc3339`].
    pub fn append(&mut self, event: &Event) -> io::Result<()> {
        let ts = zagreb_time::now_rfc3339();
        let bytes = encode(&ts, event)?;
        self.rotate_if_full()?;
        self.file.write_all(&bytes)?;
        self.file.flush()?;
        Ok(())
    }

    fn rotate_if_full(&mut self) -> io::Result<()> {
        if self.max_bytes == 0 {
            return Ok(());
        }
        let len = self.file.metadata()?.len();
        if len < self.max_bytes {
            return Ok(());
        }
        // The open fd still names the old inode after a rename. Reopen the
        // active path so the next write cannot land in the rotated file.
        rotate_names(&self.path, self.keep)?;
        self.file = open_append(&self.path)?;
        Ok(())
    }
}

/// `<path>.<index>` for `index >= 1`. Index `0` is the active path.
pub fn rotation_path(path: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{index}"));
    PathBuf::from(name)
}

/// Read non-empty JSONL lines produced by [`Writer`] (or an older file of the
/// same shape). Blank lines are skipped.
pub fn read_all(path: &Path) -> io::Result<Vec<Event>> {
    let text = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw: RawLine = serde_json::from_str(trimmed).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{}: {err}", path.display(), idx + 1),
            )
        })?;
        events.push(Event {
            ts: raw.ts,
            source: raw.source,
            kind: raw.kind,
            severity: raw.severity,
            data: raw.data,
        });
    }
    Ok(events)
}

#[derive(Serialize)]
struct Line<'a> {
    ts: &'a str,
    source: &'a str,
    kind: &'a str,
    severity: &'a str,
    data: &'a Value,
}

#[derive(Deserialize)]
struct RawLine {
    ts: String,
    source: String,
    kind: String,
    severity: String,
    data: Value,
}

fn encode(ts: &str, event: &Event) -> io::Result<Vec<u8>> {
    let line = Line {
        ts,
        source: &event.source,
        kind: &event.kind,
        severity: &event.severity,
        data: &event.data,
    };
    let mut bytes =
        serde_json::to_vec(&line).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn rotate_names(path: &Path, keep: usize) -> io::Result<()> {
    if keep == 0 {
        return ignore_missing(fs::remove_file(path));
    }
    ignore_missing(fs::remove_file(rotation_path(path, keep)))?;
    for index in (1..keep).rev() {
        let from = rotation_path(path, index);
        if from.exists() {
            fs::rename(&from, rotation_path(path, index + 1))?;
        }
    }
    if path.exists() {
        fs::rename(path, rotation_path(path, 1))?;
    }
    Ok(())
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}
