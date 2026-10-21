// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Optional LLM advisor for ambiguous high-severity incidents.
//!
//! DISABLED by default (`llm.enabled = false`). When enabled it is still
//! fenced in, checked in this order before any byte leaves the box:
//! 1. endpoint guard: refuse any endpoint containing `openrouter` (owner
//!    rule: never call OpenRouter, directly or via Hermes), any Hermes
//!    gateway (`hermes`, port 8642), and anything but `https://` (plain
//!    `http://` only to loopback, for tests);
//! 2. severity floor (`min_severity`, default error);
//! 3. calls-per-hour cap (default 6) from the persisted ledger;
//! 4. hard USD budget: worst-case cost of this call (prompt chars / 3 tokens
//!    at the input price + `max_tokens` at the output price) must fit under
//!    `budget_usd_total` (<= $15, persisted in `llm-spend.json`); the worst
//!    case is reserved in the ledger BEFORE the call and settled to the
//!    reported usage after it, so a crash mid-call never under-counts;
//!    an unreadable ledger refuses (fail closed);
//! 5. the key is read from the env var NAMED by `key_env` at call time,
//!    never logged, passed to curl on stdin (never argv).
//!
//! The answer may only pick one of the offered choices (`none`, `alert`,
//! `escalate`, `restart:<allowlisted unit>`); anything else becomes
//! `escalate`. A chosen restart still goes through every rule-side guard.

use crate::config::{LlmConfig, HARD_BUDGET_CEILING_USD};
use openmind_nerve::incident::Sev;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(feature = "llm-advisor")]
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(feature = "llm-advisor")]
use std::process::{Command, Stdio};

/// What the advisor may answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    None,
    Alert,
    Escalate,
    Restart(String),
}

impl Choice {
    pub fn wire(&self) -> String {
        match self {
            Choice::None => "none".into(),
            Choice::Alert => "alert".into(),
            Choice::Escalate => "escalate".into(),
            Choice::Restart(u) => format!("restart:{u}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    Disabled,
    Endpoint(String),
    BelowSeverity,
    RateCap { calls_last_hour: usize, cap: u32 },
    Budget { spent: f64, worst: f64, cap: f64 },
    Ledger(String),
    NoKey(String),
    Transport(String),
}

impl Refusal {
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::Disabled => "disabled",
            Refusal::Endpoint(_) => "endpoint-guard",
            Refusal::BelowSeverity => "below-severity",
            Refusal::RateCap { .. } => "calls-per-hour-cap",
            Refusal::Budget { .. } => "budget-cap",
            Refusal::Ledger(_) => "ledger-error",
            Refusal::NoKey(_) => "no-key",
            Refusal::Transport(_) => "transport-error",
        }
    }
    pub fn text(&self) -> String {
        match self {
            Refusal::Disabled => "LLM advisor disabled".into(),
            Refusal::Endpoint(r) => format!("endpoint refused: {r}"),
            Refusal::BelowSeverity => "incident below the advisor severity floor".into(),
            Refusal::RateCap {
                calls_last_hour,
                cap,
            } => format!("{calls_last_hour} calls in the last hour >= cap {cap}"),
            Refusal::Budget { spent, worst, cap } => {
                format!("budget: spent ${spent:.6} + worst case ${worst:.6} > cap ${cap:.2}")
            }
            Refusal::Ledger(e) => format!("spend ledger unusable ({e}); refusing"),
            Refusal::NoKey(n) => format!("env var {n} not set"),
            Refusal::Transport(e) => format!("call failed: {e}"),
        }
    }
}

/// Endpoint guard. Returns why an endpoint is refused.
pub fn endpoint_refusal(endpoint: &str) -> Option<String> {
    let l = endpoint.to_ascii_lowercase();
    if l.contains("openrouter") {
        return Some(
            "contains 'openrouter' (owner rule: never call OpenRouter directly or indirectly)"
                .into(),
        );
    }
    if l.contains("hermes") || l.contains(":8642") {
        return Some(
            "Hermes gateway (routes to OpenRouter by default; indirect OpenRouter is banned)"
                .into(),
        );
    }
    if l.contains('@')
        || l.chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\\')
    {
        return Some("malformed endpoint".into());
    }
    if l.starts_with("https://") {
        return None;
    }
    if let Some(rest) = l.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or("");
        let h = host.rsplit_once(':').map_or(host, |(h, _)| h);
        if h == "127.0.0.1" || h == "localhost" || h == "[::1]" {
            return None;
        }
        return Some("plain http only to loopback".into());
    }
    Some("endpoint must be https://".into())
}

