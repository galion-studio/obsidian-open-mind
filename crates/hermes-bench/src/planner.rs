// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Deterministic "model" side of a Hermes run.
//!
//! A [`Brain`] turns the conversation hermes-rs sends to a model into the
//! next step: a tool call, or the final answer. The scripted agent plugs a
//! brain in as a `hermes_rs::provider::Provider` ([`ScriptedProvider`]); the
//! mock LLM ([`crate::mockllm`]) serves the very same brain over the
//! OpenAI-compatible wire, so the curl transport and SSE/JSON parser are on
//! the path too. Either way, every tool call is executed by the hermes-rs
//! agent loop through its tool registry, never by the script itself.
//!
//! Scripts are plain straight-line Rust (`io.call("bank_balance", {})` ...)
//! running on a helper thread; each `call` becomes one model step.

use hermes_rs::provider::{
    ChatRequest, ChatResult, Provider, StreamEvent, Target, ToolCall, Usage,
};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// What the model saw last.
#[derive(Debug, Clone)]
pub enum Obs {
    User(String),
    Tool { name: String, content: String },
}

/// What the model does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Call { tool: String, args: Value },
    Done(String),
}

/// A tool result as the script sees it.
#[derive(Debug, Clone)]
pub struct Reply {
    pub tool: String,
    pub value: Value,
    /// The turn ended (hermes stopped asking); further calls are no-ops.
    pub dead: bool,
}

impl Reply {
    pub fn is_error(&self) -> bool {
        self.dead || self.value.get("error").is_some()
    }
}

/// The script's handle: each `call` is one model step through Hermes.
pub struct PlanIo {
    step_tx: Sender<Step>,
    obs_rx: Receiver<Obs>,
    dead: bool,
}

impl PlanIo {
    pub fn call(&mut self, tool: &str, args: Value) -> Reply {
        let dead = |tool: &str| Reply {
            tool: tool.into(),
            value: serde_json::json!({"error": "turn ended"}),
            dead: true,
        };
        if self.dead {
            return dead(tool);
        }
        if self
            .step_tx
            .send(Step::Call {
                tool: tool.into(),
                args,
            })
            .is_err()
        {
            self.dead = true;
            return dead(tool);
        }
        match self.obs_rx.recv() {
            Ok(Obs::Tool { name, content }) => Reply {
                value: serde_json::from_str(&content).unwrap_or(Value::String(content)),
                tool: name,
                dead: false,
            },
            _ => {
                self.dead = true;
                dead(tool)
            }
        }
    }
}

pub type Script = Box<dyn FnOnce(&mut PlanIo) -> String + Send>;

/// One turn of a script, run on its own thread.
pub struct ThreadPlanner {
    step_rx: Receiver<Step>,
    obs_tx: Sender<Obs>,
}

impl ThreadPlanner {
    pub fn spawn(script: Script) -> Self {
        let (step_tx, step_rx) = channel();
        let (obs_tx, obs_rx) = channel();
        std::thread::spawn(move || {
            let mut io = PlanIo {
                step_tx,
                obs_rx,
                dead: false,
            };
            let text = script(&mut io);
            let _ = io.step_tx.send(Step::Done(text));
        });
        ThreadPlanner { step_rx, obs_tx }
    }

    /// Feed the last observation, get the next step.
    pub fn next(&mut self, obs: Obs) -> Step {
        if let Obs::Tool { .. } = obs {
            if self.obs_tx.send(obs).is_err() {
                return Step::Done("script ended".into());
            }
        }
        self.step_rx
            .recv()
            .unwrap_or_else(|_| Step::Done("script ended".into()))
    }
}

/// Builds the script for a turn from the user's prompt.
pub type ScriptFactory = Box<dyn FnMut(&str) -> Script + Send>;

/// Conversation -> next step, one turn at a time.
pub struct Brain {
    factory: Mutex<ScriptFactory>,
    current: Mutex<Option<ThreadPlanner>>,
    seq: AtomicU64,
}

impl Brain {
    pub fn new(factory: ScriptFactory) -> Self {
        Brain {
            factory: Mutex::new(factory),
            current: Mutex::new(None),
            seq: AtomicU64::new(0),
        }
    }

    /// The next step for an OpenAI-style message list (as sent to a model).
    pub fn respond(&self, messages: &[Value]) -> Step {
        let last = messages.last().cloned().unwrap_or(Value::Null);
        let mut cur = self.current.lock().unwrap_or_else(|p| p.into_inner());
        let obs = if last["role"] == "tool" {
            Obs::Tool {
                name: last["name"].as_str().unwrap_or_default().to_string(),
                content: last["content"].as_str().unwrap_or_default().to_string(),
            }
        } else {
            let user = last["content"].as_str().unwrap_or_default().to_string();
            let script = (self.factory.lock().unwrap_or_else(|p| p.into_inner()))(&user);
            *cur = Some(ThreadPlanner::spawn(script));
            Obs::User(user)
        };
        match cur.as_mut() {
            Some(p) => p.next(obs),
            None => Step::Done("no active turn".into()),
        }
    }

    pub fn next_call_id(&self) -> String {
        format!("call_{}", self.seq.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

/// The scripted agent as a hermes-rs model provider (no network at all).
pub struct ScriptedProvider {
    pub brain: Brain,
}

impl Provider for ScriptedProvider {
    fn chat(
        &self,
        _t: &Target,
        req: &ChatRequest,
        cancel: &AtomicBool,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResult, String> {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let mut r = ChatResult {
            usage: Usage::default(),
            ..Default::default()
        };
        match self.brain.respond(&req.messages) {
            Step::Call { tool, args } => {
                r.tool_calls.push(ToolCall {
                    id: self.brain.next_call_id(),
                    name: tool,
                    arguments: args.to_string(),
                });
                r.finish_reason = "tool_calls".into();
            }
            Step::Done(text) => {
                r.content = text;
                r.finish_reason = "stop".into();
            }
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn brain_walks_a_script() {
        let b = Brain::new(Box::new(|_u: &str| -> Script {
            Box::new(|io: &mut PlanIo| {
                let r = io.call("t1", json!({"a": 1}));
                let r2 = io.call("t2", json!({}));
                format!("{} {}", r.value["x"], r2.is_error())
            })
        }));
        let s = b.respond(&[json!({"role": "user", "content": "go"})]);
        assert_eq!(
            s,
            Step::Call {
                tool: "t1".into(),
                args: json!({"a": 1})
            }
        );
        let s = b.respond(&[json!({"role": "tool", "name": "t1", "content": "{\"x\":7}"})]);
        assert_eq!(
            s,
            Step::Call {
                tool: "t2".into(),
                args: json!({})
            }
        );
        let s =
            b.respond(&[json!({"role": "tool", "name": "t2", "content": "{\"error\":\"no\"}"})]);
        assert_eq!(s, Step::Done("7 true".into()));
        // a new user message starts a fresh script
        let s = b.respond(&[json!({"role": "user", "content": "again"})]);
        assert!(matches!(s, Step::Call { .. }));
    }
}
