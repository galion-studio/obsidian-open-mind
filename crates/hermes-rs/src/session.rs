// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Session store. `SessionStore` is the interface; `MemStore` (tests),
//! `JsonlStore` (pure Rust, phones / Pi) and, with the `sqlite` feature,
//! `session_sqlite::SqliteStore` (state.db-compatible) implement it.
//! `Layered` reads a legacy read-only store as a fallback.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub source: String,
    pub session_key: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub title: Option<String>,
    pub parent_session_id: Option<String>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub message_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

impl SessionRow {
    pub fn new(id: &str, source: &str, key: Option<&str>) -> Self {
        SessionRow {
            id: id.into(),
            source: source.into(),
            session_key: key.map(str::to_string),
            started_at: crate::time::now_f64(),
            ..Default::default()
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "source": self.source, "session_key": self.session_key, "model": self.model, "billing_provider": self.provider,
            "title": self.title, "parent_session_id": self.parent_session_id, "started_at": self.started_at,
            "ended_at": self.ended_at, "message_count": self.message_count, "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens, "estimated_cost_usd": self.cost_usd,
        })
    }

    fn from_json(v: &Value) -> Option<Self> {
        let st = |k: &str| v[k].as_str().map(str::to_string);
        Some(SessionRow {
            id: st("id")?,
            source: st("source").unwrap_or_else(|| "api_server".into()),
            session_key: st("session_key"),
            model: st("model"),
            provider: st("billing_provider"),
            title: st("title"),
            parent_session_id: st("parent_session_id"),
            started_at: v["started_at"].as_f64().unwrap_or(0.0),
            ended_at: v["ended_at"].as_f64(),
            message_count: v["message_count"].as_u64().unwrap_or(0),
            input_tokens: v["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["output_tokens"].as_u64().unwrap_or(0),
            cost_usd: v["estimated_cost_usd"].as_f64().unwrap_or(0.0),
        })
    }
}

pub trait SessionStore: Send + Sync {
    fn get(&self, id: &str) -> Option<SessionRow>;
    /// Latest live (not ended) session for a declared conversation key.
    fn find_by_key(&self, source: &str, key: &str) -> Option<SessionRow>;
    fn upsert(&self, row: &SessionRow) -> Result<(), String>;
    /// Conversation messages (role/content/tool_calls/tool_call_id/name).
    fn messages(&self, id: &str) -> Vec<Value>;
    fn append(&self, id: &str, msgs: &[Value]) -> Result<(), String>;
    fn list(&self, limit: usize) -> Vec<SessionRow>;
    fn writable(&self) -> bool {
        true
    }
}

/// Normalise a stored message to the chat wire shape.
pub fn wire_message(m: &Value) -> Value {
    let mut o =
        json!({"role": m["role"], "content": m.get("content").cloned().unwrap_or(Value::Null)});
    for k in ["tool_calls", "tool_call_id", "name"] {
        if let Some(x) = m.get(k) {
            if !x.is_null() {
                o[k] = x.clone();
            }
        }
    }
    o
}

#[derive(Default)]
pub struct MemStore {
    rows: Mutex<BTreeMap<String, SessionRow>>,
    msgs: Mutex<BTreeMap<String, Vec<Value>>>,
}

