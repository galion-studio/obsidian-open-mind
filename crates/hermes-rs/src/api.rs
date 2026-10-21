// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The OpenAI-compatible HTTP API (the `api_server` platform of upstream
//! hermes-agent): /health, /v1/models, /v1/chat/completions (incl. SSE),
//! /v1/runs*, /api/sessions*, /api/jobs (read-only).

use crate::agent::{Agent, Turn, TurnResult};
use crate::config::Config;
use crate::http::{error, Request, Response, SseWriter};
use crate::persona::{channel_for_key, PromptParts};
use crate::provider::{is_generic_model, resolve_reasoning, resolve_target, Target};
use crate::session::{SessionRow, SessionStore};
use crate::tools::Event;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const SOURCE: &str = "api_server";
pub const MAX_SESSION_HEADER_LEN: usize = 256;

pub struct Gateway {
    pub cfg: Arc<Config>,
    pub agent: Agent,
    pub store: Arc<dyn SessionStore>,
    api_key: Option<String>,
    pub active: Arc<AtomicUsize>,
    pub runs: crate::runs::Runs,
    pub jobs: Option<Arc<crate::cron::Scheduler>>,
    /// Device enrollment (`/openmind/*`); `None` = disabled, every such path is 404.
    pub enroll: Option<Arc<crate::enroll::Enroll>>,
}

/// Holds one agent slot (`gateway.api_server.max_concurrent_runs`).
pub struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Flatten OpenAI content parts to text.
pub fn content_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text" || p["type"] == "input_text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// What a chat request resolves to before the agent runs.
pub struct Prepared {
    pub row: SessionRow,
    pub history: Vec<Value>,
    pub persist_history: Vec<Value>,
    pub user_text: String,
    pub target: Target,
    pub turn: Turn,
}

impl Gateway {
    pub fn new(
        cfg: Arc<Config>,
        agent: Agent,
        store: Arc<dyn SessionStore>,
        api_key: Option<String>,
    ) -> Self {
        Gateway {
            cfg,
            agent,
            store,
            api_key,
            active: Arc::new(AtomicUsize::new(0)),
            runs: crate::runs::Runs::default(),
            jobs: None,
            enroll: None,
        }
    }

    /// Upstream startup guard: a key of at least 16 chars is required.
    pub fn key_ok(&self) -> bool {
        self.api_key.as_deref().is_some_and(|k| k.len() >= 16)
    }

    fn check_auth(&self, req: &Request) -> Option<Response> {
        let expected = self.api_key.as_deref()?;
        let got = req.header("authorization").unwrap_or("");
        if let Some(tok) = got.strip_prefix("Bearer ") {
            if ct_eq(tok.trim().as_bytes(), expected.as_bytes()) {
                return None;
            }
        }
        Some(Response::json(
            401,
            &json!({"error": {"message": "Invalid gateway API key (API_SERVER_KEY)", "type": "gateway_auth_error", "code": "gateway_auth_failed"}}),
        ))
    }

    pub fn try_slot(&self) -> Option<Slot> {
        let n = self.active.fetch_add(1, Ordering::SeqCst);
        if n >= self.cfg.max_concurrent_runs.max(1) {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(self.active.clone()))
    }

