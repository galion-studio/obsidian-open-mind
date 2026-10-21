// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! One Hermes agent (hermes-rs `Agent` loop + a tool registry + a provider)
//! and a turn runner that returns every tool call the loop executed.

use crate::planner::{Brain, PlanIo, Script, ScriptFactory, ScriptedProvider};
use hermes_rs::agent::{Agent, Turn};
use hermes_rs::config::{Config, SpendCfg};
use hermes_rs::provider::{Provider, Reasoning, Target, Usage};
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{Event, Registry};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Spend / loop limits for a session.
#[derive(Debug, Clone)]
pub struct Limits {
    pub max_iterations: usize,
    pub max_tokens: Option<u64>,
    pub spend: SpendCfg,
}

impl Limits {
    /// Scripted and mock-LLM runs cost nothing: no spend cap, generous loop cap.
    pub fn offline() -> Self {
        Limits {
            max_iterations: 200,
            max_tokens: None,
            spend: SpendCfg {
                session_usd_cap: 0.0,
                daily_usd_cap: 0.0,
                daily_call_cap: 0,
            },
        }
    }

    /// A paid model: hard caps so a runaway loop cannot spend.
    pub fn live(usd_cap: f64, call_cap: u64) -> Self {
        Limits {
            max_iterations: 40,
            max_tokens: Some(1024),
            spend: SpendCfg {
                session_usd_cap: usd_cap,
                daily_usd_cap: usd_cap,
                daily_call_cap: call_cap,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallRecord {
    pub tool: String,
    pub args: Value,
    pub content: Value,
    pub is_error: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TurnReport {
    pub calls: Vec<CallRecord>,
    pub final_text: String,
    pub error: Option<String>,
    pub api_calls: usize,
    pub usage: Usage,
}

pub struct HermesSession {
    agent: Agent,
    target: Target,
    system: String,
    limits: Limits,
    seq: Mutex<u64>,
    /// `--agent gateway`: turns go to a real hermes-gateway instead of the
    /// in-process agent loop (see [`crate::e2e`]).
    remote: Option<Arc<crate::e2e::Remote>>,
}

/// The target a scripted run reports (no network, no key).
pub fn scripted_target() -> Target {
    Target {
        provider: "scripted".into(),
        base_url: "in-process".into(),
        model: "hermes-bench/scripted".into(),
        requested: "scripted".into(),
        key_env: None,
        inline_key: None,
    }
}

impl HermesSession {
    pub fn new(
        provider: Arc<dyn Provider>,
        target: Target,
        tools: Registry,
        system: &str,
        limits: Limits,
    ) -> Self {
        let mut cfg = Config::defaults(&std::env::temp_dir());
        cfg.spend = limits.spend.clone();
        let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
        HermesSession {
            agent: Agent {
                cfg: Arc::new(cfg),
                provider,
                spend,
                tools,
            },
            target,
            system: system.into(),
            limits,
            seq: Mutex::new(0),
            remote: None,
        }
    }

    /// A session whose model is a deterministic script factory.
    pub fn scripted(tools: Registry, system: &str, factory: ScriptFactory) -> Self {
        let p = Arc::new(ScriptedProvider {
            brain: Brain::new(factory),
        });
        Self::new(p, scripted_target(), tools, system, Limits::offline())
    }

    /// A session whose turns run in a hermes-gateway process. The gateway
    /// has its own bench tool registry wired to the same backends, so the
    /// local `Registry` a bench passes in is not used.
    pub fn remote(r: Arc<crate::e2e::Remote>, system: &str) -> Self {
        let mut s = Self::scripted(
            Registry::new(),
            system,
            Box::new(|_u: &str| -> Script { Box::new(|_io: &mut PlanIo| String::new()) }),
        );
        s.remote = Some(r);
        s
    }

    pub fn tool_names(&self) -> Vec<String> {
        if let Some(r) = &self.remote {
            return r.tool_names();
        }
        self.agent.tools.names()
    }

    /// One user turn through the hermes-rs agent loop.
    pub fn turn(&self, prompt: &str) -> TurnReport {
        if let Some(r) = &self.remote {
            return r.turn(&self.system, prompt);
        }
        let n = {
            let mut s = self.seq.lock().unwrap_or_else(|p| p.into_inner());
            *s += 1;
            *s
        };
        let flags: Mutex<Vec<bool>> = Mutex::new(Vec::new());
        let emit = |e: Event| {
            if let Event::ToolCompleted { error, .. } = e {
                flags.lock().unwrap_or_else(|p| p.into_inner()).push(error);
            }
        };
        let turn = Turn {
            session_id: format!("bench-{n}"),
            spend_session: "bench".into(),
            target: self.target.clone(),
            reasoning: Reasoning::Default,
            system: self.system.clone(),
            messages: vec![json!({"role": "user", "content": prompt})],
            max_tokens: self.limits.max_tokens,
            depth: 0,
            max_iterations: self.limits.max_iterations,
            cancel: Arc::new(AtomicBool::new(false)),
            steer: Arc::new(Mutex::new(Vec::new())),
        };
        let res = self.agent.run(turn, &emit);
        let flags = flags.into_inner().unwrap_or_default();
        let mut pending: HashMap<String, (String, Value)> = HashMap::new();
        let mut calls = Vec::new();
        for m in &res.added {
            if let Some(tcs) = m["tool_calls"].as_array() {
                for tc in tcs {
                    let args = tc["function"]["arguments"]
                        .as_str()
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or(Value::Null);
                    pending.insert(
                        tc["id"].as_str().unwrap_or_default().to_string(),
                        (
                            tc["function"]["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            args,
                        ),
                    );
                }
            } else if m["role"] == "tool" {
                let id = m["tool_call_id"].as_str().unwrap_or_default();
                let (tool, args) = pending.remove(id).unwrap_or_default();
                let raw = m["content"].as_str().unwrap_or_default();
                let i = calls.len();
                calls.push(CallRecord {
                    tool,
                    args,
                    content: serde_json::from_str(raw).unwrap_or_else(|_| json!(raw)),
                    is_error: flags.get(i).copied().unwrap_or(false),
                });
            }
        }
        TurnReport {
            calls,
            final_text: res.final_text,
            error: res.error,
            api_calls: res.api_calls,
            usage: res.usage,
        }
    }
}

/// A script that makes exactly one tool call and reports it.
pub fn one_call(tool: &str, args: &Value) -> ScriptFactory {
    let (tool, args) = (tool.to_string(), args.clone());
    Box::new(move |_u: &str| -> Script {
        let (tool, args) = (tool.clone(), args.clone());
        Box::new(move |io: &mut PlanIo| {
            let r = io.call(&tool, args);
            format!(
                "{} -> {}",
                r.tool,
                if r.is_error() { "error" } else { "ok" }
            )
        })
    })
}

/// Run one tool call through a fresh scripted Hermes session on `tools`.
pub fn hermes_call(tools: Registry, tool: &str, args: &Value) -> Result<CallRecord, String> {
    let s = HermesSession::scripted(tools, "hermes-bench scripted step", one_call(tool, args));
    let rep = s.turn(&format!("bench step: call {tool}"));
    match rep.calls.into_iter().next() {
        Some(c) if c.tool == tool => Ok(c),
        Some(c) => Err(format!("hermes ran {} instead of {tool}", c.tool)),
        None => Err(format!(
            "hermes ran no tool ({})",
            rep.error.unwrap_or(rep.final_text)
        )),
    }
}