impl SessionStore for MemStore {
    fn get(&self, id: &str) -> Option<SessionRow> {
        self.rows.lock().ok()?.get(id).cloned()
    }
    fn find_by_key(&self, source: &str, key: &str) -> Option<SessionRow> {
        let rows = self.rows.lock().ok()?;
        rows.values()
            .filter(|r| {
                r.source == source && r.session_key.as_deref() == Some(key) && r.ended_at.is_none()
            })
            .max_by(|a, b| {
                a.started_at
                    .partial_cmp(&b.started_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }
    fn upsert(&self, row: &SessionRow) -> Result<(), String> {
        self.rows
            .lock()
            .map_err(|_| "lock")?
            .insert(row.id.clone(), row.clone());
        Ok(())
    }
    fn messages(&self, id: &str) -> Vec<Value> {
        self.msgs
            .lock()
            .ok()
            .and_then(|m| m.get(id).cloned())
            .unwrap_or_default()
    }
    fn append(&self, id: &str, msgs: &[Value]) -> Result<(), String> {
        let mut m = self.msgs.lock().map_err(|_| "lock")?;
        m.entry(id.to_string())
            .or_default()
            .extend(msgs.iter().map(wire_message));
        let n = m[id].len() as u64;
        drop(m);
        if let Some(mut r) = self.get(id) {
            r.message_count = n;
            self.upsert(&r)?;
        }
        Ok(())
    }
    fn list(&self, limit: usize) -> Vec<SessionRow> {
        let mut v: Vec<SessionRow> = self
            .rows
            .lock()
            .map(|r| r.values().cloned().collect())
            .unwrap_or_default();
        v.sort_by(|a, b| {
            b.started_at
                .partial_cmp(&a.started_at)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v.truncate(limit);
        v
    }
}

/// Pure-Rust store for devices without SQLite: `index.json` (rows) plus one
/// append-only `<id>.jsonl` per session.
pub struct JsonlStore {
    dir: PathBuf,
    index: Mutex<BTreeMap<String, SessionRow>>,
}

fn safe_id(id: &str) -> Option<String> {
    let ok = !id.is_empty()
        && id.len() <= 256
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:@".contains(c))
        && !id.contains("..");
    ok.then(|| id.replace(':', "_"))
}

impl JsonlStore {
    pub fn open(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut index = BTreeMap::new();
        if let Ok(text) = fs::read_to_string(dir.join("index.json")) {
            if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(&text) {
                for v in a {
                    if let Some(r) = SessionRow::from_json(&v) {
                        index.insert(r.id.clone(), r);
                    }
                }
            }
        }
        Ok(JsonlStore {
            dir,
            index: Mutex::new(index),
        })
    }

    fn save(&self, idx: &BTreeMap<String, SessionRow>) -> Result<(), String> {
        let v = Value::Array(idx.values().map(SessionRow::to_json).collect());
        let tmp = self.dir.join("index.json.tmp");
        fs::write(&tmp, v.to_string()).map_err(|e| e.to_string())?;
        fs::rename(&tmp, self.dir.join("index.json")).map_err(|e| e.to_string())
    }
}

impl SessionStore for JsonlStore {
    fn get(&self, id: &str) -> Option<SessionRow> {
        self.index.lock().ok()?.get(id).cloned()
    }
    fn find_by_key(&self, source: &str, key: &str) -> Option<SessionRow> {
        let idx = self.index.lock().ok()?;
        idx.values()
            .filter(|r| {
                r.source == source && r.session_key.as_deref() == Some(key) && r.ended_at.is_none()
            })
            .max_by(|a, b| {
                a.started_at
                    .partial_cmp(&b.started_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }
    fn upsert(&self, row: &SessionRow) -> Result<(), String> {
        safe_id(&row.id).ok_or("bad session id")?;
        let mut idx = self.index.lock().map_err(|_| "lock")?;
        idx.insert(row.id.clone(), row.clone());
        self.save(&idx)
    }
    fn messages(&self, id: &str) -> Vec<Value> {
        let Some(f) = safe_id(id) else {
            return Vec::new();
        };
        fs::read_to_string(self.dir.join(format!("{f}.jsonl")))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
    fn append(&self, id: &str, msgs: &[Value]) -> Result<(), String> {
        let f = safe_id(id).ok_or("bad session id")?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(format!("{f}.jsonl")))
            .map_err(|e| e.to_string())?;
        let mut buf = String::new();
        for m in msgs {
            buf.push_str(&wire_message(m).to_string());
            buf.push('\n');
        }
        file.write_all(buf.as_bytes()).map_err(|e| e.to_string())?;
        let mut idx = self.index.lock().map_err(|_| "lock")?;
        if let Some(r) = idx.get_mut(id) {
            r.message_count += msgs.len() as u64;
        }
        self.save(&idx)
    }
    fn list(&self, limit: usize) -> Vec<SessionRow> {
        let mut v: Vec<SessionRow> = self
            .index
            .lock()
            .map(|r| r.values().cloned().collect())
            .unwrap_or_default();
        v.sort_by(|a, b| {
            b.started_at
                .partial_cmp(&a.started_at)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v.truncate(limit);
        v
    }
}

/// Own store first, then a legacy read-only store (the Python state.db).
pub struct Layered {
    pub primary: Box<dyn SessionStore>,
    pub legacy: Option<Box<dyn SessionStore>>,
}

impl SessionStore for Layered {
    fn get(&self, id: &str) -> Option<SessionRow> {
        self.primary
            .get(id)
            .or_else(|| self.legacy.as_ref()?.get(id))
    }
    fn find_by_key(&self, source: &str, key: &str) -> Option<SessionRow> {
        self.primary
            .find_by_key(source, key)
            .or_else(|| self.legacy.as_ref()?.find_by_key(source, key))
    }
    fn upsert(&self, row: &SessionRow) -> Result<(), String> {
        self.primary.upsert(row)
    }
    fn messages(&self, id: &str) -> Vec<Value> {
        let own = self.primary.messages(id);
        if own.is_empty() && self.primary.get(id).is_none() {
            if let Some(l) = &self.legacy {
                return l.messages(id);
            }
        }
        if let (Some(l), true) = (&self.legacy, self.primary.get(id).is_some()) {
            // A session continued from the legacy DB: legacy history first.
            let mut old = l.messages(id);
            if !old.is_empty() {
                old.extend(own);
                return old;
            }
        }
        own
    }
    fn append(&self, id: &str, msgs: &[Value]) -> Result<(), String> {
        self.primary.append(id, msgs)
    }
    fn list(&self, limit: usize) -> Vec<SessionRow> {
        let mut v = self.primary.list(limit);
        if let Some(l) = &self.legacy {
            for r in l.list(limit) {
                if !v.iter().any(|x| x.id == r.id) {
                    v.push(r);
                }
            }
        }
        v.sort_by(|a, b| {
            b.started_at
                .partial_cmp(&a.started_at)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v.truncate(limit);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise(s: &dyn SessionStore) {
        let mut r = SessionRow::new("sess-1", "api_server", Some("galion-one"));
        r.model = Some("anthropic/claude-haiku-5.5".into());
        s.upsert(&r).unwrap();
        s.append(
            "sess-1",
            &[
                json!({"role":"user","content":"hi","extra":1}),
                json!({"role":"assistant","content":"yo"}),
            ],
        )
        .unwrap();
        assert_eq!(s.messages("sess-1").len(), 2);
        assert!(s.messages("sess-1")[0].get("extra").is_none());
        assert_eq!(s.get("sess-1").unwrap().message_count, 2);
        assert_eq!(
            s.find_by_key("api_server", "galion-one").unwrap().id,
            "sess-1"
        );
        assert!(s.find_by_key("api_server", "other").is_none());
        assert_eq!(s.list(10).len(), 1);
    }

    #[test]
    fn mem_store() {
        exercise(&MemStore::default());
    }

    #[test]
    fn jsonl_store_roundtrip() {
        let dir = std::env::temp_dir().join(crate::new_id("hermes-rs-jsonl-"));
        exercise(&JsonlStore::open(dir.clone()).unwrap());
        let again = JsonlStore::open(dir.clone()).unwrap();
        assert_eq!(again.messages("sess-1").len(), 2);
        assert!(again
            .upsert(&SessionRow::new("../x", "api_server", None))
            .is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