    pub fn handle(self: &Arc<Self>, mut req: Request) -> Response {
        // multiplex mirror: /p/<profile>/<path>. Only the default profile here.
        if let Some(rest) = req.path.strip_prefix("/p/") {
            let (profile, tail) = rest.split_once('/').unwrap_or((rest, ""));
            if profile != "default" {
                return error(
                    404,
                    &format!("Unknown profile: {profile}"),
                    "invalid_request_error",
                    Some("profile_not_found"),
                );
            }
            req.path = format!("/{tail}");
        }
        let path = req.path.clone();
        let m = req.method.as_str();
        if path == "/openmind" || path.starts_with("/openmind/") {
            let Some(e) = &self.enroll else {
                return error(
                    404,
                    &format!("Not found: {} {}", m, path),
                    "invalid_request_error",
                    Some("not_found"),
                );
            };
            // Owner routes need the gateway key; fail closed when none is set.
            let owner = self.key_ok() && self.check_auth(&req).is_none();
            return e.handle(&req, owner, crate::time::now_secs());
        }
        if m == "GET" && (path == "/health" || path == "/v1/health") {
            return Response::json(
                200,
                &json!({"status": "ok", "platform": "hermes-agent", "version": crate::UPSTREAM_COMPAT, "implementation": "hermes-rs", "implementation_version": crate::VERSION}),
            );
        }
        if let Some(r) = self.check_auth(&req) {
            return r;
        }
        let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
        match (m, seg.as_slice()) {
            ("GET", ["health", "detailed"]) => self.health_detailed(),
            ("GET", ["v1", "models"]) => self.models(),
            ("GET", ["v1", "capabilities"]) => Response::json(
                200,
                &json!({"object": "hermes.api_server.capabilities", "platform": "hermes-agent", "implementation": "hermes-rs",
                        "endpoints": ["/v1/chat/completions", "/v1/models", "/v1/runs", "/api/sessions", "/api/jobs"],
                        "streaming": true, "runs": true, "responses_api": false}),
            ),
            ("POST", ["v1", "chat", "completions"]) => self.chat(&req),
            ("POST", ["v1", "runs"]) => crate::runs::start(self, &req),
            ("GET", ["v1", "runs", id]) => crate::runs::get(self, id),
            ("GET", ["v1", "runs", id, "events"]) => crate::runs::events(self, id),
            ("POST", ["v1", "runs", id, "stop"]) => crate::runs::stop(self, id),
            ("POST", ["v1", "runs", id, "steer"]) => crate::runs::steer(self, id, &req),
            ("POST", ["v1", "runs", id, "approval"]) => crate::runs::approval(self, id),
            ("GET", ["api", "sessions"]) => self.list_sessions(&req),
            ("GET", ["api", "sessions", id]) => self.get_session(id),
            ("GET", ["api", "sessions", id, "messages"]) => self.session_messages(id),
            ("POST", ["api", "sessions", id, "model"]) => self.lock_model(id, &req),
            ("GET", ["api", "jobs"]) => match &self.jobs {
                Some(s) => Response::json(200, &json!({"jobs": s.list()})),
                None => Response::json(200, &json!({"jobs": []})),
            },
            ("GET", ["api", "spend"]) => Response::json(200, &self.agent.spend.status()),
            _ => error(
                404,
                &format!("Not found: {} {}", m, path),
                "invalid_request_error",
                Some("not_found"),
            ),
        }
    }

    fn health_detailed(&self) -> Response {
        Response::json(
            200,
            &json!({"status": "ok", "platform": "hermes-agent", "version": crate::UPSTREAM_COMPAT, "implementation": "hermes-rs",
                    "gateway_state": "running", "active_agents": self.active.load(Ordering::SeqCst),
                    "max_concurrent_runs": self.cfg.max_concurrent_runs, "platforms": {"api_server": {"state": "connected"}},
                    "spend": self.agent.spend.status(), "pid": std::process::id()}),
        )
    }

    fn models(&self) -> Response {
        let now = crate::time::now_secs();
        let root = "hermes-agent";
        let mut data = vec![
            json!({"id": root, "object": "model", "created": now, "owned_by": "hermes", "permission": [], "root": root, "parent": null}),
        ];
        for (alias, r) in &self.cfg.model_routes {
            if alias != root {
                data.push(json!({"id": alias, "object": "model", "created": now, "owned_by": "hermes", "permission": [], "root": r.model, "parent": root}));
            }
        }
        Response::json(200, &json!({"object": "list", "data": data}))
    }

