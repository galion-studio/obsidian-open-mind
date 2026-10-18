// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! `/v1/runs`: start an agent run in the background, poll it, stream its
//! events (SSE), stop or steer it. Event payloads follow upstream
//! `_run_event`: `{"event", "run_id", "timestamp", ...fields}`.

use crate::api::{clone_turn, usage_json, Gateway, Prepared};
use crate::http::{error, Request, Response, SseWriter};
use crate::tools::Event;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub const MAX_KEPT_RUNS: usize = 200;

pub struct Run {
    pub id: String,
    status: Mutex<Value>,
    events: Mutex<Vec<Value>>,
    cond: Condvar,
    done: AtomicBool,
    pub cancel: Arc<AtomicBool>,
    pub steer: Arc<Mutex<Vec<String>>>,
}

impl Run {
    fn push(&self, name: &str, fields: Value) {
        let mut e = json!({"event": name, "run_id": self.id, "timestamp": crate::time::now_f64()});
        if let (Some(o), Some(f)) = (e.as_object_mut(), fields.as_object()) {
            for (k, v) in f {
                o.insert(k.clone(), v.clone());
            }
        }
        if let Ok(mut ev) = self.events.lock() {
            ev.push(e);
            if ev.len() > 20_000 {
                ev.drain(..10_000);
            }
        }
        self.cond.notify_all();
    }

    fn set_status(&self, status: &str, extra: Value) {
        if let Ok(mut s) = self.status.lock() {
            s["status"] = json!(status);
            s["updated_at"] = json!(crate::time::now_f64());
            if let Some(o) = extra.as_object() {
                for (k, v) in o {
                    s[k] = v.clone();
                }
            }
        }
    }

