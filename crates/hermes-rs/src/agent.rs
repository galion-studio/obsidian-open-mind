// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The agent loop: call the model, run tool calls, feed results back, until
//! a final answer, a cap, a cancel or a failure.

use crate::config::Config;
use crate::provider::{ChatRequest, Provider, Reasoning, StreamEvent, Target, Usage};
use crate::spend::SpendTracker;
use crate::tools::{Event, Registry, ToolCtx};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct Agent {
    pub cfg: Arc<Config>,
    pub provider: Arc<dyn Provider>,
    pub spend: Arc<SpendTracker>,
    pub tools: Registry,
}

pub struct Turn {
    pub session_id: String,
    pub spend_session: String,
    pub target: Target,
    pub reasoning: Reasoning,
    pub system: String,
    /// Prior conversation (no system message) plus the new user message.
    pub messages: Vec<Value>,
    pub max_tokens: Option<u64>,
    pub depth: usize,
    pub max_iterations: usize,
    pub cancel: Arc<AtomicBool>,
    /// Guidance injected mid-run (`/v1/runs/{id}/steer`).
    pub steer: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Clone, Default)]
pub struct TurnResult {
    pub final_text: String,
    pub reasoning: String,
    /// Messages produced this turn (assistant + tool), in order.
    pub added: Vec<Value>,
    pub usage: Usage,
    pub api_calls: usize,
    pub tool_calls: usize,
    pub completed: bool,
    pub partial: bool,
    pub failed: bool,
    pub error: Option<String>,
    pub finish_reason: String,
}

/// Rough token estimate (chars / 4), the same order of magnitude upstream
/// uses before it has a tokenizer.
pub fn estimate_tokens(m: &Value) -> usize {
    m.to_string().len() / 4 + 4
}

/// Trim history from the front to fit `context_length - reserve`, keeping the
/// system message and never starting on an orphaned tool result.
pub fn fit_context(
    system: &Value,
    msgs: &[Value],
    context_length: usize,
    reserve: usize,
) -> Vec<Value> {
    let budget = context_length.saturating_sub(reserve).max(256);
    let mut used = estimate_tokens(system);
    let mut keep_from = msgs.len();
    for (i, m) in msgs.iter().enumerate().rev() {
        let t = estimate_tokens(m);
        if used + t > budget && keep_from < msgs.len() {
            break;
        }
        used += t;
        keep_from = i;
    }
    let mut out: Vec<Value> = msgs[keep_from..].to_vec();
    while out.len() > 1 && out[0]["role"] == "tool" {
        out.remove(0);
    }
    let mut v = vec![system.clone()];
    v.append(&mut out);
    v
}