/// Persisted spend ledger (`<data_dir>/llm-spend.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub total_usd: f64,
    pub calls: u64,
    pub refused: u64,
    /// Unix seconds of recent calls (last hour kept).
    pub recent: Vec<u64>,
    /// Reserved worst case of an in-flight call (settled after it).
    pub reserved_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Ledger {
    pub fn load(path: &Path) -> Result<Ledger, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                let mut l: Ledger =
                    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?;
                if !(l.total_usd.is_finite()
                    && l.total_usd >= 0.0
                    && l.reserved_usd.is_finite()
                    && l.reserved_usd >= 0.0)
                {
                    return Err("ledger values invalid".into());
                }
                // A reservation left by a crash counts as spent.
                l.total_usd += l.reserved_usd;
                l.reserved_usd = 0.0;
                Ok(l)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn calls_last_hour(&self, now: u64) -> usize {
        self.recent
            .iter()
            .filter(|t| now.saturating_sub(**t) < 3600)
            .count()
    }
}

/// HTTP transport (curl in production, a mock in tests).
pub trait Transport: Send {
    /// POST `body` to the OpenAI-compatible chat endpoint under `endpoint`
    /// with `Authorization: Bearer <key>`; returns the body.
    fn post(&self, endpoint: &str, key: &str, body: &str, timeout_s: u64)
        -> Result<String, String>;
}

/// Default transport of a build without the `llm-advisor` feature: refuses.
pub struct NotCompiled;

impl Transport for NotCompiled {
    fn post(
        &self,
        _endpoint: &str,
        _key: &str,
        _body: &str,
        _timeout_s: u64,
    ) -> Result<String, String> {
        Err("built without the llm-advisor cargo feature (off by default; enabling needs Mate's approval)".into())
    }
}

/// `curl -K -`: key and body path go in on stdin (never argv). Only in
/// builds with the `llm-advisor` feature.
#[cfg(feature = "llm-advisor")]
pub struct CurlTransport {
    pub tmp_dir: PathBuf,
}

#[cfg(feature = "llm-advisor")]
impl Transport for CurlTransport {
    fn post(
        &self,
        endpoint: &str,
        key: &str,
        body: &str,
        timeout_s: u64,
    ) -> Result<String, String> {
        if key.contains('"') || key.contains('\n') || key.contains('\\') {
            return Err("key has unusable characters".into());
        }
        let url = format!("{}/chat/completions", endpoint.trim_end_matches('/'));
        let url = url.as_str();
        let body_path = self
            .tmp_dir
            .join(format!("llm-body-{}.json", std::process::id()));
        std::fs::write(&body_path, body).map_err(|e| format!("body file: {e}"))?;
        let child = Command::new("curl")
            .args([
                "-sS",
                "--fail",
                "--max-time",
                &timeout_s.to_string(),
                "--proto",
                "=https,http",
                "-K",
                "-",
                url,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&body_path);
                return Err(format!("curl: {e}"));
            }
        };
        if let Some(mut si) = child.stdin.take() {
            let _ = writeln!(si, "header = \"Authorization: Bearer {key}\"");
            let _ = writeln!(si, "header = \"Content-Type: application/json\"");
            let _ = writeln!(si, "data-binary = \"@{}\"", body_path.display());
        }
        let out = child.wait_with_output();
        let _ = std::fs::remove_file(&body_path);
        let out = out.map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            return Err(format!("curl exit {:?}", out.status.code()));
        }
        String::from_utf8(out.stdout).map_err(|_| "response not utf-8".into())
    }
}

/// The advisor's reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Advice {
    pub choice: Choice,
    pub reason: String,
    pub cost_usd: f64,
}

/// Env lookup (injectable for tests); only ever asked for `key_env`.
pub type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send>;

