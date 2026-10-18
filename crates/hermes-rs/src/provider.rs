// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Model providers: model/route resolution (`galion` -> haiku etc.),
//! reasoning settings, the OpenAI-compatible wire body, SSE parsing, a curl
//! transport (OpenRouter, River, local Ollama) and a mock for tests.

use crate::config::{Config, ProviderCfg};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    /// The name the client asked for (`galion`, `boss`, a raw id).
    pub requested: String,
    pub key_env: Option<String>,
    pub inline_key: Option<String>,
}

impl Target {
    pub fn is_free(&self) -> bool {
        self.model.ends_with(":free")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reasoning {
    Default,
    Off,
    Effort(String),
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
    pub reasoning: Option<Reasoning>,
    pub max_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.prompt_tokens += o.prompt_tokens;
        self.completion_tokens += o.completion_tokens;
        self.total_tokens += o.total_tokens;
        self.cost_usd += o.cost_usd;
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatResult {
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: String,
    pub usage: Usage,
}

pub enum StreamEvent<'a> {
    Content(&'a str),
    Reasoning(&'a str),
}

pub trait Provider: Send + Sync {
    fn chat(
        &self,
        target: &Target,
        req: &ChatRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResult, String>;
}

/// Names that mean "the gateway default" rather than a model.
pub fn is_generic_model(name: &str) -> bool {
    matches!(name.trim(), "" | "hermes-agent" | "hermes" | "default")
}

fn provider_cfg(cfg: &Config, name: &str) -> ProviderCfg {
    cfg.providers.get(name).cloned().unwrap_or_else(|| {
        cfg.providers
            .get("openrouter")
            .cloned()
            .unwrap_or(ProviderCfg {
                name: name.to_string(),
                base_url: crate::config::DEFAULT_OPENROUTER_BASE.into(),
                key_env: Some("OPENROUTER_API_KEY".into()),
                inline_key: None,
                default_model: None,
            })
    })
}

/// Resolve a requested model (+ optional provider) to a concrete target.
/// Order: api_server model_routes, model_aliases, raw id. A provider-only
/// request uses that provider's default model.
pub fn resolve_target(cfg: &Config, model: Option<&str>, provider: Option<&str>) -> Target {
    let requested = model
        .map(str::trim)
        .filter(|m| !is_generic_model(m))
        .map(str::to_string);
    let provider = provider
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let route = requested
        .as_deref()
        .and_then(|m| cfg.model_routes.get(m).or_else(|| cfg.aliases.get(m)));
    let (model_id, prov_name, base, key_env) = match route {
        Some(r) => (
            r.model.clone(),
            r.provider
                .clone()
                .or(provider.clone())
                .unwrap_or_else(|| cfg.default_provider.clone()),
            r.base_url.clone(),
            r.key_env.clone(),
        ),
        None => {
            let p = provider
                .clone()
                .unwrap_or_else(|| cfg.default_provider.clone());
            let m = requested.clone().unwrap_or_else(|| {
                if provider.is_some() {
                    provider_cfg(cfg, &p)
                        .default_model
                        .unwrap_or_else(|| cfg.default_model.clone())
                } else {
                    cfg.default_model.clone()
                }
            });
            (m, p, None, None)
        }
    };
    let pc = provider_cfg(cfg, &prov_name);
    let custom = base.is_some();
    Target {
        provider: prov_name,
        base_url: base.unwrap_or(pc.base_url),
        model: model_id,
        requested: requested.unwrap_or_else(|| "hermes-agent".into()),
        key_env: key_env.or(pc.key_env),
        inline_key: if custom { None } else { pc.inline_key },
    }
}

/// Reasoning for one call. `request` is the body's `model_options.reasoning`
/// (console shape `{enabled, effort}`) or a top-level `reasoning_effort`.
pub fn resolve_reasoning(
    cfg: &Config,
    model: &str,
    request: Option<&Value>,
    child: bool,
) -> Reasoning {
    if let Some(r) = request {
        if let Some(e) = r.as_str() {
            return effort_of(e);
        }
        if r["enabled"].as_bool() == Some(false) {
            return Reasoning::Off;
        }
        if let Some(e) = r["effort"].as_str() {
            return effort_of(e);
        }
    }
    if let Some(o) = cfg.reasoning_overrides.get(model) {
        return effort_of(o);
    }
    let base = if child {
        cfg.delegation
            .reasoning_effort
            .clone()
            .unwrap_or_else(|| cfg.reasoning_effort.clone())
    } else {
        cfg.reasoning_effort.clone()
    };
    effort_of(&base)
}

fn effort_of(e: &str) -> Reasoning {
    match e.trim().to_ascii_lowercase().as_str() {
        "" | "default" => Reasoning::Default,
        "none" | "off" | "false" => Reasoning::Off,
        other => Reasoning::Effort(other.to_string()),
    }
}

fn host_of(url: &str) -> &str {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or("");
    host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host)
}

/// The OpenAI-compatible request body for `t`. Pure, so it is golden-tested.
pub fn build_body(t: &Target, r: &ChatRequest) -> Value {
    let mut b = json!({
        "model": t.model,
        "messages": r.messages,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if !r.tools.is_empty() {
        b["tools"] = Value::Array(r.tools.clone());
        b["tool_choice"] = json!("auto");
    }
    if let Some(m) = r.max_tokens {
        b["max_tokens"] = json!(m);
    }
    let reasoning = r.reasoning.clone().unwrap_or(Reasoning::Default);
    let host = host_of(&t.base_url).to_ascii_lowercase();
    if host == "openrouter.ai" || t.provider == "openrouter" {
        b["usage"] = json!({"include": true});
        match reasoning {
            Reasoning::Default => {}
            // Anthropic reasoning-mandatory routes 400 on a disable: omit.
            Reasoning::Off if t.model.starts_with("anthropic/") => {}
            Reasoning::Off => b["reasoning"] = json!({"enabled": false}),
            Reasoning::Effort(e) => b["reasoning"] = json!({"enabled": true, "effort": e}),
        }
    } else {
        let river = host == "api.river.ai";
        match reasoning {
            Reasoning::Default => {}
            Reasoning::Off => {
                // Galion patch: River GLM has no off value ("none" is a 400).
                if !(river && t.model.to_ascii_lowercase().contains("glm")) {
                    b["reasoning_effort"] = json!("none");
                }
                if t.base_url.contains(":11434") {
                    b["think"] = json!(false);
                }
            }
            Reasoning::Effort(e) => {
                // Galion patch: River 400s on medium/minimal.
                let e = if river && (e == "medium" || e == "minimal") {
                    "low".to_string()
                } else {
                    e
                };
                b["reasoning_effort"] = json!(e);
            }
        }
    }
    b
}

/// Accumulates an OpenAI-compatible SSE stream (or one JSON body).
#[derive(Default)]
pub struct StreamAcc {
    pub result: ChatResult,
    calls: Vec<(String, String, String)>,
    pub done: bool,
    pub error: Option<String>,
    non_sse: String,
}

impl StreamAcc {
    /// Feed one line; returns (content_delta, reasoning_delta).
    pub fn feed_line(&mut self, line: &str) -> (String, String) {
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(data) = line.strip_prefix("data:") else {
            if !line.starts_with(':') && !line.starts_with("event:") && !line.is_empty() {
                self.non_sse.push_str(line);
                self.non_sse.push('\n');
            }
            return (String::new(), String::new());
        };
        let data = data.trim();
        if data == "[DONE]" {
            self.done = true;
            return (String::new(), String::new());
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return (String::new(), String::new());
        };
        self.chunk(&v)
    }

    fn chunk(&mut self, v: &Value) -> (String, String) {
        if let Some(e) = v.get("error") {
            self.error = Some(
                e["message"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| e.to_string()),
            );
        }
        self.usage(&v["usage"]);
        let mut c_out = String::new();
        let mut r_out = String::new();
        if let Some(ch) = v["choices"].get(0) {
            let d = if ch.get("delta").is_some() {
                &ch["delta"]
            } else {
                &ch["message"]
            };
            if let Some(c) = d["content"].as_str() {
                c_out.push_str(c);
            }
            for k in ["reasoning", "reasoning_content"] {
                if let Some(r) = d[k].as_str() {
                    r_out.push_str(r);
                }
            }
            if let Some(tcs) = d["tool_calls"].as_array() {
                for (pos, tc) in tcs.iter().enumerate() {
                    let idx = tc["index"].as_u64().map(|i| i as usize).unwrap_or(pos);
                    while self.calls.len() <= idx {
                        self.calls.push(Default::default());
                    }
                    let slot = &mut self.calls[idx];
                    if let Some(id) = tc["id"].as_str() {
                        slot.0 = id.to_string();
                    }
                    if let Some(n) = tc["function"]["name"].as_str() {
                        slot.1.push_str(n);
                    }
                    match &tc["function"]["arguments"] {
                        Value::String(a) => slot.2.push_str(a),
                        Value::Null => {}
                        other => slot.2.push_str(&other.to_string()),
                    }
                }
            }
            if let Some(f) = ch["finish_reason"].as_str() {
                self.result.finish_reason = f.to_string();
            }
        }
        self.result.content.push_str(&c_out);
        self.result.reasoning.push_str(&r_out);
        (c_out, r_out)
    }

    fn usage(&mut self, u: &Value) {
        if !u.is_object() {
            return;
        }
        let g = |k: &str| u[k].as_u64().unwrap_or(0);
        self.result.usage.prompt_tokens = g("prompt_tokens");
        self.result.usage.completion_tokens = g("completion_tokens");
        self.result.usage.total_tokens =
            g("total_tokens").max(g("prompt_tokens") + g("completion_tokens"));
        if let Some(c) = u["cost"].as_f64() {
            self.result.usage.cost_usd = c;
        }
    }

    pub fn finish(mut self) -> Result<ChatResult, String> {
        if !self.non_sse.trim().is_empty()
            && self.result.content.is_empty()
            && self.calls.is_empty()
        {
            if let Ok(v) = serde_json::from_str::<Value>(self.non_sse.trim()) {
                self.chunk(&v);
            } else if self.error.is_none() {
                self.error = Some(self.non_sse.chars().take(300).collect());
            }
        }
        if let Some(e) = self.error.take() {
            return Err(crate::redact(&e));
        }
        for (i, (id, name, args)) in self.calls.into_iter().enumerate() {
            if name.is_empty() {
                continue;
            }
            let id = if id.is_empty() {
                format!("call_{i}")
            } else {
                id
            };
            self.result.tool_calls.push(ToolCall {
                id,
                name,
                arguments: args,
            });
        }
        if self.result.finish_reason.is_empty() {
            self.result.finish_reason = if self.result.tool_calls.is_empty() {
                "stop".into()
            } else {
                "tool_calls".into()
            };
        }
        Ok(self.result)
    }
}

pub type EnvFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// HTTPS through the system `curl` (portable: no TLS crate, no C build dep).
/// Body and key go through stdin as a curl config, never argv.
pub struct CurlProvider {
    pub env: EnvFn,
    pub curl: String,
    pub max_time_s: u64,
}

impl CurlProvider {
    pub fn new(env: EnvFn) -> Self {
        CurlProvider {
            env,
            curl: "curl".into(),
            max_time_s: 900,
        }
    }

    fn key(&self, t: &Target) -> Option<String> {
        t.key_env
            .as_deref()
            .and_then(|k| (self.env)(k))
            .or_else(|| t.inline_key.clone())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }
}

/// Escape for a double-quoted curl config value.
pub fn curl_quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

impl Provider for CurlProvider {
    fn chat(
        &self,
        t: &Target,
        req: &ChatRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResult, String> {
        let key = self.key(t);
        let local = matches!(host_of(&t.base_url), "127.0.0.1" | "localhost" | "::1");
        if key.is_none() && !local {
            return Err(format!(
                "no API key for provider {} (set {})",
                t.provider,
                t.key_env.as_deref().unwrap_or("a key env")
            ));
        }
        if key
            .as_deref()
            .is_some_and(|k| k.contains(['"', '\n', '\r']))
        {
            return Err("provider key has unusable characters".into());
        }
        let url = format!("{}/chat/completions", t.base_url.trim_end_matches('/'));
        let body = build_body(t, req).to_string();
        let mut child = Command::new(&self.curl)
            .args([
                "-sS",
                "-N",
                "--fail-with-body",
                "--connect-timeout",
                "15",
                "--max-time",
            ])
            .arg(self.max_time_s.to_string())
            .args(["-K", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        {
            let mut si = child.stdin.take().ok_or("curl stdin")?;
            let mut cfgtxt = format!(
                "url = {}\nheader = \"Content-Type: application/json\"\n",
                curl_quote(&url)
            );
            if let Some(k) = key {
                cfgtxt.push_str(&format!("header = \"Authorization: Bearer {k}\"\n"));
            }
            cfgtxt.push_str("header = \"X-Title: hermes-rs\"\n");
            cfgtxt.push_str(&format!("data-binary = {}\n", curl_quote(&body)));
            si.write_all(cfgtxt.as_bytes())
                .map_err(|e| format!("curl stdin: {e}"))?;
        }
        let out = child.stdout.take().ok_or("curl stdout")?;
        let mut acc = StreamAcc::default();
        for line in BufReader::new(out).lines() {
            if cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cancelled".into());
            }
            let Ok(line) = line else { break };
            let (c, r) = acc.feed_line(&line);
            if !r.is_empty() {
                sink(StreamEvent::Reasoning(&r));
            }
            if !c.is_empty() {
                sink(StreamEvent::Content(&c));
            }
        }
        let status = child.wait().map_err(|e| format!("curl: {e}"))?;
        if !status.success() && acc.error.is_none() && acc.result.content.is_empty() {
            let mut err = String::new();
            if let Some(mut se) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut se, &mut err);
            }
            let tail: String = acc.non_sse.chars().take(300).collect();
            return Err(crate::redact(&format!(
                "provider call failed (curl exit {:?}) {} {}",
                status.code(),
                err.trim(),
                tail.trim()
            )));
        }
        acc.finish()
    }
}

type MockFn = dyn Fn(&Target, &ChatRequest) -> ChatResult + Send + Sync;

/// Deterministic provider for tests and `--mock`. Never touches the network.
pub struct MockProvider {
    pub behaviour: Box<MockFn>,
    pub calls: Mutex<Vec<(Target, ChatRequest)>>,
    pub delay_ms: u64,
}

impl MockProvider {
    pub fn new(f: impl Fn(&Target, &ChatRequest) -> ChatResult + Send + Sync + 'static) -> Self {
        MockProvider {
            behaviour: Box::new(f),
            calls: Mutex::new(Vec::new()),
            delay_ms: 0,
        }
    }

    /// Echo the last user line. `delegate: <goal>` makes it call
    /// delegate_task once; `tool:<name> <json>` calls any tool once.
    pub fn echo() -> Self {
        MockProvider::new(|_t, r| {
            let last = r.messages.last().cloned().unwrap_or(Value::Null);
            let user = r
                .messages
                .iter()
                .rev()
                .find(|m| m["role"] == "user")
                .and_then(|m| m["content"].as_str())
                .unwrap_or("")
                .to_string();
            let mut res = ChatResult {
                finish_reason: "stop".into(),
                ..Default::default()
            };
            res.usage = Usage {
                prompt_tokens: (r
                    .messages
                    .iter()
                    .map(|m| m.to_string().len())
                    .sum::<usize>()
                    / 4) as u64,
                completion_tokens: 8,
                total_tokens: 0,
                cost_usd: 0.0,
            };
            res.usage.total_tokens = res.usage.prompt_tokens + 8;
            if last["role"] == "tool" {
                res.content = format!(
                    "done: {}",
                    last["content"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(400)
                        .collect::<String>()
                );
                return res;
            }
            let has_tool = |n: &str| r.tools.iter().any(|t| t["function"]["name"] == n);
            if let Some(goal) = user.strip_prefix("delegate:") {
                if has_tool("delegate_task") {
                    let goals: Vec<Value> =
                        goal.split('|').map(|g| json!({"goal": g.trim()})).collect();
                    res.tool_calls.push(ToolCall {
                        id: "call_mock_1".into(),
                        name: "delegate_task".into(),
                        arguments: json!({"tasks": goals}).to_string(),
                    });
                    res.finish_reason = "tool_calls".into();
                    return res;
                }
            }
            if let Some(rest) = user.strip_prefix("tool:") {
                let (name, args) = rest.split_once(' ').unwrap_or((rest, "{}"));
                if has_tool(name) {
                    res.tool_calls.push(ToolCall {
                        id: "call_mock_1".into(),
                        name: name.into(),
                        arguments: args.into(),
                    });
                    res.finish_reason = "tool_calls".into();
                    return res;
                }
            }
            res.content = format!("echo: {user}");
            res
        })
    }
}

impl Provider for MockProvider {
    fn chat(
        &self,
        target: &Target,
        req: &ChatRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResult, String> {
        if let Ok(mut c) = self.calls.lock() {
            c.push((target.clone(), req.clone()));
        }
        if self.delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
        }
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let res = (self.behaviour)(target, req);
        if !res.reasoning.is_empty() {
            sink(StreamEvent::Reasoning(&res.reasoning));
        }
        // stream in word-sized pieces like a real provider
        for piece in res.content.split_inclusive(' ') {
            sink(StreamEvent::Content(piece));
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;
    use std::path::Path;

    fn galion_cfg() -> Config {
        let mut c = Config::defaults(Path::new("/h"));
        c.apply_yaml(
            &yaml::parse(
                r#"
model:
  provider: openrouter
  default: anthropic/claude-opus-5.5
  base_url: https://openrouter.ai/api/v1
model_aliases:
  worker:
    model: anthropic/claude-sonnet-5.5
    provider: openrouter
  river-glm:
    model: zai-org/GLM-5.3-Flash
    provider: custom
    base_url: "https://api.river.ai/v1"
    key_env: RIVER_API_KEY
providers:
  river:
    api: https://api.river.ai/v1
    default_model: deepseek-ai/DeepSeek-V4.1-Flash
agent:
  reasoning_effort: high
  reasoning_overrides:
    "anthropic/claude-haiku-5.5": "none"
delegation:
  reasoning_effort: medium
platforms:
  api_server:
    extra:
      model_routes:
        galion:
          model: "anthropic/claude-haiku-5.5"
          provider: "openrouter"
"#,
            )
            .unwrap(),
        );
        c
    }

    #[test]
    fn resolves_routes_aliases_and_defaults() {
        let c = galion_cfg();
        let t = resolve_target(&c, Some("galion"), None);
        assert_eq!(t.model, "anthropic/claude-haiku-5.5");
        assert_eq!(t.provider, "openrouter");
        assert_eq!(t.requested, "galion");
        let t = resolve_target(&c, Some("hermes-agent"), None);
        assert_eq!(t.model, "anthropic/claude-opus-5.5");
        let t = resolve_target(&c, Some("worker"), None);
        assert_eq!(t.model, "anthropic/claude-sonnet-5.5");
        let t = resolve_target(&c, Some("deepseek-ai/DeepSeek-V4.1-Flash"), Some("river"));
        assert_eq!(t.base_url, "https://api.river.ai/v1");
        assert_eq!(t.key_env.as_deref(), Some("RIVER_API_KEY"));
        let t = resolve_target(&c, None, Some("river"));
        assert_eq!(t.model, "deepseek-ai/DeepSeek-V4.1-Flash");
        let t = resolve_target(&c, Some("river-glm"), None);
        assert_eq!(t.base_url, "https://api.river.ai/v1");
    }

    #[test]
    fn reasoning_rules() {
        let c = galion_cfg();
        assert_eq!(
            resolve_reasoning(&c, "anthropic/claude-opus-5.5", None, false),
            Reasoning::Effort("high".into())
        );
        assert_eq!(
            resolve_reasoning(&c, "anthropic/claude-haiku-5.5", None, false),
            Reasoning::Off
        );
        assert_eq!(
            resolve_reasoning(&c, "anthropic/claude-sonnet-5.5", None, true),
            Reasoning::Effort("medium".into())
        );
        let req = json!({"enabled": false, "effort": "low"});
        assert_eq!(
            resolve_reasoning(&c, "x", Some(&req), false),
            Reasoning::Off
        );
        let req = json!({"enabled": true, "effort": "high"});
        assert_eq!(
            resolve_reasoning(&c, "x", Some(&req), false),
            Reasoning::Effort("high".into())
        );
    }

    #[test]
    fn wire_bodies() {
        let c = galion_cfg();
        let mut r = ChatRequest {
            messages: vec![json!({"role":"user","content":"hi"})],
            ..Default::default()
        };
        r.reasoning = Some(Reasoning::Effort("high".into()));
        let b = build_body(&resolve_target(&c, None, None), &r);
        assert_eq!(b["reasoning"], json!({"enabled": true, "effort": "high"}));
        assert_eq!(b["usage"], json!({"include": true}));
        r.reasoning = Some(Reasoning::Off);
        let b = build_body(&resolve_target(&c, Some("galion"), None), &r);
        assert!(b.get("reasoning").is_none(), "anthropic disable is omitted");
        r.reasoning = Some(Reasoning::Effort("medium".into()));
        let b = build_body(
            &resolve_target(&c, Some("deepseek-ai/DeepSeek-V4.1-Flash"), Some("river")),
            &r,
        );
        assert_eq!(b["reasoning_effort"], "low");
        r.reasoning = Some(Reasoning::Off);
        let b = build_body(&resolve_target(&c, Some("river-glm"), None), &r);
        assert!(b.get("reasoning_effort").is_none());
    }

    #[test]
    fn sse_accumulates_text_tools_usage() {
        let mut a = StreamAcc::default();
        let lines = [
            r#"data: {"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"lo","reasoning":"hmm"}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"delegate_task","arguments":"{\"tasks\":"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"[]}"}}]},"finish_reason":"tool_calls"}]}"#,
            ": OPENROUTER PROCESSING",
            r#"data: {"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"cost":0.0012}}"#,
            "data: [DONE]",
        ];
        let mut text = String::new();
        for l in lines {
            text.push_str(&a.feed_line(l).0);
        }
        assert!(a.done);
        let r = a.finish().unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(r.reasoning, "hmm");
        assert_eq!(r.tool_calls[0].name, "delegate_task");
        assert_eq!(r.tool_calls[0].arguments, "{\"tasks\":[]}");
        assert_eq!(r.usage.total_tokens, 15);
        assert!((r.usage.cost_usd - 0.0012).abs() < 1e-9);
    }

    #[test]
    fn non_sse_error_body() {
        let mut a = StreamAcc::default();
        a.feed_line(r#"{"error":{"message":"Insufficient credits","code":402}}"#);
        assert_eq!(a.finish().unwrap_err(), "Insufficient credits");
    }

    #[test]
    fn curl_quote_escapes() {
        assert_eq!(curl_quote("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }
}