    fn header_id(req: &Request, name: &str) -> Result<Option<String>, Response> {
        match req.header(name).map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(None),
            Some(v) if v.len() > MAX_SESSION_HEADER_LEN || v.chars().any(|c| c.is_control()) => {
                Err(error(
                    400,
                    &format!("{name} is too long or has control characters"),
                    "invalid_request_error",
                    Some("invalid_session_header"),
                ))
            }
            Some(v) => Ok(Some(v.to_string())),
        }
    }

    /// Resolve session, history, model pin, prompt and the agent turn.
    pub fn prepare(
        &self,
        session_id: Option<String>,
        session_key: Option<String>,
        body: &Value,
        body_messages: &[Value],
        extra_instructions: Option<&str>,
    ) -> Result<Prepared, Response> {
        let mut client_system = Vec::new();
        let mut convo = Vec::new();
        for m in body_messages {
            let role = m["role"].as_str().unwrap_or("");
            match role {
                "system" | "developer" => client_system.push(content_text(&m["content"])),
                "user" | "assistant" | "tool" => {
                    let mut w = crate::session::wire_message(m);
                    if m["content"].is_array() {
                        w["content"] = json!(content_text(&m["content"]));
                    }
                    convo.push(w)
                }
                _ => {}
            }
        }
        let Some(last_user) = convo.iter().rposition(|m| m["role"] == "user") else {
            return Err(error(
                400,
                "No user message found in messages",
                "invalid_request_error",
                None,
            ));
        };
        let user_text = convo[last_user]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let body_history: Vec<Value> = convo[..last_user].to_vec();

        let existing = match (&session_id, &session_key) {
            (Some(id), _) => self.store.get(id),
            (None, Some(k)) => self.store.find_by_key(SOURCE, k),
            _ => None,
        };
        let mut row = existing.clone().unwrap_or_else(|| {
            let id = session_id.clone().unwrap_or_else(|| crate::new_id("api-"));
            SessionRow::new(&id, SOURCE, session_key.as_deref())
        });
        if row.session_key.is_none() {
            row.session_key = session_key.clone();
        }
        let stored = if existing.is_some() {
            self.store.messages(&row.id)
        } else {
            Vec::new()
        };
        // Galion patch: the stored transcript wins only when it is at least as long.
        let (history, persist_history) = if !stored.is_empty() && stored.len() >= body_history.len()
        {
            (stored, Vec::new())
        } else {
            (
                body_history.clone(),
                if stored.is_empty() {
                    body_history
                } else {
                    Vec::new()
                },
            )
        };

        // Model pinning (Galion patch on api_server.py): a stored session
        // model applies only when this turn named no model and no provider.
        let req_model = body["model"].as_str().filter(|m| !is_generic_model(m));
        let req_provider = body["provider"].as_str().filter(|p| !p.trim().is_empty());
        let target = if req_model.is_some() || req_provider.is_some() {
            resolve_target(&self.cfg, req_model, req_provider)
        } else if let Some(pinned) = row.model.as_deref() {
            resolve_target(&self.cfg, Some(pinned), row.provider.as_deref())
        } else {
            resolve_target(&self.cfg, None, None)
        };
        if row.model.is_none() {
            row.model = Some(target.model.clone());
            row.provider = Some(target.provider.clone());
        }
        let rq = body["model_options"]
            .get("reasoning")
            .or_else(|| body.get("reasoning_effort"))
            .or_else(|| body.get("reasoning"));
        let reasoning = resolve_reasoning(&self.cfg, &target.model, rq, false);

        let channel = channel_for_key(session_key.as_deref());
        let persona = crate::persona::persona_prompt(
            self.cfg.persona_file.as_deref(),
            channel.as_deref(),
            &row.id,
        );
        // A persona channel drops the global `agent.system_prompt` (its length rule conflicts).
        let cfg_prompt = if persona.is_some() {
            None
        } else {
            self.cfg.system_prompt.as_deref()
        };
        let mut parts = PromptParts::load(&self.cfg.home, channel.as_deref(), cfg_prompt);
        parts.persona = persona;
        if let Some(i) = extra_instructions {
            parts.client.push(i.to_string());
        }
        parts.client.extend(client_system);

        let mut messages = history.clone();
        messages.push(json!({"role": "user", "content": user_text}));
        let max_tokens = body["max_tokens"]
            .as_u64()
            .or_else(|| body["max_completion_tokens"].as_u64());
        let turn = Turn {
            session_id: row.id.clone(),
            spend_session: row.id.clone(),
            target: target.clone(),
            reasoning,
            system: parts.build(),
            messages,
            max_tokens,
            depth: 0,
            max_iterations: self.cfg.max_turns,
            cancel: Arc::new(AtomicBool::new(false)),
            steer: Arc::new(Mutex::new(Vec::new())),
        };
        Ok(Prepared {
            row,
            history,
            persist_history,
            user_text,
            target,
            turn,
        })
    }

    /// Store the turn: any body history the store lacked, the user message,
    /// the assistant/tool messages, usage.
    pub fn persist(&self, p: &Prepared, res: &TurnResult) {
        let mut row = p.row.clone();
        row.input_tokens += res.usage.prompt_tokens;
        row.output_tokens += res.usage.completion_tokens;
        row.cost_usd += res.usage.cost_usd;
        if row.title.is_none() {
            row.title = Some(p.user_text.chars().take(60).collect());
        }
        if self.store.upsert(&row).is_err() {
            return;
        }
        let mut msgs = p.persist_history.clone();
        msgs.push(json!({"role": "user", "content": p.user_text}));
        msgs.extend(res.added.iter().cloned());
        let _ = self.store.append(&row.id, &msgs);
    }

    fn chat(self: &Arc<Self>, req: &Request) -> Response {
        let body = match req.json() {
            Ok(b) => b,
            Err(e) => return error(400, &e, "invalid_request_error", None),
        };
        let Some(msgs) = body["messages"].as_array().filter(|a| !a.is_empty()) else {
            return error(
                400,
                "Missing or empty 'messages'",
                "invalid_request_error",
                Some("messages"),
            );
        };
        let sid = match Self::header_id(req, "x-hermes-session-id") {
            Ok(v) => v,
            Err(r) => return r,
        };
        let key = match Self::header_id(req, "x-hermes-session-key") {
            Ok(v) => v,
            Err(r) => return r,
        };
        let prepared = match self.prepare(sid.clone(), key.clone(), &body, msgs, None) {
            Ok(p) => p,
            Err(r) => return r,
        };
        let Some(slot) = self.try_slot() else {
            return error(
                429,
                "too many concurrent runs",
                "rate_limit_error",
                Some("max_concurrent_runs"),
            );
        };
        let completion_id = format!("chatcmpl-{}", &crate::new_id("")[..16]);
        let created = crate::time::now_secs();
        let model_name = body["model"].as_str().unwrap_or("hermes-agent").to_string();
        let echo_sid = sid.unwrap_or_else(|| prepared.row.id.clone());
        let stream = body["stream"].as_bool().unwrap_or(false);
        if !stream {
            let res = self.agent.run(clone_turn(&prepared.turn), &|_e| {});
            drop(slot);
            self.persist(&prepared, &res);
            return chat_json(
                &completion_id,
                created,
                &model_name,
                &res,
                &echo_sid,
                key.as_deref(),
            );
        }
        let gw = self.clone();
        let mut resp = Response::Stream {
            status: 200,
            headers: vec![("X-Hermes-Session-Id".into(), echo_sid.clone())],
            writer: Box::new(move |w: &mut SseWriter| {
                let _slot = slot;
                let chunk = |delta: Value, finish: Value| {
                    json!({"id": completion_id, "object": "chat.completion.chunk", "created": created, "model": model_name,
                           "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
                };
                w.frame(None, &chunk(json!({"role": "assistant"}), Value::Null));
                let cancel = prepared.turn.cancel.clone();
                let out = Mutex::new(w);
                let emit = |e: Event| {
                    let mut w = match out.lock() {
                        Ok(w) => w,
                        Err(p) => p.into_inner(),
                    };
                    let ok = match &e {
                        Event::Delta(d) => w.frame(None, &chunk(json!({"content": d}), Value::Null)),
                        Event::Reasoning(r) => w.frame(None, &chunk(json!({"reasoning_content": r}), Value::Null)),
                        Event::ToolStarted { tool, preview } => {
                            w.frame(Some("hermes.tool.progress"), &json!({"tool": tool, "status": "running", "preview": preview}))
                        }
                        Event::ToolCompleted { tool, duration, error, .. } => w.frame(
                            Some("hermes.tool.progress"),
                            &json!({"tool": tool, "status": "completed", "duration": duration, "error": error}),
                        ),
                        Event::Status(s) => w.frame(Some("hermes.status"), &json!({"status": s})),
                    };
                    if !ok {
                        cancel.store(true, Ordering::SeqCst);
                    }
                };
                let res = gw.agent.run(clone_turn(&prepared.turn), &emit);
                gw.persist(&prepared, &res);
                let w = match out.into_inner() {
                    Ok(w) => w,
                    Err(p) => p.into_inner(),
                };
                let mut fin = chunk(json!({}), json!(res.finish_reason_wire()));
                fin["usage"] = usage_json(&res);
                if res.finish_reason_wire() != "stop" {
                    if let Some(e) = &res.error {
                        fin["error"] = json!({"message": e, "type": "agent_error"});
                    }
                    fin["hermes"] = extras(&res);
                }
                w.frame(None, &fin);
                w.raw(b"data: [DONE]\n\n");
            }),
        };
        if let Some(k) = key {
            resp = resp.with_header("X-Hermes-Session-Key", &k);
        }
        resp
    }

    fn list_sessions(&self, req: &Request) -> Response {
        let limit = req
            .query_param("limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(50usize)
            .min(500);
        let rows: Vec<Value> = self
            .store
            .list(limit)
            .iter()
            .map(SessionRow::to_json)
            .collect();
        Response::json(200, &json!({"object": "list", "data": rows}))
    }

    fn get_session(&self, id: &str) -> Response {
        match self.store.get(id) {
            Some(r) => Response::json(200, &r.to_json()),
            None => error(
                404,
                &format!("Session not found: {id}"),
                "invalid_request_error",
                Some("session_not_found"),
            ),
        }
    }

    fn session_messages(&self, id: &str) -> Response {
        if self.store.get(id).is_none() {
            return error(
                404,
                &format!("Session not found: {id}"),
                "invalid_request_error",
                Some("session_not_found"),
            );
        }
        let cap = self.cfg.history_tool_output_max_chars;
        let msgs: Vec<Value> = self
            .store
            .messages(id)
            .into_iter()
            .map(|mut m| {
                if m["role"] == "tool" {
                    if let Some(s) = m["content"].as_str() {
                        if s.chars().count() > cap {
                            m["content"] = json!(format!(
                                "{}...[truncated]",
                                s.chars().take(cap).collect::<String>()
                            ));
                        }
                    }
                }
                m
            })
            .collect();
        Response::json(
            200,
            &json!({"object": "list", "session_id": id, "data": msgs}),
        )
    }

    fn lock_model(&self, id: &str, req: &Request) -> Response {
        let body = match req.json() {
            Ok(b) => b,
            Err(e) => return error(400, &e, "invalid_request_error", None),
        };
        let Some(mut row) = self.store.get(id) else {
            return error(
                404,
                &format!("Session not found: {id}"),
                "invalid_request_error",
                Some("session_not_found"),
            );
        };
        match body["model"].as_str().filter(|m| !m.trim().is_empty()) {
            Some(m) => {
                let t = resolve_target(&self.cfg, Some(m), body["provider"].as_str());
                row.model = Some(t.model.clone());
                row.provider = Some(t.provider.clone());
            }
            None => {
                row.model = None;
                row.provider = None;
            }
        }
        if let Err(e) = self.store.upsert(&row) {
            return error(500, &e, "server_error", None);
        }
        Response::json(
            200,
            &json!({"session_id": id, "model": row.model, "provider": row.provider}),
        )
    }
}

pub fn clone_turn(t: &Turn) -> Turn {
    Turn {
        session_id: t.session_id.clone(),
        spend_session: t.spend_session.clone(),
        target: t.target.clone(),
        reasoning: t.reasoning.clone(),
        system: t.system.clone(),
        messages: t.messages.clone(),
        max_tokens: t.max_tokens,
        depth: t.depth,
        max_iterations: t.max_iterations,
        cancel: t.cancel.clone(),
        steer: t.steer.clone(),
    }
}

impl TurnResult {
    /// Upstream `_finish_reason`.
    pub fn finish_reason_wire(&self) -> &'static str {
        let truncated = self.partial
            && self
                .error
                .as_deref()
                .is_some_and(|e| e.to_lowercase().contains("truncat"));
        if truncated {
            "length"
        } else if self.failed || (!self.completed && self.error.is_some()) {
            "error"
        } else {
            "stop"
        }
    }
}

pub fn usage_json(r: &TurnResult) -> Value {
    json!({"prompt_tokens": r.usage.prompt_tokens, "completion_tokens": r.usage.completion_tokens, "total_tokens": r.usage.total_tokens})
}

fn extras(r: &TurnResult) -> Value {
    let fr = r.finish_reason_wire();
    json!({"completed": r.completed, "partial": r.partial, "failed": r.failed, "error": r.error,
           "error_code": if fr == "length" { "output_truncated" } else { "agent_error" }})
}

fn chat_json(
    id: &str,
    created: i64,
    model: &str,
    r: &TurnResult,
    sid: &str,
    key: Option<&str>,
) -> Response {
    let mut resp = if r.final_text.is_empty() && (r.failed || r.partial) {
        let msg = r.error.clone().unwrap_or_else(|| "agent run failed".into());
        let code = if msg.contains("spend cap") || msg.contains("call cap") {
            402
        } else {
            502
        };
        error(
            code,
            &msg,
            "agent_error",
            Some(if code == 402 {
                "spend_cap"
            } else {
                "agent_failed"
            }),
        )
    } else {
        let fr = r.finish_reason_wire();
        let mut msg = json!({"role": "assistant", "content": r.final_text});
        if !r.reasoning.is_empty() {
            msg["reasoning_content"] = json!(r.reasoning);
        }
        let mut body = json!({"id": id, "object": "chat.completion", "created": created, "model": model,
                              "choices": [{"index": 0, "message": msg, "finish_reason": fr}], "usage": usage_json(r)});
        if fr != "stop" {
            body["hermes"] = extras(r);
        }
        let mut resp = Response::json(200, &body);
        if fr != "stop" {
            resp = resp
                .with_header("X-Hermes-Completed", "false")
                .with_header("X-Hermes-Partial", if r.partial { "true" } else { "false" });
        }
        resp
    };
    resp = resp.with_header("X-Hermes-Session-Id", &crate::hdr_value(sid, 200));
    if let Some(k) = key {
        resp = resp.with_header("X-Hermes-Session-Key", &crate::hdr_value(k, 200));
    }
    resp
}

/// Bind and serve a gateway on `addr` in a background thread; returns the
/// bound address. Used by the binary and the golden tests.
pub fn spawn(gw: Arc<Gateway>, addr: &str) -> std::io::Result<std::net::SocketAddr> {
    let listener = std::net::TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    // NERVE push (no-op unless NERVE_URL is set; never blocks: bounded queue,
    // drop-on-full counted): 5xx errors and slow requests per request.
    let handler: crate::http::Handler = Arc::new(move |req: crate::http::Request| {
        let what = format!("{} {}", req.method, req.path);
        let t0 = std::time::Instant::now();
        let resp = gw.handle(req);
        nerve_client::observe_request("hermes-rs", &what, resp.status(), t0.elapsed());
        resp
    });
    nerve_client::event("hermes-rs", "started", &format!("listening on {local}"));
    nerve_client::start_heartbeat("hermes-rs", 60);
    std::thread::Builder::new()
        .name("hermes-accept".into())
        .spawn(move || crate::http::serve(listener, handler, 64))?;
    Ok(local)
}