pub struct Advisor {
    pub cfg: LlmConfig,
    pub ledger_path: PathBuf,
    pub transport: Box<dyn Transport>,
    pub get_env: EnvLookup,
    /// Calls actually sent (this process).
    pub calls: u64,
    pub refusals: std::collections::BTreeMap<&'static str, u64>,
}

impl Advisor {
    pub fn new(cfg: LlmConfig, data_dir: &Path) -> Self {
        Advisor {
            cfg,
            ledger_path: data_dir.join("llm-spend.json"),
            transport: default_transport(data_dir),
            get_env: Box::new(|k| std::env::var(k).ok()),
            calls: 0,
            refusals: Default::default(),
        }
    }

    fn cap(&self) -> f64 {
        self.cfg.budget_usd_total.min(HARD_BUDGET_CEILING_USD)
    }

    /// Worst-case USD of a call with this prompt.
    pub fn worst_case(&self, prompt_chars: usize) -> f64 {
        let tin = prompt_chars.div_ceil(3) as f64 + 64.0;
        let tout = self.cfg.max_tokens as f64;
        tin * self.cfg.price_in_per_m / 1e6 + tout * self.cfg.price_out_per_m / 1e6
    }

    fn refuse(&mut self, r: Refusal) -> Result<Advice, Refusal> {
        *self.refusals.entry(r.code()).or_default() += 1;
        if !matches!(r, Refusal::Disabled | Refusal::Ledger(_)) {
            if let Ok(mut l) = Ledger::load(&self.ledger_path) {
                l.refused += 1;
                let _ = l.save(&self.ledger_path);
            }
        }
        Err(r)
    }

    /// Ask for advice. `choices` always contains none/alert/escalate.
    pub fn advise(
        &mut self,
        sev: Sev,
        summary: &str,
        choices: &[Choice],
        now: u64,
    ) -> Result<Advice, Refusal> {
        if !self.cfg.enabled {
            return self.refuse(Refusal::Disabled);
        }
        if let Some(r) = endpoint_refusal(&self.cfg.endpoint) {
            return self.refuse(Refusal::Endpoint(r));
        }
        let floor = Sev::parse(&self.cfg.min_severity).unwrap_or(Sev::Error);
        if sev < floor {
            return self.refuse(Refusal::BelowSeverity);
        }
        let mut ledger = match Ledger::load(&self.ledger_path) {
            Ok(l) => l,
            Err(e) => return self.refuse(Refusal::Ledger(e)),
        };
        ledger.recent.retain(|t| now.saturating_sub(*t) < 3600);
        let n = ledger.calls_last_hour(now);
        if n >= self.cfg.calls_per_hour as usize {
            return self.refuse(Refusal::RateCap {
                calls_last_hour: n,
                cap: self.cfg.calls_per_hour,
            });
        }
        let prompt = build_prompt(summary, choices, self.cfg.max_prompt_chars);
        let worst = self.worst_case(prompt.len());
        if ledger.total_usd + worst > self.cap() {
            return self.refuse(Refusal::Budget {
                spent: ledger.total_usd,
                worst,
                cap: self.cap(),
            });
        }
        let key = match (self.get_env)(&self.cfg.key_env) {
            Some(k) if !k.trim().is_empty() => k,
            _ => return self.refuse(Refusal::NoKey(self.cfg.key_env.clone())),
        };
        // Reserve the worst case before anything leaves the box.
        ledger.reserved_usd = worst;
        ledger.recent.push(now);
        ledger.calls += 1;
        if let Err(e) = ledger.save(&self.ledger_path) {
            return self.refuse(Refusal::Ledger(e));
        }
        self.calls += 1;
        let body = json!({
            "model": self.cfg.model,
            "max_tokens": self.cfg.max_tokens,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": prompt},
            ],
        })
        .to_string();
        let res = self
            .transport
            .post(&self.cfg.endpoint, key.trim(), &body, self.cfg.timeout_s);
        drop(key);
        let (cost, out) = match &res {
            Ok(text) => {
                let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
                let cost = usage_cost(&v, &self.cfg)
                    .unwrap_or(worst)
                    .min(worst.max(0.0) * 4.0);
                let u = &v["usage"];
                ledger.input_tokens += u["prompt_tokens"].as_u64().unwrap_or(0);
                ledger.output_tokens += u["completion_tokens"].as_u64().unwrap_or(0);
                let content = v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                (cost, Some(content))
            }
            // A failed call may still have been billed: charge the worst case.
            Err(_) => (worst, None),
        };
        ledger.reserved_usd = 0.0;
        ledger.total_usd += cost;
        if let Err(e) = ledger.save(&self.ledger_path) {
            return self.refuse(Refusal::Ledger(e));
        }
        match out {
            None => self.refuse(Refusal::Transport(res.err().unwrap_or_default())),
            Some(content) => {
                let (choice, reason) = parse_choice(&content, choices);
                Ok(Advice {
                    choice,
                    reason,
                    cost_usd: cost,
                })
            }
        }
    }

    pub fn stats(&self) -> Value {
        let l = Ledger::load(&self.ledger_path);
        json!({
            "enabled": self.cfg.enabled,
            "endpoint": self.cfg.endpoint,
            "endpoint_guard": endpoint_refusal(&self.cfg.endpoint).unwrap_or_else(|| "ok".into()),
            "model": self.cfg.model,
            "key_env": self.cfg.key_env,
            "budget_usd_total": self.cap(),
            "calls_per_hour": self.cfg.calls_per_hour,
            "max_tokens": self.cfg.max_tokens,
            "calls_this_process": self.calls,
            "refusals": self.refusals,
            "ledger": match l {
                Ok(l) => json!({"spent_usd": l.total_usd, "calls_total": l.calls, "refused_total": l.refused,
                                "input_tokens": l.input_tokens, "output_tokens": l.output_tokens,
                                "remaining_usd": (self.cap() - l.total_usd).max(0.0)}),
                Err(e) => json!({"error": e}),
            },
        })
    }
}