impl Agent {
    pub fn run(&self, mut turn: Turn, emit: &(dyn Fn(Event) + Sync)) -> TurnResult {
        let mut res = TurnResult::default();
        let user_text = turn
            .messages
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("")
            .to_string();
        if let Some(l) = &self.cfg.model_lock {
            if !l.target_ok(&self.cfg, &turn.target) {
                let msg = crate::model_lock::refused(
                    "agent turn",
                    &format!(
                        "model {} on {} refused; locked to {}",
                        turn.target.model, turn.target.provider, l.model
                    ),
                );
                res.failed = true;
                res.error = Some(msg);
                res.finish_reason = "error".into();
                return res;
            }
        }
        let system = json!({"role": "system", "content": turn.system});
        let schemas = self.tools.schemas();
        let mut failures: HashMap<String, usize> = HashMap::new();
        let reserve = turn.max_tokens.unwrap_or(4096) as usize;
        for _ in 0..turn.max_iterations.max(1) {
            if turn.cancel.load(Ordering::SeqCst) {
                res.error = Some("cancelled".into());
                res.partial = !res.final_text.is_empty();
                res.finish_reason = "cancelled".into();
                return res;
            }
            if let Ok(mut s) = turn.steer.lock() {
                for g in s.drain(..) {
                    let m = json!({"role": "user", "content": format!("[steer] {g}")});
                    turn.messages.push(m.clone());
                    res.added.push(m);
                }
            }
            if let Err(b) = self.spend.check(&turn.spend_session) {
                res.failed = true;
                res.error = Some(b.message());
                res.finish_reason = "error".into();
                return res;
            }
            let req = ChatRequest {
                messages: fit_context(&system, &turn.messages, self.cfg.context_length, reserve),
                tools: schemas.clone(),
                reasoning: Some(turn.reasoning.clone()),
                max_tokens: turn.max_tokens,
            };
            let mut sink = |e: StreamEvent| match e {
                StreamEvent::Content(c) => emit(Event::Delta(c.to_string())),
                StreamEvent::Reasoning(r) => emit(Event::Reasoning(r.to_string())),
            };
            res.api_calls += 1;
            let out = match self
                .provider
                .chat(&turn.target, &req, &turn.cancel, &mut sink)
            {
                Ok(o) => o,
                Err(e) => {
                    res.failed = true;
                    res.partial = !res.final_text.is_empty();
                    res.error = Some(crate::redact(&e));
                    res.finish_reason = "error".into();
                    return res;
                }
            };
            self.spend
                .record(&turn.spend_session, &turn.target.model, out.usage.cost_usd);
            res.usage.add(&out.usage);
            if !out.reasoning.is_empty() {
                res.reasoning.push_str(&out.reasoning);
            }
            if out.tool_calls.is_empty() {
                res.final_text = out.content.clone();
                let mut m = json!({"role": "assistant", "content": out.content});
                if !out.reasoning.is_empty() {
                    m["reasoning"] = json!(out.reasoning);
                }
                res.added.push(m);
                res.completed = true;
                res.partial = out.finish_reason == "length";
                if res.partial {
                    res.error = Some("output truncated".into());
                }
                res.finish_reason = if res.partial {
                    "length".into()
                } else {
                    "stop".into()
                };
                return res;
            }
            let calls: Vec<Value> = out
                .tool_calls
                .iter()
                .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}}))
                .collect();
            let am = json!({"role": "assistant", "content": if out.content.is_empty() { Value::Null } else { json!(out.content) }, "tool_calls": calls});
            turn.messages.push(am.clone());
            res.added.push(am);
            for c in &out.tool_calls {
                res.tool_calls += 1;
                let preview: String = c.arguments.chars().take(200).collect();
                emit(Event::ToolStarted {
                    tool: c.name.clone(),
                    preview: crate::redact(&preview),
                });
                let t0 = Instant::now();
                let args: Value = if c.arguments.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&c.arguments).unwrap_or(Value::Null)
                };
                let output = match (self.tools.get(&c.name), args.is_null()) {
                    (None, _) => crate::tools::ToolOutput::err(format!("Unknown tool: {}", c.name)),
                    (Some(_), true) => {
                        crate::tools::ToolOutput::err("tool arguments are not valid JSON")
                    }
                    (Some(t), false) => {
                        let ctx = ToolCtx {
                            session_id: &turn.session_id,
                            spend_session: &turn.spend_session,
                            depth: turn.depth,
                            user_text: &user_text,
                            cancel: &turn.cancel,
                            emit,
                        };
                        t.call(&args, &ctx)
                    }
                };
                let preview: String = output.content.chars().take(500).collect();
                emit(Event::ToolCompleted {
                    tool: c.name.clone(),
                    duration: (t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
                    error: output.is_error,
                    preview: crate::redact(&preview),
                });
                let tm = json!({"role": "tool", "tool_call_id": c.id, "name": c.name, "content": output.content});
                turn.messages.push(tm.clone());
                res.added.push(tm);
                if output.is_error {
                    let key = format!("{}\u{0}{}", c.name, c.arguments);
                    let n = failures.entry(key).or_insert(0);
                    *n += 1;
                    if *n >= self.cfg.tool_hard_stop_after {
                        res.failed = true;
                        res.error = Some(format!(
                            "tool loop guardrail: {} failed {} times with the same arguments",
                            c.name, n
                        ));
                        res.finish_reason = "error".into();
                        return res;
                    }
                }
            }
        }
        res.partial = true;
        res.error = Some(format!("iteration cap reached ({})", turn.max_iterations));
        res.finish_reason = "length".into();
        res
    }
}

/// The boss agent: `base` tools plus `delegate_task` (children get `base`).
pub fn boss(
    cfg: Arc<Config>,
    provider: Arc<dyn Provider>,
    spend: Arc<SpendTracker>,
    base: Registry,
) -> Agent {
    let mut tools = base.clone();
    tools.add(Arc::new(crate::delegate::DelegateTool::new(
        cfg.clone(),
        provider.clone(),
        spend.clone(),
        base,
    )));
    Agent {
        cfg,
        provider,
        spend,
        tools,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_context_drops_oldest_and_orphans() {
        let sys = json!({"role": "system", "content": "s"});
        let big = "x".repeat(4000);
        let msgs = vec![
            json!({"role": "user", "content": big}),
            json!({"role": "assistant", "content": null, "tool_calls": []}),
            json!({"role": "tool", "tool_call_id": "a", "content": big}),
            json!({"role": "user", "content": "now"}),
        ];
        let out = fit_context(&sys, &msgs, 1300, 0);
        assert_eq!(out[0]["role"], "system");
        assert_eq!(out.last().unwrap()["content"], "now");
        assert!(out
            .iter()
            .skip(1)
            .all(|m| m["role"] != "tool" || out[1]["role"] != "tool"));
        assert!(out.len() < 5);
    }
}
