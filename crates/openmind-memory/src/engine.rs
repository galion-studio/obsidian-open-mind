// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! v1 endpoint logic: /recall, /capture, /search/memories,
//! /search/conversations, /session/end. Ported from `gateway/server.ts`,
//! `core/hooks/auto-recall.ts`, `core/hooks/auto-capture.ts`,
//! `core/conversation/l0-recorder.ts` and `core/tools/*-search.ts`.

use crate::cache::{FileCache, FileSig, Lru};
use crate::config::Config;
use crate::fts::build_fts_query;
use crate::store::{L0Record, Store};
use crate::text::*;
use crate::util::{iso_utc, now_ms, rand_hex, utc_date};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a cached /recall answer depends on besides (query, session_key):
/// DB generation, persona.md and scene_index.json versions.
type Stamp = ((u64, i64), FileSig, FileSig);

#[derive(Default)]
pub struct EngineStats {
    pub recalls: AtomicU64,
    pub recall_cache_hits: AtomicU64,
    pub captures: AtomicU64,
    pub batch_requests: AtomicU64,
    pub batch_items: AtomicU64,
    pub l0_rows_written: AtomicU64,
    pub list_requests: AtomicU64,
}

pub struct Engine {
    pub cfg: Config,
    pub store: Option<Store>,
    pub started: Instant,
    cursors: Mutex<HashMap<String, i64>>,
    jsonl_lock: Mutex<()>,
    /// persona.md minus scene navigation, trimmed (None when empty).
    persona: FileCache<Option<Arc<String>>>,
    /// Parsed .metadata/scene_index.json.
    scenes: FileCache<Arc<Vec<SceneEntry>>>,
    /// Session-filtered persona, keyed by (session, persona version).
    persona_by_session: Lru<(String, FileSig), Option<Arc<String>>>,
    recall_cache: Lru<(String, String), (Stamp, Value)>,
    pub stats: EngineStats,
}

/// (session_key filter, cursor, limit) of a list request.
type PageArgs = (Option<String>, Option<String>, usize);