const SYSTEM: &str = "You are the Gaia cortex advisor. You never run commands. Reply with ONE line of JSON: {\"choice\": <one of the offered choices, verbatim>, \"reason\": <short>}. If unsure, choose \"escalate\".";

fn build_prompt(summary: &str, choices: &[Choice], max: usize) -> String {
    let c: Vec<String> = choices.iter().map(Choice::wire).collect();
    let mut s = format!(
        "Incident:\n{summary}\n\nOffered choices: {}\n",
        c.join(", ")
    );
    if s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// Cost from the OpenAI-compatible `usage` block (cached tokens priced apart).
pub fn usage_cost(v: &Value, c: &LlmConfig) -> Option<f64> {
    let u = v.get("usage")?;
    let pin = u.get("prompt_tokens")?.as_u64()? as f64;
    let pout = u.get("completion_tokens")?.as_u64()? as f64;
    let cached = u["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| u["prompt_cache_hit_tokens"].as_u64())
        .unwrap_or(0) as f64;
    let cached = cached.min(pin);
    Some(
        ((pin - cached) * c.price_in_per_m
            + cached * c.price_cached_in_per_m
            + pout * c.price_out_per_m)
            / 1e6,
    )
}

/// Map the model's text to one offered choice; anything else = escalate.
pub fn parse_choice(content: &str, offered: &[Choice]) -> (Choice, String) {
    let t = content.trim();
    let start = t.find('{');
    let end = t.rfind('}');
    let v: Value = match (start, end) {
        (Some(a), Some(b)) if b > a => serde_json::from_str(&t[a..=b]).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let reason: String = v["reason"]
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(300)
        .collect();
    let pick = v["choice"].as_str().unwrap_or("");
    match offered.iter().find(|c| c.wire() == pick) {
        Some(c) => (c.clone(), reason),
        None => (
            Choice::Escalate,
            format!(
                "advisor answer not an offered choice ({:?}); escalated",
                pick.chars().take(60).collect::<String>()
            ),
        ),
    }
}

#[cfg(feature = "llm-advisor")]
fn default_transport(data_dir: &Path) -> Box<dyn Transport> {
    Box::new(CurlTransport {
        tmp_dir: data_dir.to_path_buf(),
    })
}

#[cfg(not(feature = "llm-advisor"))]
fn default_transport(_data_dir: &Path) -> Box<dyn Transport> {
    Box::new(NotCompiled)
}

/// Was the advisor transport compiled in?
pub const COMPILED: bool = cfg!(feature = "llm-advisor");
