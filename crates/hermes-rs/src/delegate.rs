// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! `delegate_task`: run subagents with Galion's caps.
//!
//! * at most `delegation.max_concurrent_children` tasks per call (3),
//! * `delegation.max_spawn_depth` (1): children never get delegate_task,
//! * a node-wide gate of `max_concurrent` children running at once,
//! * spend is charged to the parent session, so the per-session cap covers
//!   the parent and all its children,
//! * Galion patch: refuse unless the latest user message asked for a helper.

use crate::agent::{Agent, Turn};
use crate::config::Config;
use crate::provider::{resolve_reasoning, resolve_target, Provider};
use crate::spend::SpendTracker;
use crate::tools::{Event, Registry, Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const HELPER_PHRASES: [&str; 5] = [
    "delegate_task",
    "delegate task",
    "one task",
    "use a helper",
    "spawn a",
];

pub const CHILD_SYSTEM: &str = "You are a focused subagent working for Hermes. You know nothing about the parent conversation beyond the goal and context below. Do the goal, then reply with a concise summary of what you found or did. Use only figures a tool returned.";

/// Counting gate for children running at once on this node.
pub struct Gate {
    n: Mutex<usize>,
    cv: Condvar,
    pub max: usize,
}

pub struct GateGuard<'a>(&'a Gate);

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut n) = self.0.n.lock() {
            *n = n.saturating_sub(1);
        }
        self.0.cv.notify_one();
    }
}

impl Gate {
    pub fn new(max: usize) -> Self {
        Gate {
            n: Mutex::new(0),
            cv: Condvar::new(),
            max: max.max(1),
        }
    }

    pub fn acquire(&self, timeout: Duration) -> Option<GateGuard<'_>> {
        let deadline = Instant::now() + timeout;
        let mut n = self.n.lock().ok()?;
        while *n >= self.max {
            let left = deadline.checked_duration_since(Instant::now())?;
            n = self.cv.wait_timeout(n, left).ok()?.0;
        }
        *n += 1;
        Some(GateGuard(self))
    }

    pub fn running(&self) -> usize {
        self.n.lock().map(|n| *n).unwrap_or(0)
    }
}

pub struct DelegateTool {
    pub cfg: Arc<Config>,
    pub provider: Arc<dyn Provider>,
    pub spend: Arc<SpendTracker>,
    /// Tools children may use (never delegate_task at depth 1).
    pub child_tools: Registry,
    pub gate: Arc<Gate>,
    pub wait: Duration,
}

impl DelegateTool {
    pub fn new(
        cfg: Arc<Config>,
        provider: Arc<dyn Provider>,
        spend: Arc<SpendTracker>,
        child_tools: Registry,
    ) -> Self {
        let max = cfg.delegation.max_concurrent;
        DelegateTool {
            cfg,
            provider,
            spend,
            child_tools: child_tools.without("delegate_task"),
            gate: Arc::new(Gate::new(max)),
            wait: Duration::from_secs(120),
        }
    }
}

/// Upstream accepts `tasks: [...]` or the legacy single `goal`.
pub fn parse_tasks(args: &Value) -> Vec<(String, String)> {
    let one = |t: &Value| {
        let goal = t["goal"].as_str().unwrap_or("").trim().to_string();
        let ctx = t["context"].as_str().unwrap_or("").trim().to_string();
        (goal, ctx)
    };
    match args["tasks"].as_array() {
        Some(a) => a.iter().map(one).collect(),
        None if args["goal"].is_string() => vec![one(args)],
        None => Vec::new(),
    }
}

pub fn user_asked_for_helper(text: &str) -> bool {
    let t = text.to_lowercase();
    // Upstream patch returns True when the text cannot be read.
    t.trim().is_empty() || HELPER_PHRASES.iter().any(|p| t.contains(p))
}

impl Tool for DelegateTool {
    fn name(&self) -> String {
        "delegate_task".into()
    }

