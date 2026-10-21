// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Durable, append-only JSONL log of a run.
//!
//! Line kinds:
//! - `{"t":"header","crate":"galion-vending","format":1,"seed":..,"days":..}`
//!   (standard level; unchanged), or format 2 with `"level":"hard"`
//! - `{"t":"call","day":..,"call":{tool,args},"ok":bool,"result"|"error":..}`
//! - `{"t":"day_end","day":..,"state_hash":".."}` (fsync'd)
//!
//! The world is a pure function of the seed, so the calls alone rebuild the
//! state. [`resume`] replays every complete day, checks each `state_hash`
//! (a mismatch means the log or the engine changed: hard error), drops a
//! half-finished day, and continues appending. A crash therefore costs at
//! most the shift in progress, never a lie about what already happened.

use crate::sim::Sim;
use crate::tools::{ToolCall, ToolError, ToolOk};
use crate::world::Level;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Header format for standard runs (unchanged since the first release).
pub const FORMAT: u64 = 1;
/// Header format for runs at a non-standard level (carries `level`).
pub const FORMAT_LEVEL: u64 = 2;

pub struct Journal {
    file: File,
    path: PathBuf,
}

impl Journal {
    /// Create (truncate) a journal and write the header.
    pub fn create(path: &Path, seed: u64, days: u32, level: Level) -> std::io::Result<Self> {
        let file = File::create(path)?;
        let mut j = Journal {
            file,
            path: path.to_path_buf(),
        };
        if level.is_standard() {
            j.line(&json!({"t": "header", "crate": "galion-vending", "format": FORMAT, "seed": seed, "days": days}));
        } else {
            j.line(&json!({"t": "header", "crate": "galion-vending", "format": FORMAT_LEVEL, "seed": seed, "days": days, "level": level}));
        }
        j.file.sync_data()?;
        Ok(j)
    }

    fn open_append(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(Journal {
            file,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn line(&mut self, v: &Value) {
        // A failing disk must not change the simulation; the error surfaces
        // at resume time as a truncated day.
        let _ = writeln!(self.file, "{v}");
    }

    pub(crate) fn call(&mut self, day: u32, call: &ToolCall, res: &Result<ToolOk, ToolError>) {
        let v = match res {
            Ok(r) => json!({"t": "call", "day": day, "call": call, "ok": true, "result": r}),
            Err(e) => json!({"t": "call", "day": day, "call": call, "ok": false, "error": e}),
        };
        self.line(&v);
    }

    pub(crate) fn day_end(&mut self, day: u32, hash: &str) {
        self.line(&json!({"t": "day_end", "day": day, "state_hash": hash}));
        let _ = self.file.flush();
        let _ = self.file.sync_data();
    }
}

/// New standard simulation that journals to `path`.
pub fn create(path: &Path, seed: u64, days: u32) -> std::io::Result<Sim> {
    create_level(path, seed, days, Level::Standard)
}

/// New simulation at `level` that journals to `path`.
pub fn create_level(path: &Path, seed: u64, days: u32, level: Level) -> std::io::Result<Sim> {
    let mut sim = Sim::with_level(seed, days, level);
    sim.attach(Journal::create(path, seed, days, level)?);
    Ok(sim)
}

#[derive(Debug)]
pub enum ResumeError {
    Io(std::io::Error),
    Format(String),
    Diverged {
        day: u32,
        logged: String,
        replayed: String,
    },
}

impl std::fmt::Display for ResumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResumeError::Io(e) => write!(f, "io: {e}"),
            ResumeError::Format(m) => write!(f, "bad journal: {m}"),
            ResumeError::Diverged {
                day,
                logged,
                replayed,
            } => {
                write!(
                    f,
                    "replay diverged at day {day}: log {logged}, replay {replayed}"
                )
            }
        }
    }
}

impl std::error::Error for ResumeError {}

impl From<std::io::Error> for ResumeError {
    fn from(e: std::io::Error) -> Self {
        ResumeError::Io(e)
    }
}

/// Replay a journal without touching it. Returns the rebuilt simulation and
/// the byte-exact lines that belong to complete days.
pub fn replay(path: &Path) -> Result<(Sim, Vec<String>), ResumeError> {
    let f = File::open(path)?;
    let mut lines = BufReader::new(f).lines();
    let header_line = lines
        .next()
        .ok_or_else(|| ResumeError::Format("empty".into()))??;
    let header: Value =
        serde_json::from_str(&header_line).map_err(|e| ResumeError::Format(e.to_string()))?;
    if header["t"] != "header" {
        return Err(ResumeError::Format("missing or unknown header".into()));
    }
    let level = if header["format"] == FORMAT {
        Level::Standard
    } else if header["format"] == FORMAT_LEVEL {
        header["level"]
            .as_str()
            .and_then(Level::parse)
            .ok_or_else(|| ResumeError::Format("missing or unknown level".into()))?
    } else {
        return Err(ResumeError::Format("missing or unknown header".into()));
    };
    let seed = header["seed"]
        .as_u64()
        .ok_or_else(|| ResumeError::Format("seed".into()))?;
    let days = header["days"]
        .as_u64()
        .ok_or_else(|| ResumeError::Format("days".into()))? as u32;
    let mut sim = Sim::with_level(seed, days, level);
    let mut kept = vec![header_line];
    let mut day_buf: Vec<String> = Vec::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    for line in lines {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            break; // torn write at the tail
        };
        match v["t"].as_str() {
            Some("call") => {
                let c: ToolCall = serde_json::from_value(v["call"].clone())
                    .map_err(|e| ResumeError::Format(e.to_string()))?;
                calls.push(c);
                day_buf.push(line);
            }
            Some("day_end") => {
                let day = v["day"].as_u64().unwrap_or(0) as u32;
                let logged = v["state_hash"].as_str().unwrap_or("").to_string();
                for c in calls.drain(..) {
                    let _ = sim.call(c);
                }
                let replayed = sim.day_hashes.last().cloned().unwrap_or_default();
                if replayed != logged || sim.day_hashes.len() as u32 != day {
                    return Err(ResumeError::Diverged {
                        day,
                        logged,
                        replayed,
                    });
                }
                day_buf.push(line);
                kept.append(&mut day_buf);
            }
            _ => return Err(ResumeError::Format(format!("unknown line: {line}"))),
        }
    }
    Ok((sim, kept))
}

/// Rebuild from `path`, drop any half-finished shift, keep journaling.
pub fn resume(path: &Path) -> Result<Sim, ResumeError> {
    let (mut sim, kept) = replay(path)?;
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = File::create(&tmp)?;
        for l in &kept {
            writeln!(f, "{l}")?;
        }
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    sim.attach(Journal::open_append(path)?);
    Ok(sim)
}
