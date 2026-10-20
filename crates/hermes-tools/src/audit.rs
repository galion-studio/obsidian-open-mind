// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Append-only JSON Lines audit log: one line per coding-tool decision
//! (allowed or refused), with a Europe/Zagreb timestamp. Arguments are
//! redacted and cut; file contents are never logged, only their size.

use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Audit {
    path: PathBuf,
    lock: Mutex<()>,
}

/// Cut a string for the log, then redact secret-looking words.
pub fn cut(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    let t = if s.chars().count() > n {
        format!("{t}...")
    } else {
        t
    };
    hermes_rs::redact(&t)
}

impl Audit {
    pub fn new(path: PathBuf) -> Self {
        Audit {
            path,
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Write one line. Returns false when the log could not be written; the
    /// caller then refuses the action (no unaudited tool runs).
    pub fn record(&self, mut entry: Value) -> bool {
        let now = hermes_rs::time::now_secs();
        if let Some(o) = entry.as_object_mut() {
            o.insert("ts".into(), json!(hermes_rs::time::zagreb_rfc3339(now)));
        }
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path);
        match f {
            Ok(mut f) => writeln!(f, "{entry}").is_ok(),
            Err(_) => false,
        }
    }
}
