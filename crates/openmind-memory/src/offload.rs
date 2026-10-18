// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! /v2/offload/{ingest,query-mmd,read-ref}, ported from `offload_server/*`.
//! Files live under `<data_dir>/offload/<sanitized session>/` exactly like the
//! gateway's LocalStorageBackend. The LLM-driven L1/L1.5 offload tasks are
//! not ported (v0): ingest writes refs, pending.jsonl and canvas.mmd only.

use crate::hash::{md5_hex, sha256_hex};
use crate::text::sanitize_session_id;
use crate::util::rand_hex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub fn request_id() -> String {
    format!("req-{}", rand_hex(8))
}

pub fn success(data: Value, rid: &str) -> Value {
    json!({ "code": 0, "message": "ok", "request_id": rid, "data": data })
}

pub fn error(code: u16, msg: &str, rid: &str) -> Value {
    json!({ "code": code, "message": msg, "request_id": rid })
}

fn valid_session_id(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    let ok = !s.is_empty()
        && s.chars().count() <= 500
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':'));
    ok.then(|| s.to_string())
}

pub fn base_path(session_id: &str) -> String {
    format!("offload/{}", sanitize_session_id(session_id))
}

pub struct Offload {
    root: PathBuf,
    read_only: bool,
    lock: Mutex<()>,
}

/// `JSON.stringify(value)` for a possibly-absent value ("" when undefined).
fn js_stringify(v: Option<&Value>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

fn node_id(tool_call_id: &str, canonical: &str) -> String {
    let stripped: String = tool_call_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if stripped.is_empty() {
        return format!("n{}", &sha256_hex(canonical.as_bytes())[..12]);
    }
    format!("n{stripped}").chars().take(24).collect()
}

fn canonical(tp: &Value) -> String {
    let g = |k: &str| tp.get(k).cloned().unwrap_or(Value::Null);
    json!({
        "tool_name": g("tool_name"), "tool_call_id": g("tool_call_id"), "params": g("params"),
        "result": g("result"), "error": g("error"), "timestamp": g("timestamp"), "duration_ms": g("duration_ms"),
    })
    .to_string()
}

fn mermaid_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "'")
        .replace("\r\n", " ")
        .replace(['\r', '\n'], " ")
}

fn render_ref(nid: &str, tp: &Value) -> String {
    let result = match tp.get("result") {
        Some(Value::String(s)) => s.clone(),
        other => js_stringify(other),
    };
    let mut lines = vec![
        format!("# node_id: {nid}"),
        format!(
            "tool: {}",
            tp.get("tool_name").and_then(Value::as_str).unwrap_or("")
        ),
        format!(
            "tool_call_id: {}",
            tp.get("tool_call_id").and_then(Value::as_str).unwrap_or("")
        ),
        "```json".into(),
        js_stringify(tp.get("params")),
        "```".into(),
        "## result".into(),
        result,
    ];
    if let Some(Value::String(e)) = tp.get("error") {
        lines.push("## error".into());
        lines.push(e.clone());
    }
    lines.join("\n") + "\n"
}

fn render_canvas(nodes: &[(String, String)]) -> String {
    let mut lines = vec![
        "%% each node drills to refs/<nodeId>.md".to_string(),
        "flowchart TD".into(),
    ];
    for (id, name) in nodes {
        lines.push(format!("  {id}[\"{}\"]", mermaid_label(name)));
    }
    for w in nodes.windows(2) {
        lines.push(format!("  {} --> {}", w[0].0, w[1].0));
    }
    lines.join("\n") + "\n"
}

/// Rough token count for non-`full` read-ref slicing (the gateway uses
/// tiktoken o200k; ~4 UTF-8 bytes per token is the documented approximation).
fn count_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

