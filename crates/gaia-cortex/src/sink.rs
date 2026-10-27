// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Append-only JSONL logs: decisions, alerts, the escalate-to-human queue,
//! and the optional alert outbox. In-memory rings back the API.

use serde_json::{json, Value};
use std::collections::VecDeque;
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
pub struct Escalations {
    pub log: Jsonl,
    pub open: VecDeque<Value>,
}

impl Escalations {
    pub fn new(path: Option<PathBuf>) -> Self {
        Escalations {
            log: Jsonl::new(path),
            open: VecDeque::new(),
        }
    }

    pub fn push(&mut self, v: Value) {
        self.log.push(v.clone());
        self.open.push_back(v);
        while self.open.len() > RING {
            self.open.pop_front();
        }
    }

    pub fn ack(&mut self, id: &str, now_ts: &str) -> bool {
        let before = self.open.len();
        self.open.retain(|v| v["id"] != id);
        let hit = self.open.len() != before;
        if hit {
            if let Some(p) = &self.log.path {
                let _ = append(p, &json!({"ack": id, "ts": now_ts}));
            }
        }
        hit
    }
}