/// One /capture body after validation and filtering, before any write.
struct CapturePlan {
    session_key: String,
    session_id: String,
    captured: Vec<(String, String, String, i64)>, // id, role, content, ts
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
            match Store::open_with(&path, cfg.read_only, cfg.readers) {
                Ok(s) => Some(s),
                Err(e) => {
                    crate::log::warn(&format!("store open failed ({}): {e}", path.display()));
                    None
                }
            }
        };
        let ttl = Some(Duration::from_millis(cfg.recall_cache_ttl_ms));
        Engine {
            persona: FileCache::new(cfg.data_dir.join("persona.md")),
            scenes: FileCache::new(cfg.data_dir.join(".metadata/scene_index.json")),
            persona_by_session: Lru::new(64, None),
            recall_cache: Lru::new(cfg.recall_cache, ttl),
            stats: EngineStats::default(),
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

    /// GET /stats (authenticated): counters, cache and pool figures. No content.
    pub fn stats_json(&self) -> Value {
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let st = &self.stats;
        let rows = self.store.as_ref().map(|s| {
            json!({
                "l0_conversations": s.count("l0_conversations"),
                "l1_records": s.count("l1_records"),
            })
        });
        json!({
            "implementation": "openmind-memory",
            "implementation_version": env!("CARGO_PKG_VERSION"),
            "uptime": self.started.elapsed().as_secs(),
            "read_only": self.cfg.read_only,
            "readers": self.store.as_ref().map(|s| s.readers()).unwrap_or(0),
            "rows": rows,
            "requests": crate::http::metrics_json(),
            "engine": {
                "recalls": g(&st.recalls),
                "recall_cache_hits": g(&st.recall_cache_hits),
                "captures": g(&st.captures),
                "batch_requests": g(&st.batch_requests),
                "batch_items": g(&st.batch_items),
                "l0_rows_written": g(&st.l0_rows_written),
                "list_requests": g(&st.list_requests),
            },
            "caches": {
                "recall": {
                    "cap": self.recall_cache.cap(), "len": self.recall_cache.len(),
                    "ttl_ms": self.cfg.recall_cache_ttl_ms,
                    "hits": g(&self.recall_cache.hits), "misses": g(&self.recall_cache.misses),
                    "evictions": g(&self.recall_cache.evictions),
                },
                "persona_by_session": {
                    "cap": self.persona_by_session.cap(), "len": self.persona_by_session.len(),
                    "hits": g(&self.persona_by_session.hits), "misses": g(&self.persona_by_session.misses),
                },
                "file_reloads": { "persona": g(&self.persona.reloads), "scene_index": g(&self.scenes.reloads) },
            },
        })
    }
    /// `searchByKeyword` (auto-recall): formatted L1 lines.
    fn keyword_lines(&self, clean: &str) -> Vec<String> {
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
        let hits = store.search_l1_fts(&q, max * 2);
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

    fn scene_index(&self) -> Arc<Vec<SceneEntry>> {
        self.scenes.get(|raw| {
            let Some(raw) = raw else {
                return Arc::new(Vec::new());
            };
            let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) else {
                return Arc::new(Vec::new());
            };
            Arc::new(
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
                    .collect(),
            )
        })
    }

    /// L3 persona for one session (cached per persona version).
    fn persona_for(&self, session_key: &str) -> Option<Arc<String>> {
        let key = (session_key.to_string(), self.persona.sig());
        if let Some(v) = self.persona_by_session.get(&key) {
            return v;
        }
        let base = self.persona.get(|raw| {
            let p = strip_scene_navigation(&raw?).trim().to_string();
            (!p.is_empty()).then(|| Arc::new(p))
        });
        let v = base.and_then(|p| {
            let f = filter_persona_blocks_by_session(&p, session_key);
            (!f.trim().is_empty()).then(|| Arc::new(f))
        });
        self.persona_by_session.insert(key, v.clone());
        v
    }

    /// Cache stamp; None when the writer is busy (then the cache is skipped).
    fn stamp(&self) -> Option<Stamp> {
        let gen = match &self.store {
            Some(s) => s.try_generation()?,
            None => (0, 0),
        };
        Some((gen, self.persona.sig(), self.scenes.sig()))
    }

    /// POST /recall (bounded result cache in front of [`Engine::recall_uncached`]).
    pub fn recall(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("query")) || !truthy(body.get("session_key")) {
            return err("Missing required fields: query, session_key");
        }
        self.stats.recalls.fetch_add(1, Ordering::Relaxed);
        let key = (str_of(body.get("query")), str_of(body.get("session_key")));
        let stamp = if self.recall_cache.cap() > 0 {
            self.stamp()
        } else {
            None
        };
        if let Some(st) = &stamp {
            if let Some((s, v)) = self.recall_cache.get(&key) {
                if s == *st {
                    self.stats.recall_cache_hits.fetch_add(1, Ordering::Relaxed);
                    return (200, v);
                }
            }
        }
        let (code, v) = self.recall_uncached(&key.0, &key.1);
        if let (Some(st), 200) = (stamp, code) {
            self.recall_cache.insert(key, (st, v.clone()));
        }
        (code, v)
    }

    fn recall_uncached(&self, query: &str, session_key: &str) -> (u16, Value) {
        let query = query.to_string();
        let session_key = session_key.to_string();

        // L1 keyword search (embedding/hybrid fall back to keyword: no embeddings).
        let clean = sanitize_text(&query);
        let lines = if js_len(&clean) >= 2 {
            self.keyword_lines(&clean)
        } else {
            Vec::new()
        };
        let memory_count = lines.len();

        // L3 persona, current session only.
        let persona = self.persona_for(&session_key);

        // L2 scene navigation for this session's scene file.
        let wanted = scene_filename_for_session(&session_key);
        let scenes: Vec<SceneEntry> = self
            .scene_index()
            .iter()
            .filter(|e| e.filename == wanted)
            .cloned()
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
                .l0_text_fallback(&query, 8)
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
        (200, Value::Object(out))
    }

    /// Validate + filter one /capture body (no writes). Uses and does not
    /// advance the per-session cursor.
    fn capture_plan(&self, body: &Value) -> Result<CapturePlan, (u16, Value)> {
        if !truthy(body.get("user_content"))
            || !truthy(body.get("assistant_content"))
            || !truthy(body.get("session_key"))
        {
            return Err(err(
                "Missing required fields: user_content, assistant_content, session_key",
            ));
        }
        let session_key = str_of(body.get("session_key"));
        let session_id = if truthy(body.get("session_id")) {
            str_of(body.get("session_id"))
        } else {
            session_key.clone()
        };
        let fallback;
        let messages: &[Value] = match body.get("messages") {
            Some(Value::Array(a)) => a,
            Some(v) if !v.is_null() => &[],
            _ => {
                fallback = [
                    json!({"role": "user", "content": body.get("user_content")}),
                    json!({"role": "assistant", "content": body.get("assistant_content")}),
                ];
                &fallback
            }
        };

        let request_ms = now_ms();
        let cursor = self
            .cursors
            .lock()
            .map(|m| m.get(&session_key).copied())
            .ok()
            .flatten()
            .unwrap_or(request_ms - 1);

        // extractUserAssistantMessages + cursor filter + sanitize + filter.
        let mut captured: Vec<(String, String, String, i64)> = Vec::new();
        for m in messages {
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
            let ts = m
                .get("timestamp")
                .and_then(Value::as_f64)
                .map(|f| f as i64)
                .unwrap_or_else(now_ms);
            if ts <= cursor {
                continue;
            }
            let id = m
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or_else(|| format!("msg_{}_{}", now_ms(), rand_hex(3)));
            let mut text = sanitize_text(content.trim());
            if role == "assistant" {
                text = strip_code_blocks(&text);
            }
            if should_capture_l0(&text) {
                captured.push((id, role.to_string(), text, ts));
            }
        }
        Ok(CapturePlan {
            session_key,
            session_id,
            captured,
        })
    }

    fn advance_cursor(&self, p: &CapturePlan) {
        if p.captured.is_empty() || self.cfg.read_only {
            return;
        }
        if let Ok(mut m) = self.cursors.lock() {
            let max_ts = p.captured.iter().map(|c| c.3).max().unwrap_or(0);
            if m.len() > 10_000 {
                m.clear();
            }
            m.insert(p.session_key.clone(), max_ts);
        }
    }

    /// Write planned captures: one JSONL append and ONE SQLite transaction
    /// for all of them.
    fn capture_commit(&self, plans: &[CapturePlan]) {
        if self.cfg.read_only || plans.iter().all(|p| p.captured.is_empty()) {
            return;
        }
        // L0 JSONL shard (same flat format as the gateway).
        let now = now_ms();
        let recorded_at = iso_utc(now);
        let mut lines = String::new();
        for p in plans {
            for (id, role, content, ts) in &p.captured {
                let rec = json!({
                    "sessionKey": p.session_key, "sessionId": p.session_id, "recordedAt": recorded_at,
                    "id": id, "role": role, "content": content, "timestamp": ts,
                });
                lines.push_str(&rec.to_string());
                lines.push('\n');
            }
        }
        let dir = self.cfg.data_dir.join("conversations");
        {
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
        if let Some(store) = &self.store {
            let rec_at = iso_utc(now_ms());
            let mut rows = Vec::new();
            for p in plans {
                for (i, (_, role, content, ts)) in p.captured.iter().enumerate() {
                    rows.push(L0Record {
                        id: format!("l0_{}_{}_{}_{}", p.session_key, now_ms(), i, rand_hex(3)),
                        session_key: p.session_key.clone(),
                        session_id: p.session_id.clone(),
                        role: role.clone(),
                        message_text: content.clone(),
                        recorded_at: rec_at.clone(),
                        timestamp: *ts,
                    });
                }
            }
            match store.upsert_l0_batch(&rows) {
                Ok(n) => {
                    self.stats
                        .l0_rows_written
                        .fetch_add(n as u64, Ordering::Relaxed);
                }
                Err(e) => crate::log::warn(&format!("upsertL0 failed (non-blocking): {e}")),
            }
        }
    }

    /// POST /capture
    pub fn capture(&self, body: &Value) -> (u16, Value) {
        let plan = match self.capture_plan(body) {
            Ok(p) => p,
            Err(e) => return e,
        };
        self.stats.captures.fetch_add(1, Ordering::Relaxed);
        self.advance_cursor(&plan);
        let n = plan.captured.len();
        self.capture_commit(std::slice::from_ref(&plan));
        // No L1/L2/L3 extraction pipeline in v0, so nothing is scheduled.
        (
            200,
            json!({ "l0_recorded": n, "scheduler_notified": false }),
        )
    }

    /// POST /capture/batch `{items:[<capture body>...]}` (new in growth):
    /// every valid item is written in one transaction; invalid items get a
    /// per-item error and do not fail the batch.
    pub fn capture_batch(&self, body: &Value) -> (u16, Value) {
        let Some(items) = body.get("items").and_then(Value::as_array) else {
            return err("Missing required field: items (array of capture bodies)");
        };
        if items.len() > self.cfg.batch_max {
            return (
                413,
                json!({ "error": format!("Too many items: {} > {}", items.len(), self.cfg.batch_max) }),
            );
        }
        self.stats.batch_requests.fetch_add(1, Ordering::Relaxed);
        self.stats
            .batch_items
            .fetch_add(items.len() as u64, Ordering::Relaxed);
        let mut plans = Vec::with_capacity(items.len());
        let mut results = Vec::with_capacity(items.len());
        for (i, it) in items.iter().enumerate() {
            match self.capture_plan(it) {
                Ok(p) => {
                    self.advance_cursor(&p);
                    results.push(json!({ "index": i, "l0_recorded": p.captured.len(), "scheduler_notified": false }));
                    plans.push(p);
                }
                Err((code, e)) => {
                    results.push(json!({ "index": i, "status": code, "error": e["error"] }))
                }
            }
        }
        let total: usize = plans.iter().map(|p| p.captured.len()).sum();
        self.capture_commit(&plans);
        (
            200,
            json!({
                "code": 0, "message": "ok", "count": items.len(), "accepted": plans.len(),
                "l0_recorded": total, "results": results,
            }),
        )
    }

    fn page_args(body: &Value) -> Result<PageArgs, (u16, Value)> {
        let session = Some(str_of(body.get("session_key"))).filter(|s| !s.is_empty());
        let cursor = match body.get("cursor") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.is_empty() => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(err("cursor must be a string")),
        };
        Ok((
            session,
            cursor,
            limit_of(body.get("limit"), 50).clamp(1, 500),
        ))
    }

    /// POST /conversations/list `{session_key?, limit?, cursor?}` (new):
    /// newest-first L0 rows, keyset-paginated.
    pub fn list_conversations(&self, body: &Value) -> (u16, Value) {
        self.stats.list_requests.fetch_add(1, Ordering::Relaxed);
        let (session, cursor, limit) = match Self::page_args(body) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let Some(store) = &self.store else {
            return (
                200,
                json!({ "items": [], "count": 0, "next_cursor": null, "has_more": false }),
            );
        };
        match store.list_l0(session.as_deref(), cursor.as_deref(), limit) {
            Ok(p) => {
                let items: Vec<Value> = p
                    .items
                    .iter()
                    .map(|r| {
                        json!({
                            "record_id": r.record_id, "session_key": r.session_key, "session_id": r.session_id,
                            "role": r.role, "message_text": r.message_text, "recorded_at": r.recorded_at,
                            "timestamp": r.timestamp,
                        })
                    })
                    .collect();
                (
                    200,
                    json!({ "items": items, "count": p.items.len(), "has_more": p.next.is_some(), "next_cursor": p.next }),
                )
            }
            Err(e) => err(&e),
        }
    }

    /// POST /memories/list `{session_key?, limit?, cursor?}` (new):
    /// newest-first L1 records, keyset-paginated.
    pub fn list_memories(&self, body: &Value) -> (u16, Value) {
        self.stats.list_requests.fetch_add(1, Ordering::Relaxed);
        let (session, cursor, limit) = match Self::page_args(body) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let Some(store) = &self.store else {
            return (
                200,
                json!({ "items": [], "count": 0, "next_cursor": null, "has_more": false }),
            );
        };
        match store.list_l1(session.as_deref(), cursor.as_deref(), limit) {
            Ok(p) => {
                let items: Vec<Value> = p
                    .items
                    .iter()
                    .map(|r| {
                        json!({
                            "record_id": r.record_id, "content": r.content, "type": r.kind,
                            "priority": r.priority, "scene_name": r.scene_name, "session_key": r.session_key,
                            "session_id": r.session_id, "timestamp_str": r.timestamp_str, "updated_time": r.updated_time,
                        })
                    })
                    .collect();
                (
                    200,
                    json!({ "items": items, "count": p.items.len(), "has_more": p.next.is_some(), "next_cursor": p.next }),
                )
            }
            Err(e) => err(&e),
        }
    }

    /// POST /search/memories
    pub fn search_memories(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("query")) {
            return err("Missing required field: query");
        }
        let query = str_of(body.get("query"));
        let limit = limit_of(body.get("limit"), 5);
        let offset = body.get("offset").map(|v| limit_of(Some(v), 0));
        let off = offset.unwrap_or(0);
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
        let hits = build_fts_query(&query)
            .map(|q| store.search_l1_fts(&q, (limit + off) * 3))
            .unwrap_or_default();
        if hits.is_empty() {
            return reply("No matching memories found.".into(), 0, "fts");
        }
        let items: Vec<_> = hits
            .into_iter()
            .filter(|h| type_filter.is_empty() || h.kind == type_filter)
            .filter(|h| {
                scene_filter.is_empty() || h.scene_name.to_lowercase().contains(&scene_filter)
            })
            .skip(off)
            .take(limit + 1)
            .collect();
        let has_more = items.len() > limit;
        let mut items = items;
        items.truncate(limit);
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
        with_page(
            reply(lines.join("\n"), items.len(), "fts"),
            offset,
            has_more,
        )
    }

    /// POST /search/conversations
    pub fn search_conversations(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("query")) {
            return err("Missing required field: query");
        }
        let query = str_of(body.get("query"));
        let limit = limit_of(body.get("limit"), 5);
        let offset = body.get("offset").map(|v| limit_of(Some(v), 0));
        let off = offset.unwrap_or(0);
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
        let k = if session.is_empty() {
            (limit + off) * 3
        } else {
            (limit + off) * 4
        };
        let hits = build_fts_query(&query)
            .map(|q| store.search_l0_fts(&q, k))
            .unwrap_or_default();
        let items: Vec<_> = hits
            .into_iter()
            .filter(|h| session.is_empty() || h.session_key == session)
            .skip(off)
            .take(limit + 1)
            .collect();
        let has_more = items.len() > limit;
        let mut items = items;
        items.truncate(limit);
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
                format!(" [{}]", it.recorded_at)
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
        with_page(reply(lines.join("\n"), items.len()), offset, has_more)
    }

    /// POST /session/end — no buffered pipeline work to flush in v0.
    pub fn session_end(&self, body: &Value) -> (u16, Value) {
        if !truthy(body.get("session_key")) {
            return err("Missing required field: session_key");
        }
        (200, json!({ "flushed": true }))
    }
}

/// Search pagination (growth): only when the client sent `offset`, add
/// `offset` and `next_offset` (null at the end). Old clients see no change.
fn with_page(r: (u16, Value), offset: Option<usize>, has_more: bool) -> (u16, Value) {
    let (code, mut v) = r;
    if let (Some(off), Some(obj)) = (offset, v.as_object_mut()) {
        let total = obj.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
        obj.insert("offset".into(), json!(off));
        obj.insert(
            "next_offset".into(),
            if has_more {
                json!(off + total)
            } else {
                Value::Null
            },
        );
    }
    (code, v)
}