fn truncate_prefix(text: &str, max_tokens: usize) -> (String, bool) {
    if count_tokens(text) <= max_tokens {
        return (text.to_string(), false);
    }
    let mut end = (max_tokens * 4).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

impl Offload {
    pub fn new(root: &Path, read_only: bool) -> Offload {
        Offload {
            root: root.to_path_buf(),
            read_only,
            lock: Mutex::new(()),
        }
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(rel)).ok()
    }

    fn write(&self, rel: &str, content: &str) -> std::io::Result<()> {
        if self.read_only {
            return Ok(());
        }
        let p = self.root.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, content)
    }

    fn append(&self, rel: &str, content: &str) -> std::io::Result<()> {
        use std::io::Write;
        if self.read_only {
            return Ok(());
        }
        let p = self.root.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)?
            .write_all(content.as_bytes())
    }

    /// POST /v2/offload/ingest
    pub fn ingest(&self, body: &Value, rid: &str) -> (u16, Value) {
        let Some(sid) = valid_session_id(body.get("session_id")) else {
            return (400, error(400, "session_id: Must only contain alphanumeric, underscore, hyphen, dot, or colon characters (1-500 chars)", rid));
        };
        let pairs: Vec<Value> = match body.get("tool_pairs") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => a.clone(),
            Some(_) => return (400, error(400, "tool_pairs: Expected array", rid)),
        };
        for tp in &pairs {
            let s = |k: &str| tp.get(k).is_some_and(Value::is_string);
            let ok = s("tool_name")
                && s("tool_call_id")
                && s("timestamp")
                && tp.get("error").is_none_or(Value::is_string)
                && tp.get("duration_ms").is_none_or(Value::is_number);
            if !ok {
                return (
                    400,
                    error(
                        400,
                        "tool_pairs: each pair needs string tool_name, tool_call_id, timestamp",
                        rid,
                    ),
                );
            }
        }
        let prompt = match body.get("prompt") {
            None | Some(Value::Null) => None,
            Some(Value::String(p)) if !p.trim().is_empty() => Some(p.trim().to_string()),
            Some(_) => {
                return (
                    400,
                    error(400, "prompt must not be empty or whitespace-only", rid),
                )
            }
        };
        let recent: Vec<(String, String)> = match body.get("recent_messages") {
            Some(Value::Array(a)) => {
                let mut v = Vec::new();
                for m in a {
                    match (
                        m.get("role").and_then(Value::as_str),
                        m.get("content").and_then(Value::as_str),
                    ) {
                        (Some(r @ ("user" | "assistant")), Some(c)) => {
                            v.push((r.to_string(), c.to_string()))
                        }
                        _ => return (400, error(400, "recent_messages: invalid item", rid)),
                    }
                }
                v
            }
            _ => Vec::new(),
        };
        if pairs.is_empty() && prompt.is_none() {
            return (
                400,
                error(
                    400,
                    "Either tool_pairs must be non-empty or prompt must be provided",
                    rid,
                ),
            );
        }
        let skip = sid.contains("subagent") || is_internal_session(&sid);
        if pairs.is_empty() || skip {
            // L1.5 task judgement needs the LLM pipeline (not in v0): accept, do nothing.
            return (200, success(json!({}), rid));
        }
        let base = base_path(&sid);
        if prompt.is_some() || !recent.is_empty() {
            let mut parts = Vec::new();
            if !recent.is_empty() {
                parts.push(
                    "\u{5386}\u{53F2}\u{6D88}\u{606F}，\u{53EF}\u{4F5C}\u{4E3A}\u{53C2}\u{8003}："
                        .to_string(),
                );
                for (r, c) in &recent {
                    parts.push(format!(
                        "[{}]: {c}",
                        if r == "user" { "User" } else { "Assistant" }
                    ));
                }
            }
            if let Some(p) = &prompt {
                parts.push(format!("\n\u{6700}\u{65B0}user message：\n[User]: {p}"));
            }
            let _ = self.write(&format!("{base}/recent-context.txt"), &parts.join("\n"));
        }
        let _g = self.lock.lock();
        let mut lines = String::new();
        for tp in &pairs {
            let mut o = serde_json::Map::new();
            o.insert("toolName".into(), tp["tool_name"].clone());
            o.insert("toolCallId".into(), tp["tool_call_id"].clone());
            for (src, dst) in [
                ("params", "params"),
                ("result", "result"),
                ("error", "error"),
            ] {
                if let Some(v) = tp.get(src) {
                    o.insert(dst.into(), v.clone());
                }
            }
            o.insert("timestamp".into(), tp["timestamp"].clone());
            if let Some(v) = tp.get("duration_ms") {
                o.insert("durationMs".into(), v.clone());
            }
            lines.push_str(&Value::Object(o).to_string());
            lines.push('\n');
        }
        if let Err(e) = self.append(&format!("{base}/pending.jsonl"), &lines) {
            return (500, error(500, &format!("write failed: {e}"), rid));
        }
        let ids = self.write_refs_and_canvas(&base, &pairs);
        (200, success(json!({ "node_ids": ids }), rid))
    }

    fn write_refs_and_canvas(&self, base: &str, pairs: &[Value]) -> Vec<String> {
        let st = |tp: &Value, k: &str| tp.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let planned: Vec<(usize, String)> = pairs
            .iter()
            .enumerate()
            .map(|(i, tp)| (i, node_id(&st(tp, "tool_call_id"), &canonical(tp))))
            .collect();
        let ids: Vec<String> = planned.iter().map(|p| p.1.clone()).collect();
        let mut order = planned.clone();
        order.sort_by(|a, b| {
            st(&pairs[a.0], "timestamp")
                .cmp(&st(&pairs[b.0], "timestamp"))
                .then(a.0.cmp(&b.0))
        });
        let mut batch: HashMap<String, (String, String, usize)> = HashMap::new();
        for (i, nid) in &order {
            let _ = self.write(
                &format!("{base}/refs/{nid}.md"),
                &render_ref(nid, &pairs[*i]),
            );
            batch.insert(
                nid.clone(),
                (st(&pairs[*i], "tool_name"), st(&pairs[*i], "timestamp"), *i),
            );
        }
        // Existing refs with mtimes.
        let mut mtime: HashMap<String, i64> = HashMap::new();
        let listed_ok = match std::fs::read_dir(self.root.join(base).join("refs")) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let Some(id) = name.strip_suffix(".md").filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(true) {
                        continue;
                    }
                    let mt = e
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    let v = mtime.entry(id.to_string()).or_insert(mt);
                    *v = (*v).min(mt);
                }
                true
            }
            Err(_) => false,
        };
        // pending.jsonl metadata.
        let mut meta: HashMap<String, (String, String, usize)> = HashMap::new();
        let mut pending_ids = Vec::new();
        if let Some(raw) = self.read(&format!("{base}/pending.jsonl")) {
            let mut seq = 0usize;
            for line in raw.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
                let Ok(row) = serde_json::from_str::<Value>(line) else {
                    seq += 1;
                    continue;
                };
                let g = |a: &str, b: &str| row.get(a).or_else(|| row.get(b)).cloned();
                let tcid = g("toolCallId", "tool_call_id")
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default();
                let tname = g("toolName", "tool_name")
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default();
                let ts = row
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let canon = json!({
                    "tool_name": tname, "tool_call_id": tcid, "params": row.get("params").cloned().unwrap_or(Value::Null),
                    "result": row.get("result").cloned().unwrap_or(Value::Null),
                    "error": row.get("error").filter(|v| v.is_string()).cloned().unwrap_or(Value::Null),
                    "timestamp": ts, "duration_ms": g("durationMs", "duration_ms").unwrap_or(Value::Null),
                }).to_string();
                let nid = node_id(&tcid, &canon);
                if !meta.contains_key(&nid) {
                    meta.insert(nid.clone(), (tname.clone(), ts.clone(), seq));
                    pending_ids.push(nid.clone());
                }
                if !tcid.is_empty() && tcid != nid && !meta.contains_key(&tcid) {
                    meta.insert(tcid, (tname, ts, seq));
                }
                seq += 1;
            }
        }
        if !listed_ok {
            for id in &pending_ids {
                mtime.entry(id.clone()).or_insert(0);
            }
        }
        let now = crate::util::now_ms();
        for (i, id) in ids.iter().enumerate() {
            mtime.entry(id.clone()).or_insert(now + i as i64);
        }
        let mut ranked: Vec<(String, String, String, usize, i64)> = mtime
            .into_iter()
            .map(|(id, mt)| {
                let fb = batch.get(&id);
                let fp = meta.get(&id);
                let name = fb
                    .map(|b| b.0.clone())
                    .or_else(|| fp.map(|p| p.0.clone()))
                    .unwrap_or_else(|| id.clone());
                let ts = fp
                    .map(|p| p.1.clone())
                    .filter(|s| !s.is_empty())
                    .or_else(|| fb.map(|b| b.1.clone()))
                    .unwrap_or_default();
                let seq = fp
                    .map(|p| p.2)
                    .or_else(|| fb.map(|b| b.2))
                    .unwrap_or(usize::MAX);
                (id, name, ts, seq, mt)
            })
            .collect();
        ranked.sort_by(|a, b| {
            use std::cmp::Ordering::*;
            let t = match (a.2.is_empty(), b.2.is_empty()) {
                (true, false) => Greater,
                (false, true) => Less,
                _ => a.2.cmp(&b.2),
            };
            t.then(a.3.cmp(&b.3))
                .then(a.4.cmp(&b.4))
                .then(a.0.cmp(&b.0))
        });
        let nodes: Vec<(String, String)> = ranked.into_iter().map(|r| (r.0, r.1)).collect();
        let _ = self.write(&format!("{base}/canvas.mmd"), &render_canvas(&nodes));
        ids
    }

    /// POST /v2/offload/query-mmd
    pub fn query_mmd(&self, body: &Value, rid: &str) -> (u16, Value) {
        let Some(sid) = valid_session_id(body.get("session_id")) else {
            return (
                400,
                error(400, "missing or invalid session_id in body", rid),
            );
        };
        let limit = match body.get("limit") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_u64().filter(|n| *n >= 1) {
                Some(n) => Some(n as usize),
                None => {
                    return (
                        400,
                        error(400, "missing or invalid session_id in body", rid),
                    )
                }
            },
        };
        let base = base_path(&sid);
        let state: Value = self
            .read(&format!("{base}/state.json"))
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or(json!({}));
        let active = state
            .get("activeMmdFile")
            .and_then(Value::as_str)
            .map(String::from);
        let ver = |c: &str| {
            if c.is_empty() {
                String::new()
            } else {
                md5_hex(c.as_bytes())[..12].to_string()
            }
        };
        let mut mmds: Vec<(String, String, String)> = Vec::new();
        match (&active, limit) {
            (Some(a), Some(1)) => {
                let c = self.read(&format!("{base}/mmds/{a}")).unwrap_or_default();
                mmds.push((a.clone(), ver(&c), c));
            }
            _ => {
                let mut names: Vec<String> = std::fs::read_dir(self.root.join(&base).join("mmds"))
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.file_name().to_string_lossy().to_string())
                            .filter(|n| n.ends_with(".mmd"))
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                if let Some(l) = limit {
                    names.truncate(l);
                }
                for n in names {
                    let c = self.read(&format!("{base}/mmds/{n}")).unwrap_or_default();
                    mmds.push((n, ver(&c), c));
                }
            }
        }
        if mmds.iter().all(|m| m.2.trim().is_empty()) {
            if let Some(canvas) = self
                .read(&format!("{base}/canvas.mmd"))
                .filter(|c| !c.trim().is_empty())
            {
                let v = ver(&canvas);
                if mmds.is_empty() {
                    mmds.push(("canvas.mmd".into(), v, canvas));
                } else {
                    let i = mmds.iter().position(|m| m.2.trim().is_empty()).unwrap_or(0);
                    mmds[i].1 = v;
                    mmds[i].2 = canvas;
                }
            }
        }
        let list: Vec<Value> = mmds
            .into_iter()
            .map(|(f, v, c)| json!({ "filename": f, "content": c, "version": v }))
            .collect();
        (
            200,
            success(json!({ "mmds": list, "currentMmd": active }), rid),
        )
    }

    /// POST /v2/offload/read-ref
    pub fn read_ref(&self, body: &Value, rid: &str) -> (u16, Value) {
        let bad = |m: &str| (400, error(400, m, rid));
        let Some(sid) = valid_session_id(body.get("session_id")) else {
            return bad("session_id: invalid");
        };
        let Some(rref) = body
            .get("result_ref")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.chars().count() <= 1000)
        else {
            return bad("result_ref: invalid");
        };
        let opt_str = |k: &str| {
            body.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let opt_u = |k: &str| {
            body.get(k)
                .and_then(Value::as_u64)
                .filter(|n| *n >= 1)
                .map(|n| n as usize)
        };
        let query = opt_str("query");
        let (start, end) = (opt_u("start_line"), opt_u("end_line"));
        let max_tokens = body
            .get("max_tokens")
            .and_then(Value::as_u64)
            .filter(|n| (1..=4096).contains(n))
            .unwrap_or(1600) as usize;
        let full = body.get("full").and_then(Value::as_bool).unwrap_or(false);
        if let (Some(s), Some(e)) = (start, end) {
            if s > e {
                return bad("start_line must not exceed end_line");
            }
        }
        if query.is_some() && (start.is_some() || end.is_some()) {
            return bad("query cannot be combined with start_line or end_line");
        }
        let prefix = format!("{}/refs/", base_path(&sid));
        let owned = rref.strip_prefix(prefix.as_str()).filter(|f| {
            !f.is_empty()
                && !f.contains('/')
                && !f.contains('\\')
                && !f.contains('\0')
                && *f != "."
                && *f != ".."
                && f.ends_with(".md")
        });
        let not_found = (404, error(404, "result_ref not found", rid));
        if owned.is_none() {
            return not_found;
        }
        let Some(raw) = self.read(rref) else {
            return not_found;
        };
        let lines_window = |raw: &str| {
            let lines: Vec<&str> = raw
                .split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .collect();
            let si = (start.unwrap_or(1) - 1).min(lines.len());
            let ei = end.unwrap_or(lines.len()).min(lines.len()).max(si);
            (lines[si..ei].join("\n"), si > 0 || ei < lines.len())
        };
        let mut data = json!({ "result_ref": rref });
        let o = data.as_object_mut().expect("object");
        if full {
            if let Some(q) = &query {
                let found = raw.to_lowercase().contains(&q.to_lowercase());
                o.insert(
                    "content".into(),
                    json!(if found { raw.as_str() } else { "" }),
                );
                o.insert("truncated".into(), json!(false));
                o.insert("match_found".into(), json!(found));
            } else if start.is_none() && end.is_none() {
                o.insert("content".into(), json!(raw));
                o.insert("truncated".into(), json!(false));
            } else {
                o.insert("content".into(), json!(lines_window(&raw).0));
                o.insert("truncated".into(), json!(false));
            }
        } else if let Some(q) = &query {
            let lower = raw.to_lowercase();
            match lower.find(&q.to_lowercase()) {
                None => {
                    o.insert("content".into(), json!(""));
                    o.insert("truncated".into(), json!(false));
                    o.insert("match_found".into(), json!(false));
                }
                Some(_) if lower.len() != raw.len() => {
                    // Case folding changed byte offsets: fall back to a prefix window.
                    let (c, t) = truncate_prefix(&raw, max_tokens);
                    o.insert("content".into(), json!(c));
                    o.insert("truncated".into(), json!(t));
                    o.insert("match_found".into(), json!(true));
                }
                Some(at) => {
                    let budget = max_tokens * 4;
                    let mid = at + q.len() / 2;
                    let mut a = mid.saturating_sub(budget / 2);
                    let mut b = (a + budget).min(raw.len());
                    a = b.saturating_sub(budget).min(a);
                    while !raw.is_char_boundary(a) {
                        a -= 1;
                    }
                    while !raw.is_char_boundary(b) {
                        b += 1;
                    }
                    o.insert("content".into(), json!(&raw[a..b]));
                    o.insert("truncated".into(), json!(a > 0 || b < raw.len()));
                    o.insert("match_found".into(), json!(true));
                }
            }
        } else {
            let (sel, cut) = lines_window(&raw);
            let (c, t) = truncate_prefix(&sel, max_tokens);
            o.insert("content".into(), json!(c));
            o.insert("truncated".into(), json!(t || cut));
        }
        (200, success(data, rid))
    }
}

/// `/memory-.*-session-\d+/`
fn is_internal_session(s: &str) -> bool {
    let mut from = 0;
    while let Some(p) = s[from..].find("memory-") {
        let after = from + p + 7;
        let mut k = after;
        while let Some(q) = s[k..].find("-session-") {
            let d = k + q + 9;
            if s[d..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return true;
            }
            k += q + 1;
        }
        from = after;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_and_internal() {
        assert_eq!(node_id("toolu_01ABC", "{}"), "ntoolu01ABC");
        assert_eq!(node_id("", "{}").len(), 13);
        assert!(is_internal_session("x-memory-tdai-session-12"));
        assert!(!is_internal_session("memory-session-x"));
        assert_eq!(base_path("a:b/c"), "offload/a_b_c");
    }
}
