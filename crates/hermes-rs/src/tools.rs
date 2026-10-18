// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Tool registry and the tool trait the agent loop calls.

use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Events a turn emits while it runs (streamed to SSE clients).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Delta(String),
    Reasoning(String),
    ToolStarted {
        tool: String,
        preview: String,
    },
    ToolCompleted {
        tool: String,
        duration: f64,
        error: bool,
        preview: String,
    },
    Status(String),
}

pub struct ToolCtx<'a> {
    pub session_id: &'a str,
    /// Session the spend is charged to (the parent for subagents).
    pub spend_session: &'a str,
    pub depth: usize,
    pub user_text: &'a str,
    pub cancel: &'a AtomicBool,
    pub emit: &'a (dyn Fn(Event) + Sync),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(s: impl Into<String>) -> Self {
        ToolOutput {
            content: s.into(),
            is_error: false,
        }
    }
    /// Upstream `tool_error`: a JSON `{"error": ...}` body.
    pub fn err(s: impl AsRef<str>) -> Self {
        ToolOutput {
            content: json!({"error": s.as_ref()}).to_string(),
            is_error: true,
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    /// JSON Schema of the parameters.
    fn parameters(&self) -> Value;
    fn description(&self) -> String;
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput;
}

#[derive(Clone, Default)]
pub struct Registry {
    tools: Vec<Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    pub fn add(&mut self, t: Arc<dyn Tool>) {
        let n = t.name();
        self.tools.retain(|x| x.name() != n);
        self.tools.push(t);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn without(&self, name: &str) -> Registry {
        Registry {
            tools: self
                .tools
                .iter()
                .filter(|t| t.name() != name)
                .cloned()
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// OpenAI `tools` array.
    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name(), "description": t.description(), "parameters": t.parameters()}}))
            .collect()
    }
}

/// A trivial built-in so a node with no MCP still has one safe tool.
pub struct ClockTool;

impl Tool for ClockTool {
    fn name(&self) -> String {
        "clock".into()
    }
    fn description(&self) -> String {
        "Current time in Europe/Zagreb (RFC 3339 with offset).".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn call(&self, _args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        ToolOutput::ok(crate::time::zagreb_rfc3339(crate::time::now_secs()))
    }
}
