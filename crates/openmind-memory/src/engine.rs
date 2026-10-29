// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! v1 endpoint logic: /recall, /capture, /search/memories,
//! /search/conversations, /session/end. Ported from `gateway/server.ts`,
//! `core/hooks/auto-recall.ts`, `core/hooks/auto-capture.ts`,
//! `core/conversation/l0-recorder.ts` and `core/tools/*-search.ts`.

use crate::config::Config;
use crate::fts::build_fts_query;
use crate::store::{import_record_id, L0Record, Store};
use crate::text::*;
use crate::util::{iso_utc, now_ms, rand_hex, utc_date};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

pub struct Engine {
    pub cfg: Config,
    pub store: Option<Store>,
    pub started: Instant,
    cursors: Mutex<HashMap<String, i64>>,
    jsonl_lock: Mutex<()>,
}

/// JS truthiness for request fields.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(_) => true,
    }
}

fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn limit_of(v: Option<&Value>, default: usize) -> usize {
    match v {
        None | Some(Value::Null) => default,
        Some(Value::Number(n)) => n
            .as_f64()
            .map(|f| if f > 0.0 { f.min(1000.0) as usize } else { 0 })
            .unwrap_or(default),
        Some(Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .ok()
            .map(|f| if f > 0.0 { f.min(1000.0) as usize } else { 0 })
            .unwrap_or(default),
        _ => default,
    }
}

fn meta_times(meta_json: &str) -> (Option<String>, Option<String>) {
    if meta_json.is_empty() || meta_json == "{}" {
        return (None, None);
    }
    match serde_json::from_str::<Value>(meta_json) {
        Ok(v) => {
            let g = |k: &str| {
                v.get(k)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            };
            (g("activity_start_time"), g("activity_end_time"))
        }
        Err(_) => (None, None),
    }
}

/// Upper bound of ranked rows read when a search filter is set.
const FILTER_SCAN: usize = 5000;

pub fn err(msg: &str) -> (u16, Value) {
    (400, json!({ "error": msg }))
}

impl Engine {
    pub fn new(cfg: Config) -> Engine {
        let path = cfg.db_path();
        let store = if cfg.read_only && !path.exists() {
            crate::log::warn(&format!("read-only and no database at {}", path.display()));
            None
        } else {
            match Store::open(&path, cfg.read_only) {
                Ok(s) => Some(s),
                Err(e) => {
                    crate::log::warn(&format!("store open failed ({}): {e}", path.display()));
                    None
                }
            }
        };
        Engine {
            cfg,
            store,
            started: Instant::now(),
            cursors: Mutex::new(HashMap::new()),
            jsonl_lock: Mutex::new(()),
        }
    }

    pub fn health(&self) -> Value {
        json!({
            "status": if self.store.is_some() { "ok" } else { "degraded" },
            "version": "0.1.0",
            "uptime": self.started.elapsed().as_secs(),
            "stores": { "vectorStore": self.store.is_some(), "embeddingService": false },
            "services": { "timerScanner": null, "pipelineWorker": null, "stateBackend": "none" },
            "implementation": "openmind-memory",
            "implementation_version": env!("CARGO_PKG_VERSION"),
            "read_only": self.cfg.read_only,
            "fts": self.store.as_ref().is_some_and(|s| s.fts_available),
        })
    }

    /// `searchByKeyword` (auto-recall): formatted L1 lines.
    fn keyword_lines(&self, clean: &str, only: Option<&str>) -> Vec<String> {
        let Some(store) = &self.store else {
            return Vec::new();
        };
        if !store.fts_available {
            return Vec::new();
        }
        let Some(q) = build_fts_query(clean) else {
            return Vec::new();
        };
        let max = self.cfg.recall_max_results;
        let hits = match only {
            None => store.search_l1_fts(&q, max * 2),
            // Session filter before the limit; scan a wider window so other
            // sessions cannot starve this one (bounded: 50x the limit).
            Some(sk) => {
                store.search_l1_fts_filtered(&q, max * 2, max * 100, &|h| h.session_key == sk)
            }
        };
        if hits.is_empty() {
            return Vec::new();
        }
        let above: Vec<_> = hits
            .iter()
            .filter(|h| h.score >= self.cfg.recall_threshold)
            .take(max)
            .collect();
        let chosen: Vec<_> = if !above.is_empty() {
            above
        } else if hits.len() <= max {
            hits.iter().take(max).collect()
        } else {
            return Vec::new();
        };
        chosen
            .into_iter()
            .map(|h| {
                let (s, e) = meta_times(&h.metadata_json);
                format_memory_line(
                    &h.kind,
                    &h.content,
                    &h.scene_name,
                    s.as_deref(),
                    e.as_deref(),
                    Some(&h.timestamp_str),
                )
            })
            .collect()
    }

    fn read_data_file(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.cfg.data_dir.join(rel)).ok()
    }

    fn scene_index(&self) -> Vec<SceneEntry> {
        let Some(raw) = self.read_data_file(".metadata/scene_index.json") else {
            return Vec::new();
        };
        let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|it| {
                let filename = it.get("filename")?.as_str()?.to_string();
                if filename.is_empty() {
                    return None;
                }
                Some(SceneEntry {
                    filename,
                    summary: it
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                    heat: it.get("heat").and_then(Value::as_f64).unwrap_or(0.0),
                    updated: it
                        .get("updated")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into(),
                })
            })
            .collect()
    }

    /// POST /recall
    ///
    /// Optional `scope` (slot 426, D-403-2): absent or `"all"` = legacy
    /// gateway behaviour (L1 keyword lines and the L0 text fallback span every
    /// session of the store); `"session"` = only rows whose `session_key`
    /// equals the request's. Persona and scene blocks are per session either
    /// way. A given `scope` is echoed in the reply so a client can tell an
    /// older server (which ignores the field) from one that honoured it.
    pub fn recall(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("query")) || !truthy(body.get("session_key")) {
            return err("Missing required fields: query, session_key");
        }
        let scope = match body.get("scope") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s == "all" || s == "session" => Some(s.as_str()),
            Some(_) => return err("scope must be \"session\" or \"all\""),
        };
        let query = str_of(body.get("query"));
        let session_key = str_of(body.get("session_key"));
        let only = (scope == Some("session")).then_some(session_key.as_str());

        // L1 keyword search (embedding/hybrid fall back to keyword: no embeddings).
        let clean = sanitize_text(&query);
        let lines = if js_len(&clean) >= 2 {
            self.keyword_lines(&clean, only)
        } else {
            Vec::new()
        };
        let memory_count = lines.len();

        // L3 persona, current session only.
        let persona = self.read_data_file("persona.md").and_then(|raw| {
            let p = strip_scene_navigation(&raw).trim().to_string();
            if p.is_empty() {
                return None;
            }
            let p = filter_persona_blocks_by_session(&p, &session_key);
            if p.trim().is_empty() {
                None
            } else {
                Some(p)
            }
        });

        // L2 scene navigation for this session's scene file.
        let wanted = scene_filename_for_session(&session_key);
        let scenes: Vec<SceneEntry> = self
            .scene_index()
            .into_iter()
            .filter(|e| e.filename == wanted)
            .collect();
        let nav = if scenes.is_empty() {
            None
        } else {
            Some(generate_scene_navigation(
                &scenes,
                &self.cfg.data_dir.to_string_lossy(),
            ))
        };

        let mut strategy: Option<String> = None;
        let mut context = String::new();
        if !lines.is_empty() || persona.is_some() || nav.is_some() {
            let mut stable = Vec::new();
            if let Some(p) = &persona {
                stable.push(format!("<user-persona>\n{p}\n</user-persona>"));
            }
            if let Some(n) = &nav {
                stable.push(format!("<scene-navigation>\n{n}\n</scene-navigation>"));
            }
            let append = if stable.is_empty() {
                String::new()
            } else {
                filter_recall_text_by_session(&stable.join("\n\n"), &session_key)
            };
            let prepend = if lines.is_empty() {
                String::new()
            } else {
                let p = format!(
                    "<relevant-memories>\n\u{4EE5}\u{4E0B}\u{662F}\u{5F53}\u{524D}\u{5BF9}\u{8BDD}\u{53EC}\u{56DE}\u{7684}\u{76F8}\u{5173}\u{8BB0}\u{5FC6}，\u{4E0D}\u{4EE3}\u{8868}\u{5F53}\u{524D}\u{4EFB}\u{52A1}\u{8FDB}\u{7A0B}，\u{4EC5}\u{4F5C}\u{4E3A}\u{53C2}\u{8003}：\n\n{}\n</relevant-memories>",
                    lines.join("\n")
                );
                filter_recall_text_by_session(&p, &session_key)
            };
            if !append.is_empty() || !prepend.is_empty() {
                strategy = Some(self.cfg.recall_strategy.clone());
                context = append;
                if self.cfg.recall_include_l1 && !prepend.is_empty() {
                    context = if context.is_empty() {
                        prepend
                    } else {
                        format!("{prepend}\n\n{context}")
                    };
                }
            }
        }

        // No embeddings -> always the L0 text fallback; keep exact-substring hits only.
        if let Some(store) = &self.store {
            let mut seen = std::collections::HashSet::new();
            let exact: Vec<String> = store
                .l0_text_fallback_scoped(&query, only, 8)
                .into_iter()
                .filter(|t| !t.is_empty() && seen.insert(t.clone()))
                .filter(|t| t.contains(query.as_str()))
                .collect();
            if !exact.is_empty() {
                let l0 = exact.join("\n");
                context = if context.is_empty() {
                    l0
                } else {
                    format!("{l0}\n{context}")
                };
                if js_len(&context) > 2000 {
                    context = js_slice_utf16(&context, 2000).to_string();
                }
            }
        }
        let context = filter_recall_text_by_session(&context, &session_key);

        let mut out = Map::new();
        out.insert("context".into(), json!(context));
        if let Some(s) = strategy {
            out.insert("strategy".into(), json!(s));
        }
        out.insert("memory_count".into(), json!(memory_count));
        out.insert("code".into(), json!(0));
        out.insert("message".into(), json!("ok"));
        out.insert("retryable".into(), json!(false));
        if let Some(s) = scope {
            out.insert("scope".into(), json!(s));
        }
        (200, Value::Object(out))
    }

    /// POST /capture
    pub fn capture(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("user_content"))
            || !truthy(body.get("assistant_content"))
            || !truthy(body.get("session_key"))
        {
            return err("Missing required fields: user_content, assistant_content, session_key");
        }
        let session_key = str_of(body.get("session_key"));
        let session_id = if truthy(body.get("session_id")) {
            str_of(body.get("session_id"))
        } else {
            session_key.clone()
        };
        let messages: Vec<Value> = match body.get("messages") {
            Some(Value::Array(a)) => a.clone(),
            Some(v) if !v.is_null() => Vec::new(),
            _ => vec![
                json!({"role": "user", "content": body.get("user_content")}),
                json!({"role": "assistant", "content": body.get("assistant_content")}),
            ],
        };

        // Opt-in import/replay mode: needs OPENMIND_MEMORY_ALLOW_IMPORT=1 on the
        // server AND `"import": true` in the body; otherwise the flag is ignored
        // like any unknown field (gateway parity).
        let import = self.cfg.allow_import && truthy(body.get("import"));
        let request_ms = now_ms();
        let cursor = if import {
            i64::MIN // replay keeps old timestamps; dedupe replaces the cursor
        } else {
            self.cursors
                .lock()
                .map(|m| m.get(&session_key).copied())
                .ok()
                .flatten()
                .unwrap_or(request_ms - 1)
        };

        // extractUserAssistantMessages + cursor filter + sanitize + filter.
        let mut captured: Vec<(String, String, String, i64)> = Vec::new(); // id, role, content, ts
        let mut skipped_no_ts = 0usize;
        for m in &messages {
            let role = m.get("role").and_then(Value::as_str).unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            let content = match m.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            let content = replace_data_images(&content);
            if content.trim().is_empty() {
                continue;
            }
            let given_ts = m.get("timestamp").and_then(Value::as_f64).map(|f| f as i64);
            if import && given_ts.is_none() {
                // No stable timestamp -> no stable dedupe key: refuse rather
                // than insert a row a second replay would duplicate.
                skipped_no_ts += 1;
                continue;
            }
            let ts = given_ts.unwrap_or_else(now_ms);
            if ts <= cursor {
                continue;
            }
            let mut text = sanitize_text(content.trim());
            if role == "assistant" {
                text = strip_code_blocks(&text);
            }
            if !should_capture_l0(&text) {
                continue;
            }
            let id = if import {
                import_record_id(&session_key, ts, &text)
            } else {
                m.get("id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .unwrap_or_else(|| format!("msg_{}_{}", now_ms(), rand_hex(3)))
            };
            captured.push((id, role.to_string(), text, ts));
        }

        if import {
            return self.capture_import(&session_key, &session_id, captured, skipped_no_ts);
        }

        if !captured.is_empty() && !self.cfg.read_only {
            if let Ok(mut m) = self.cursors.lock() {
                let max_ts = captured.iter().map(|c| c.3).max().unwrap_or(cursor);
                if m.len() > 10_000 {
                    m.clear();
                }
                m.insert(session_key.clone(), max_ts);
            }
            self.append_jsonl(&session_key, &session_id, &captured);
            if let Some(store) = &self.store {
                let rec_at = iso_utc(now_ms());
                for (i, (_, role, content, ts)) in captured.iter().enumerate() {
                    let r = L0Record {
                        id: format!("l0_{}_{}_{}_{}", session_key, now_ms(), i, rand_hex(3)),
                        session_key: session_key.clone(),
                        session_id: session_id.clone(),
                        role: role.clone(),
                        message_text: content.clone(),
                        recorded_at: rec_at.clone(),
                        timestamp: *ts,
                    };
                    if let Err(e) = store.upsert_l0(&r) {
                        crate::log::warn(&format!("upsertL0 failed (non-blocking): {e}"));
                    }
                }
            }
        }
        // No L1/L2/L3 extraction pipeline in v0, so nothing is scheduled.
        (
            200,
            json!({ "l0_recorded": captured.len(), "scheduler_notified": false }),
        )
    }

    /// L0 JSONL shard (same flat format as the gateway).
    fn append_jsonl(
        &self,
        session_key: &str,
        session_id: &str,
        rows: &[(String, String, String, i64)],
    ) {
        if rows.is_empty() {
            return;
        }
        let now = now_ms();
        let recorded_at = iso_utc(now);
        let mut lines = String::new();
        for (id, role, content, ts) in rows {
            let rec = json!({
                "sessionKey": session_key, "sessionId": session_id, "recordedAt": recorded_at,
                "id": id, "role": role, "content": content, "timestamp": ts,
            });
            lines.push_str(&rec.to_string());
            lines.push('\n');
        }
        let dir = self.cfg.data_dir.join("conversations");
        let _g = self.jsonl_lock.lock();
        let res = std::fs::create_dir_all(&dir).and_then(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(format!("{}.jsonl", utc_date(now))))?
                .write_all(lines.as_bytes())
        });
        if let Err(e) = res {
            crate::log::warn(&format!("L0 JSONL write failed: {e}"));
        }
    }

    /// Import mode: dedupe against the database in one transaction, then
    /// append only the rows actually inserted to the JSONL shard. The session
    /// cursor is neither read nor advanced, so live capture is unaffected.
    fn capture_import(
        &self,
        session_key: &str,
        session_id: &str,
        captured: Vec<(String, String, String, i64)>,
        skipped_no_ts: usize,
    ) -> (u16, Value) {
        let reply = |inserted: usize, dups: usize| {
            (
                200,
                json!({
                    "l0_recorded": inserted,
                    "scheduler_notified": false,
                    "import": { "inserted": inserted, "duplicates": dups, "skipped_no_timestamp": skipped_no_ts },
                }),
            )
        };
        if self.cfg.read_only {
            return reply(0, 0);
        }
        let Some(store) = &self.store else {
            return (
                503,
                json!({ "error": "import mode needs the database (store unavailable)" }),
            );
        };
        let rec_at = iso_utc(now_ms());
        let recs: Vec<L0Record> = captured
            .iter()
            .map(|(id, role, content, ts)| L0Record {
                id: id.clone(),
                session_key: session_key.to_string(),
                session_id: session_id.to_string(),
                role: role.clone(),
                message_text: content.clone(),
                recorded_at: rec_at.clone(),
                timestamp: *ts,
            })
            .collect();
        match store.import_l0(&recs) {
            Ok(out) => {
                let new: Vec<_> = out.inserted.iter().map(|&i| captured[i].clone()).collect();
                self.append_jsonl(session_key, session_id, &new);
                reply(new.len(), out.duplicates)
            }
            Err(e) => {
                crate::log::warn(&format!("import failed (nothing stored): {e}"));
                (
                    500,
                    json!({ "error": "import failed, nothing stored; safe to retry" }),
                )
            }
        }
    }

    /// POST /search/memories
    pub fn search_memories(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("query")) {
            return err("Missing required field: query");
        }
        let query = str_of(body.get("query"));
        let limit = limit_of(body.get("limit"), 5);
        let type_filter = str_of(body.get("type"));
        let scene_filter = str_of(body.get("scene")).to_lowercase();
        let reply = |text: String, total: usize, strategy: &str| {
            (
                200,
                json!({ "results": text, "total": total, "strategy": strategy }),
            )
        };
        if query.trim().is_empty() {
            return reply("No matching memories found.".into(), 0, "none");
        }
        let Some(store) = &self.store else {
            return reply("No matching memories found.".into(), 0, "none");
        };
        if !store.fts_available {
            return reply(
                "Embedding service is not configured and FTS is not available. Memory search requires an embedding provider or FTS5 support. Please configure an embedding provider in the embedding.provider setting (e.g. openai_compatible).".into(),
                0,
                "none",
            );
        }
        // Filters run before the limit over up to FILTER_SCAN ranked rows
        // (VPS rewrite semantics), instead of filtering only limit*3 rows.
        let keep = |h: &crate::store::L1Hit| {
            (type_filter.is_empty() || h.kind == type_filter)
                && (scene_filter.is_empty() || h.scene_name.to_lowercase().contains(&scene_filter))
        };
        let filtered = !type_filter.is_empty() || !scene_filter.is_empty();
        let scan = if filtered { FILTER_SCAN } else { limit };
        let items = build_fts_query(&query)
            .map(|q| store.search_l1_fts_filtered(&q, limit, scan, &keep))
            .unwrap_or_default();
        if items.is_empty() {
            return reply("No matching memories found.".into(), 0, "fts");
        }
        let mut lines = vec![
            format!("Found {} matching memories:", items.len()),
            String::new(),
        ];
        for it in &items {
            let scene = if it.scene_name.is_empty() {
                String::new()
            } else {
                format!(" [scene: {}]", it.scene_name)
            };
            let prio = if it.priority >= 0 {
                format!(" (priority: {})", it.priority)
            } else {
                " (global instruction)".into()
            };
            lines.push(format!(
                "- **[{}]**{prio}{scene} (score: {:.3})",
                it.kind, it.score
            ));
            lines.push(format!("  {}", it.content));
            lines.push(String::new());
        }
        reply(lines.join("\n"), items.len(), "fts")
    }

    /// POST /search/conversations
    pub fn search_conversations(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("query")) {
            return err("Missing required field: query");
        }
        let query = str_of(body.get("query"));
        let limit = limit_of(body.get("limit"), 5);
        let session = str_of(body.get("session_key"));
        let reply = |text: String, total: usize| (200, json!({ "results": text, "total": total }));
        let none = "No matching conversation messages found.";
        if query.trim().is_empty() {
            return reply(none.into(), 0);
        }
        let Some(store) = &self.store else {
            return reply(none.into(), 0);
        };
        if !store.fts_available {
            return reply(
                "Embedding service is not configured and FTS is not available. Conversation search requires an embedding provider or FTS5 support. Please configure an embedding provider in the embedding.provider setting (e.g. openai_compatible).".into(),
                0,
            );
        }
        // The session filter runs in SQL before the limit (VPS rewrite
        // semantics), instead of filtering only limit*4 global hits.
        let sk = (!session.is_empty()).then_some(session.as_str());
        let items = build_fts_query(&query)
            .map(|q| store.search_l0_fts_session(&q, sk, limit))
            .unwrap_or_default();
        if items.is_empty() {
            return reply(none.into(), 0);
        }
        let mut lines = vec![
            format!("Found {} matching message(s):", items.len()),
            String::new(),
        ];
        for it in &items {
            let date = if it.recorded_at.is_empty() {
                String::new()
            } else {
                // Stored UTC (JS toISOString parity, lexical order); shown in
                // Europe/Zagreb with its offset (F-354-1). Legacy text that is
                // not RFC 3339 is shown unchanged.
                let shown = zagreb_time::render_rfc3339(&it.recorded_at)
                    .unwrap_or_else(|| it.recorded_at.clone());
                format!(" [{shown}]")
            };
            lines.push("---".into());
            lines.push(format!(
                "**[{}]** Session: {}{date} (score: {:.3})",
                it.role, it.session_key, it.score
            ));
            lines.push(String::new());
            lines.push(it.message_text.clone());
            lines.push(String::new());
        }
        reply(lines.join("\n"), items.len())
    }

    /// POST /recent (2026-10-29, for the MCP `memory_recent` tool): newest
    /// L0 messages first, read-only. Body: optional `limit` (1..=50, default
    /// 10) and `session_key`. Times are rendered in Europe/Zagreb.
    pub fn recent(&self, body: &Value) -> (u16, Value) {
        let limit = limit_of(body.get("limit"), 10).clamp(1, 50);
        let session = str_of(body.get("session_key"));
        let sk = (!session.is_empty()).then_some(session.as_str());
        let Some(store) = &self.store else {
            return (200, json!({ "items": [], "total": 0 }));
        };
        let items: Vec<Value> = store
            .recent_l0(sk, limit)
            .into_iter()
            .map(|h| {
                json!({
                    "session_key": h.session_key,
                    "role": h.role,
                    "text": h.message_text,
                    "recorded_at": h.recorded_at,
                    "local_time": if h.timestamp > 0 { zagreb_time::from_unix(h.timestamp / 1000) } else { String::new() },
                })
            })
            .collect();
        let total = items.len();
        (200, json!({ "items": items, "total": total }))
    }

    /// POST /session/end — no buffered pipeline work to flush in v0.
    pub fn session_end(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("session_key")) {
            return err("Missing required field: session_key");
        }
        (200, json!({ "flushed": true }))
    }
}
