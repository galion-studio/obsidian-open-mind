// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Append-only audit trail for every banking tool call.
//!
//! Each entry carries a Europe/Zagreb RFC 3339 stamp, the tool, redacted
//! args, the outcome (`ok`, `draft`, `error`, `refused`), the HTTP calls it
//! made (method, path, status; never headers or bodies) and a hash chain
//! (`prev` -> `hash`). The chain uses FNV-1a 64: it makes accidental edits
//! and truncation visible; it is NOT a cryptographic signature (a signed
//! log is listed as a V2 follow-up in `docs/bench/hermes-banking-tools.md`).

use crate::money::mask_iban;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const GENESIS: &str = "0000000000000000";

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub seq: u64,
    pub at: String,
    pub tool: String,
    pub args: Value,
    pub outcome: String,
    pub detail: String,
    pub http: Vec<String>,
    pub prev: String,
    pub hash: String,
}

impl Entry {
    fn body(&self) -> Value {
        json!({"seq": self.seq, "at": self.at, "tool": self.tool, "args": self.args,
               "outcome": self.outcome, "detail": self.detail, "http": self.http,
               "prev": self.prev})
    }

    pub fn to_json(&self) -> Value {
        let mut v = self.body();
        v["hash"] = json!(self.hash);
        v
    }

    pub fn from_json(v: &Value) -> Option<Entry> {
        Some(Entry {
            seq: v["seq"].as_u64()?,
            at: v["at"].as_str()?.into(),
            tool: v["tool"].as_str()?.into(),
            args: v["args"].clone(),
            outcome: v["outcome"].as_str()?.into(),
            detail: v["detail"].as_str()?.into(),
            http: v["http"]
                .as_array()?
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            prev: v["prev"].as_str()?.into(),
            hash: v["hash"].as_str()?.into(),
        })
    }

    fn compute_hash(&self) -> String {
        fnv1a(self.body().to_string().as_bytes())
    }
}

pub fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Keys redacted when they contain one of these.
const SECRET_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "authorization",
    "card_number",
];
/// Keys redacted on exact match (short words that hide inside others).
const SECRET_EXACT: &[&str] = &["pin", "pan", "cvv", "cvc"];

/// Redact secret-looking keys and mask IBANs, recursively.
pub fn redact(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                let lk = k.to_ascii_lowercase();
                let nv = if SECRET_PARTS.iter().any(|s| lk.contains(s))
                    || SECRET_EXACT.contains(&lk.as_str())
                {
                    json!("[redacted]")
                } else if lk.contains("iban") {
                    json!(mask_iban(val.as_str().unwrap_or_default()))
                } else {
                    redact(val)
                };
                out.insert(k.clone(), nv);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(redact).collect()),
        other => other.clone(),
    }
}

#[derive(Debug, Default)]
pub struct Audit {
    pub entries: Vec<Entry>,
    path: Option<PathBuf>,
}

impl Audit {
    pub fn new() -> Self {
        Audit::default()
    }

    /// Also append every entry as one JSON line to `path`.
    pub fn with_file(path: &Path) -> Self {
        Audit {
            entries: Vec::new(),
            path: Some(path.to_path_buf()),
        }
    }

    pub fn record(
        &mut self,
        tool: &str,
        args: &Value,
        outcome: &str,
        detail: &str,
        http: Vec<String>,
    ) -> &Entry {
        let prev = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| GENESIS.to_string());
        let mut e = Entry {
            seq: self.entries.len() as u64 + 1,
            at: zagreb_time::now_rfc3339(),
            tool: tool.to_string(),
            args: redact(args),
            outcome: outcome.to_string(),
            detail: detail.chars().take(400).collect(),
            http,
            prev,
            hash: String::new(),
        };
        e.hash = e.compute_hash();
        if let Some(p) = &self.path {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let _ = writeln!(f, "{}", e.to_json());
            }
        }
        self.entries.push(e);
        self.entries.last().expect("just pushed")
    }

    pub fn to_jsonl(&self) -> String {
        self.entries
            .iter()
            .map(|e| e.to_json().to_string() + "\n")
            .collect()
    }

    /// Verify the chain. `Err(i)` is the 0-based index of the first bad entry.
    pub fn verify(entries: &[Entry]) -> Result<(), usize> {
        let mut prev = GENESIS.to_string();
        for (i, e) in entries.iter().enumerate() {
            if e.seq != i as u64 + 1 || e.prev != prev || e.compute_hash() != e.hash {
                return Err(i);
            }
            prev = e.hash.clone();
        }
        Ok(())
    }

    pub fn parse_jsonl(s: &str) -> Option<Vec<Entry>> {
        s.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                serde_json::from_str::<Value>(l)
                    .ok()
                    .and_then(|v| Entry::from_json(&v))
            })
            .collect()
    }
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn chain_and_redaction() {
        let mut a = Audit::new();
        a.record(
            "x",
            &json!({"refresh_token": "s3", "iban": "HR1210010051863000160", "n": {"pin": "1"}}),
            "ok",
            "",
            vec![],
        );
        a.record("y", &json!({}), "draft", "d", vec!["GET /a -> 200".into()]);
        assert_eq!(Audit::verify(&a.entries), Ok(()));
        let j = a.to_jsonl();
        assert!(!j.contains("s3") && !j.contains("863000160") && j.contains("HR12 **** 0160"));
        let mut parsed = Audit::parse_jsonl(&j).unwrap();
        assert_eq!(Audit::verify(&parsed), Ok(()));
        parsed[0].detail = "edited".into();
        assert_eq!(Audit::verify(&parsed), Err(0));
        parsed.truncate(1);
        assert!(Audit::verify(&parsed).is_err());
    }
}