    pub fn status(&self) -> Value {
        self.status.lock().map(|s| s.clone()).unwrap_or(Value::Null)
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct Runs {
    map: Mutex<HashMap<String, Arc<Run>>>,
}

impl Runs {
    pub fn get(&self, id: &str) -> Option<Arc<Run>> {
        self.map.lock().ok()?.get(id).cloned()
    }

    fn insert(&self, r: Arc<Run>) {
        if let Ok(mut m) = self.map.lock() {
            if m.len() >= MAX_KEPT_RUNS {
                let mut done: Vec<(String, f64)> = m
                    .values()
                    .filter(|r| r.is_done())
                    .map(|r| {
                        (
                            r.id.clone(),
                            r.status()["updated_at"].as_f64().unwrap_or(0.0),
                        )
                    })
                    .collect();
                done.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                for (id, _) in done.into_iter().take(MAX_KEPT_RUNS / 4) {
                    m.remove(&id);
                }
            }
            m.insert(r.id.clone(), r);
        }
    }
}

/// `input` may be a string or a list of messages (upstream accepts both).
fn input_messages(body: &Value) -> Result<Vec<Value>, String> {
    let mut msgs: Vec<Value> = Vec::new();
    if let Some(h) = body["conversation_history"].as_array() {
        msgs.extend(
            h.iter()
                .filter(|m| m["role"] == "user" || m["role"] == "assistant")
                .cloned(),
        );
    }
    match &body["input"] {
        Value::String(s) if !s.trim().is_empty() => {
            msgs.push(json!({"role": "user", "content": s}))
        }
        Value::Array(a) => msgs.extend(a.iter().cloned()),
        _ => return Err("Missing 'input'".into()),
    }
    Ok(msgs)
}

pub fn start(gw: &Arc<Gateway>, req: &Request) -> Response {
    let body = match req.json() {
        Ok(b) => b,
        Err(e) => return error(400, &e, "invalid_request_error", None),
    };
    let msgs = match input_messages(&body) {
        Ok(m) => m,
        Err(e) => return error(400, &e, "invalid_request_error", Some("input")),
    };
    let key = req
        .header("x-hermes-session-key")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let sid = body["session_id"]
        .as_str()
        .map(str::to_string)
        .or_else(|| req.header("x-hermes-session-id").map(str::to_string))
        .filter(|s| !s.trim().is_empty());
    if sid
        .as_deref()
        .is_some_and(|s| s.len() > crate::api::MAX_SESSION_HEADER_LEN)
    {
        return error(
            400,
            "session_id too long",
            "invalid_request_error",
            Some("invalid_session_id"),
        );
    }
    let prepared = match gw.prepare(
        sid,
        key.clone(),
        &body,
        &msgs,
        body["instructions"].as_str(),
    ) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(slot) = gw.try_slot() else {
        return error(
            429,
            "too many concurrent runs",
            "rate_limit_error",
            Some("max_concurrent_runs"),
        );
    };
    let run_id = crate::new_id("run_");
    let run = Arc::new(Run {
        id: run_id.clone(),
        status: Mutex::new(
            json!({"object": "hermes.run", "run_id": run_id, "status": "started",
                                  "session_id": prepared.row.id, "model": prepared.target.model,
                                  "created_at": crate::time::now_f64(), "updated_at": crate::time::now_f64()}),
        ),
        events: Mutex::new(Vec::new()),
        cond: Condvar::new(),
        done: AtomicBool::new(false),
        cancel: prepared.turn.cancel.clone(),
        steer: prepared.turn.steer.clone(),
    });
    gw.runs.insert(run.clone());
    let gw2 = gw.clone();
    let r2 = run.clone();
    let spawned = std::thread::Builder::new()
        .name("hermes-run".into())
        .spawn(move || {
            let _slot = slot;
            execute(&gw2, &r2, prepared);
        });
    if spawned.is_err() {
        return error(500, "could not start run", "server_error", None);
    }
    let mut resp = Response::json(
        202,
        &json!({"run_id": run_id, "status": "started", "replayed": false}),
    );
    if let Some(k) = key {
        resp = resp.with_header("X-Hermes-Session-Key", &k);
    }
    resp
}

fn execute(gw: &Gateway, run: &Arc<Run>, prepared: Prepared) {
    run.set_status("running", json!({}));
    run.push("run.started", json!({"session_id": prepared.row.id}));
    let emit = |e: Event| match e {
        Event::Delta(d) => run.push("message.delta", json!({"delta": d})),
        Event::Reasoning(r) => run.push("reasoning.available", json!({"text": r})),
        Event::ToolStarted { tool, preview } => {
            run.push("tool.started", json!({"tool": tool, "preview": preview}))
        }
        Event::ToolCompleted {
            tool,
            duration,
            error,
            preview,
        } => run.push(
            "tool.completed",
            json!({"tool": tool, "duration": duration, "error": error, "preview": preview}),
        ),
        Event::Status(s) => run.push("status", json!({"text": s})),
    };
    let res = gw.agent.run(clone_turn(&prepared.turn), &emit);
    gw.persist(&prepared, &res);
    let (status, fields) = if run.cancel.load(Ordering::SeqCst) {
        ("cancelled", json!({}))
    } else if res.failed && res.final_text.is_empty() {
        (
            "failed",
            json!({"error": res.error.clone().unwrap_or_else(|| "agent run failed".into())}),
        )
    } else {
        (
            "completed",
            json!({"output": res.final_text, "usage": usage_json(&res)}),
        )
    };
    run.set_status(status, fields.clone());
    run.push(&format!("run.{status}"), fields);
    run.done.store(true, Ordering::SeqCst);
    run.cond.notify_all();
}

pub fn get(gw: &Gateway, id: &str) -> Response {
    match gw.runs.get(id) {
        Some(r) => Response::json(200, &r.status()),
        None => error(
            404,
            &format!("Run not found: {id}"),
            "invalid_request_error",
            Some("run_not_found"),
        ),
    }
}

pub fn events(gw: &Gateway, id: &str) -> Response {
    let Some(run) = gw.runs.get(id) else {
        return error(
            404,
            &format!("Run not found: {id}"),
            "invalid_request_error",
            Some("run_not_found"),
        );
    };
    Response::Stream {
        status: 200,
        headers: Vec::new(),
        writer: Box::new(move |w: &mut SseWriter| {
            let mut next = 0usize;
            loop {
                let (batch, finished) = {
                    let ev = match run.events.lock() {
                        Ok(e) => e,
                        Err(p) => p.into_inner(),
                    };
                    let ev = if ev.len() <= next && !run.is_done() {
                        match run.cond.wait_timeout(ev, Duration::from_secs(15)) {
                            Ok((e, _)) => e,
                            Err(p) => p.into_inner().0,
                        }
                    } else {
                        ev
                    };
                    let batch: Vec<Value> = ev.iter().skip(next).cloned().collect();
                    next = ev.len();
                    (batch, run.is_done())
                };
                if batch.is_empty() && !finished && !w.raw(b": keepalive\n\n") {
                    return;
                }
                for e in &batch {
                    if !w.frame(None, e) {
                        return;
                    }
                }
                if finished && batch.is_empty() {
                    w.raw(b": stream closed\n\n");
                    return;
                }
            }
        }),
    }
}

pub fn stop(gw: &Gateway, id: &str) -> Response {
    let Some(run) = gw.runs.get(id) else {
        return error(
            404,
            &format!("Run not found: {id}"),
            "invalid_request_error",
            Some("run_not_found"),
        );
    };
    if !run.is_done() {
        run.cancel.store(true, Ordering::SeqCst);
        run.set_status("stopping", json!({}));
        run.push("run.stopping", json!({}));
    }
    Response::json(200, &json!({"run_id": id, "status": "stopping"}))
}

pub fn steer(gw: &Gateway, id: &str, req: &Request) -> Response {
    let Some(run) = gw.runs.get(id) else {
        return error(
            404,
            &format!("Run not found: {id}"),
            "invalid_request_error",
            Some("run_not_found"),
        );
    };
    let body = req.json().unwrap_or(Value::Null);
    let text = ["text", "input", "message", "guidance"]
        .iter()
        .find_map(|k| body[*k].as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return error(
            400,
            "Missing steer text",
            "invalid_request_error",
            Some("text"),
        );
    }
    if run.is_done() || run.status()["status"] != "running" {
        return error(
            409,
            &format!("Run is not running: {id}"),
            "invalid_request_error",
            Some("run_not_running"),
        );
    }
    if let Ok(mut s) = run.steer.lock() {
        s.push(text);
    }
    run.push("run.steered", json!({"accepted": true}));
    Response::json(
        200,
        &json!({"object": "hermes.run.steer", "run_id": id, "accepted": true}),
    )
}

/// Approvals are `mode: off` on Galion, so a run never has a pending one.
pub fn approval(gw: &Gateway, id: &str) -> Response {
    if gw.runs.get(id).is_none() {
        return error(
            404,
            &format!("Run not found: {id}"),
            "invalid_request_error",
            Some("run_not_found"),
        );
    }
    error(
        409,
        &format!("Run has no active approval session: {id}"),
        "invalid_request_error",
        Some("approval_not_active"),
    )
}