    fn description(&self) -> String {
        format!(
            "Spawn one or more subagents in isolated contexts. Up to {} tasks per call, run in parallel; subagents cannot delegate further (max depth {}). Each child knows nothing about this conversation: make every goal self-contained.",
            self.cfg.delegation.max_children, self.cfg.delegation.max_depth
        )
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"tasks": {"type": "array", "minItems": 1,
            "description": format!("1 to {} self-contained tasks.", self.cfg.delegation.max_children),
            "items": {"type": "object", "properties": {
                "goal": {"type": "string", "description": "What this subagent should accomplish. Be specific and self-contained."},
                "context": {"type": "string", "description": "Background this child needs: paths, errors, constraints."}},
                "required": ["goal"]}}},
            "required": ["tasks"]})
    }

    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let action = args["action"].as_str().unwrap_or("").trim().to_lowercase();
        if matches!(action.as_str(), "list" | "steer" | "stop") {
            return ToolOutput::err("delegate_task actions list/steer/stop are not supported by hermes-rs v1: children run synchronously inside this call");
        }
        let d = &self.cfg.delegation;
        if ctx.depth >= d.max_depth {
            return ToolOutput::err(format!(
                "Delegation depth limit reached (max_spawn_depth={}). Do the task yourself.",
                d.max_depth
            ));
        }
        if d.require_user_ask && !user_asked_for_helper(ctx.user_text) {
            return ToolOutput::err("This request did not ask for a helper. Answer it yourself. Do not call delegate_task.");
        }
        let tasks = parse_tasks(args);
        if tasks.is_empty() || tasks.iter().any(|(g, _)| g.is_empty()) {
            return ToolOutput::err(
                "delegate_task needs tasks: [{goal, context?}] with a non-empty goal each",
            );
        }
        if tasks.len() > d.max_children {
            return ToolOutput::err(format!(
                "Too many tasks: {} > delegation.max_concurrent_children={}. Split the work or do some of it yourself.",
                tasks.len(),
                d.max_children
            ));
        }
        if let Err(b) = self.spend.check(ctx.spend_session) {
            return ToolOutput::err(b.message());
        }
        let target = resolve_target(&self.cfg, d.model.as_deref(), d.provider.as_deref());
        let reasoning = resolve_reasoning(&self.cfg, &target.model, None, true);
        let agent = Agent {
            cfg: self.cfg.clone(),
            provider: self.provider.clone(),
            spend: self.spend.clone(),
            tools: self.child_tools.clone(),
        };
        let t0 = Instant::now();
        let emit = ctx.emit;
        let results: Vec<Value> = std::thread::scope(|s| {
            let handles: Vec<_> = tasks
                .iter()
                .enumerate()
                .map(|(i, (goal, context))| {
                    let agent = &agent;
                    let target = target.clone();
                    let reasoning = reasoning.clone();
                    let gate = self.gate.clone();
                    let wait = self.wait;
                    let cancel = ctx.cancel.clone();
                    let spend_session = ctx.spend_session.to_string();
                    let child_session = format!("{}:child-{i}", ctx.session_id);
                    let depth = ctx.depth + 1;
                    let max_iter = d.max_iterations;
                    s.spawn(move || {
                        let Some(_g) = gate.acquire(wait) else {
                            return json!({"task_index": i, "status": "failed", "error": "no subagent slot free (node-wide cap)"});
                        };
                        emit(Event::Status(format!("subagent {i} started")));
                        let mut system = CHILD_SYSTEM.to_string();
                        if !context.is_empty() {
                            system.push_str("\n\nContext:\n");
                            system.push_str(context);
                        }
                        let turn = Turn {
                            session_id: child_session,
                            spend_session,
                            target: target.clone(),
                            reasoning,
                            system,
                            messages: vec![json!({"role": "user", "content": goal})],
                            max_tokens: None,
                            depth,
                            max_iterations: max_iter,
                            cancel,
                            steer: Arc::new(Mutex::new(Vec::new())),
                        };
                        let r = agent.run(turn, &|_e| {});
                        emit(Event::Status(format!("subagent {i} finished")));
                        let ok = r.completed && !r.failed;
                        json!({"task_index": i, "status": if ok { "completed" } else { "failed" }, "summary": r.final_text,
                               "error": r.error, "api_calls": r.api_calls, "model": target.model,
                               "tokens": {"input": r.usage.prompt_tokens, "output": r.usage.completion_tokens}})
                    })
                })
                .collect();
            handles
                .into_iter()
                .enumerate()
                .map(|(i, h)| h.join().unwrap_or_else(|_| json!({"task_index": i, "status": "failed", "error": "subagent panicked"})))
                .collect()
        });
        let any_ok = results.iter().any(|r| r["status"] == "completed");
        let out = json!({"results": results, "total_duration_seconds": (t0.elapsed().as_secs_f64() * 100.0).round() / 100.0});
        ToolOutput {
            content: out.to_string(),
            is_error: !any_ok,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatResult, MockProvider};
    use crate::tools::ClockTool;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn tool(mock: MockProvider, max_concurrent: usize) -> (DelegateTool, Arc<MockProvider>) {
        let mut cfg = Config::defaults(Path::new("/tmp"));
        cfg.delegation.model = Some("anthropic/claude-sonnet-5.5".into());
        cfg.delegation.reasoning_effort = Some("medium".into());
        cfg.delegation.max_concurrent = max_concurrent;
        let cfg = Arc::new(cfg);
        let mock = Arc::new(mock);
        let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
        let mut reg = Registry::new();
        reg.add(Arc::new(ClockTool));
        (DelegateTool::new(cfg, mock.clone(), spend, reg), mock)
    }

    fn call(t: &DelegateTool, args: Value, depth: usize, user: &str) -> ToolOutput {
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = ToolCtx {
            session_id: "s",
            spend_session: "s",
            depth,
            user_text: user,
            cancel: &cancel,
            emit: &|_e| {},
        };
        t.call(&args, &ctx)
    }

    #[test]
    fn runs_children_with_worker_model_and_no_delegate_tool() {
        let (t, mock) = tool(MockProvider::echo(), 3);
        let out = call(
            &t,
            json!({"tasks": [{"goal": "a"}, {"goal": "b", "context": "ctx"}]}),
            0,
            "use a helper please",
        );
        assert!(!out.is_error, "{}", out.content);
        let v: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(v["results"].as_array().unwrap().len(), 2);
        assert_eq!(v["results"][0]["summary"], "echo: a");
        assert_eq!(v["results"][1]["summary"], "echo: b");
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        for (tg, req) in calls.iter() {
            assert_eq!(tg.model, "anthropic/claude-sonnet-5.5");
            assert_eq!(
                req.reasoning,
                Some(crate::provider::Reasoning::Effort("medium".into()))
            );
            assert!(req
                .tools
                .iter()
                .all(|x| x["function"]["name"] != "delegate_task"));
        }
    }

    #[test]
    fn caps_tasks_depth_and_user_gate() {
        let (t, mock) = tool(MockProvider::echo(), 3);
        let four = json!({"tasks": [{"goal": "1"}, {"goal": "2"}, {"goal": "3"}, {"goal": "4"}]});
        let out = call(&t, four, 0, "delegate_task please");
        assert!(out.is_error && out.content.contains("Too many tasks"));
        let out = call(&t, json!({"tasks": [{"goal": "x"}]}), 1, "delegate_task");
        assert!(out.is_error && out.content.contains("depth limit"));
        let out = call(
            &t,
            json!({"tasks": [{"goal": "x"}]}),
            0,
            "what is the weather",
        );
        assert!(out.is_error && out.content.contains("did not ask for a helper"));
        let out = call(&t, json!({"goal": "legacy"}), 0, "spawn a helper");
        assert!(!out.is_error);
        assert_eq!(
            mock.calls.lock().unwrap().len(),
            1,
            "only the legacy call ran a child"
        );
    }

    #[test]
    fn node_wide_gate_limits_parallel_children() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (a, p) = (active.clone(), peak.clone());
        let mock = MockProvider::new(move |_, _| {
            let n = a.fetch_add(1, Ordering::SeqCst) + 1;
            p.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(80));
            a.fetch_sub(1, Ordering::SeqCst);
            ChatResult {
                content: "ok".into(),
                finish_reason: "stop".into(),
                ..Default::default()
            }
        });
        let (t, _) = tool(mock, 1);
        let out = call(
            &t,
            json!({"tasks": [{"goal": "1"}, {"goal": "2"}, {"goal": "3"}]}),
            0,
            "delegate_task",
        );
        assert!(!out.is_error);
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(t.gate.running(), 0);
    }

    #[test]
    fn children_share_the_parent_spend_cap() {
        let mock = MockProvider::new(|_, _| {
            let mut r = ChatResult {
                content: "ok".into(),
                finish_reason: "stop".into(),
                ..Default::default()
            };
            r.usage.cost_usd = 0.3;
            r
        });
        let (mut t, _) = tool(mock, 3);
        let mut cfg = (*t.cfg).clone();
        cfg.spend.session_usd_cap = 0.5;
        t.cfg = Arc::new(cfg.clone());
        t.spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
        let out = call(
            &t,
            json!({"tasks": [{"goal": "1"}, {"goal": "2"}]}),
            0,
            "delegate_task",
        );
        assert!(!out.is_error);
        assert!((t.spend.session_usd("s") - 0.6).abs() < 1e-9);
        let out = call(&t, json!({"tasks": [{"goal": "3"}]}), 0, "delegate_task");
        assert!(out.is_error && out.content.contains("spend cap"));
    }
}
