// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! On-disk corpus for the timed CI fuzz runs (`.github/workflows/fuzz.yml`).
//!
//! `GALION_FUZZ_CORPUS=<dir>` loads every file in `<dir>` as an extra seed
//! (at most [`MAX_FILES`], each at most [`MAX_BYTES`]) and saves inputs that
//! produced a new outcome signature, so the next run (CI cache or box)
//! starts where this one stopped. A panicking input is written to
//! `<dir>/crashes/` before the panic continues, so CI can upload it.
//! Unset: nothing is read or written. File names are an FNV-1a 64 hash of the
//! content, so the same input never lands twice. std only.
//!
//! This file is shared by path (`#[path]`) between galion-common's
//! `http_fuzz` and galion-guard's `fuzz_parser`; it is test-only code.

#![allow(dead_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Most files loaded from or saved to one corpus directory.
pub const MAX_FILES: usize = 512;
/// Largest corpus file loaded or saved (bytes).
pub const MAX_BYTES: usize = 8 * 1024;

pub fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub struct Corpus {
    dir: Option<PathBuf>,
    /// Inputs loaded at start (sorted by name, so runs are reproducible).
    pub loaded: Vec<Vec<u8>>,
    names: HashSet<String>,
    pub saved: usize,
}

impl Corpus {
    /// From `GALION_FUZZ_CORPUS`; `None` dir when unset or empty.
    pub fn from_env() -> Self {
        let dir = std::env::var("GALION_FUZZ_CORPUS")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        Self::open(dir)
    }

    pub fn open(dir: Option<PathBuf>) -> Self {
        let mut c = Corpus {
            dir,
            loaded: vec![],
            names: HashSet::new(),
            saved: 0,
        };
        let Some(d) = c.dir.clone() else { return c };
        let _ = std::fs::create_dir_all(&d);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for p in files {
            if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
                c.names.insert(n.to_string());
            }
            if c.loaded.len() >= MAX_FILES {
                continue;
            }
            if let Ok(b) = std::fs::read(&p) {
                if b.len() <= MAX_BYTES {
                    c.loaded.push(b);
                }
            }
        }
        c
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Total files in the directory (loaded + saved this run).
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Save one interesting input; no-op without a dir, at the cap, for an
    /// oversize input or a duplicate.
    pub fn save(&mut self, input: &[u8]) {
        let Some(d) = &self.dir else { return };
        if self.names.len() >= MAX_FILES || input.len() > MAX_BYTES {
            return;
        }
        let name = format!("{:016x}", fnv(input));
        if self.names.contains(&name) {
            return;
        }
        if std::fs::write(d.join(&name), input).is_ok() {
            self.names.insert(name);
            self.saved += 1;
        }
    }

    /// Write a crashing input to `<dir>/crashes/` (without a dir:
    /// `target/fuzz-crashes/` under the test cwd) and
    /// return the path.
    pub fn save_crash(&self, target: &str, input: &[u8]) -> PathBuf {
        let d = match &self.dir {
            Some(d) => d.join("crashes"),
            None => PathBuf::from("target/fuzz-crashes"),
        };
        let _ = std::fs::create_dir_all(&d);
        let p = d.join(format!("{target}-{:016x}", fnv(input)));
        let _ = std::fs::write(&p, input);
        p
    }
}

/// `GALION_FUZZ_SECS` as seconds (`None` when unset).
pub fn secs() -> Option<u64> {
    std::env::var("GALION_FUZZ_SECS")
        .ok()
        .map(|s| s.parse().unwrap_or(30))
}

/// `GALION_FUZZ_SEED`, else time-based (printed by the caller).
pub fn seed() -> u64 {
    std::env::var("GALION_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
        })
}

/// One line the CI job greps into the step summary.
pub fn summary(
    target: &str,
    execs: u64,
    distinct: usize,
    corpus: &Corpus,
    secs: f64,
    seed: u64,
) -> String {
    format!(
        "fuzz-summary target={target} execs={execs} distinct={distinct} corpus={} new={} crashes=0 secs={secs:.1} rate={:.0}/s seed={seed}",
        corpus.len(),
        corpus.saved,
        execs as f64 / secs.max(0.001),
    )
}
