// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Escalation: what happens to one coalesced [`Batch`].
//!
//! Pluggable ([`Escalator`]). Modes (`NERVE_EVT_ESCALATE`):
//! - `delegate` (default): publish an `escalation.batch` event on the stream
//!   for the cortex, which owns the LLM advisor and the budget. Zero spend.
//! - `log`: journal line only (NoOp). Zero spend.
//! - `off`: drop.
//! - `openai`: OpenAI-compatible chat completion, DISABLED unless every slot
//!   is set: `NERVE_EVT_LLM_ENDPOINT`, `NERVE_EVT_LLM_MODEL`,
//!   `NERVE_EVT_LLM_KEY_ENV` (the NAME of the env var holding the key; the
//!   value is read at call time and never logged), prices, and a USD cap
//!   above zero. Hard caps: `NERVE_EVT_LLM_BUDGET_USD` (total, persisted across
//!   restarts in `$NERVE_DATA_DIR/evt-llm-spend.json`, worst case reserved
//!   before each call) and `NERVE_EVT_LLM_CALLS_PER_HOUR` (default 4).
//!
//! OpenRouter guard (user rule: nerve and cortex never call OpenRouter,
//! directly or via Hermes): [`endpoint_allowed`] refuses any URL containing
//! `openrouter` and any Hermes gateway (host `hermes`, port 8642), at config
//! time AND right before every call; curl runs with `--max-redirs 0` so a
//! redirect cannot reach it either. A build-time endpoint
//! (`NERVE_EVT_LLM_ENDPOINT_BUILTIN`) containing `openrouter` fails to compile.
//! Escalations are only ever triggered by a closed batch, never by a timer.

use super::coalesce::Batch;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::PathBuf;

/// Optional endpoint baked in at build time (none by default).
pub const BUILTIN_ENDPOINT: Option<&str> = option_env!("NERVE_EVT_LLM_ENDPOINT_BUILTIN");

const fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        let mut j = 0;
        while j < needle.len() && hay[i + j].to_ascii_lowercase() == needle[j] {
            j += 1;
        }
        if j == needle.len() {
            return true;
        }
        i += 1;
    }
    false
}

// Compile-time guard: a baked-in endpoint can never be OpenRouter.
const _: () = {
    if let Some(e) = BUILTIN_ENDPOINT {
        assert!(
            !contains_ci(e.as_bytes(), b"openrouter"),
            "NERVE_EVT_LLM_ENDPOINT_BUILTIN must not be OpenRouter"
        );
    }
};

/// Runtime guard for every escalation endpoint.
pub fn endpoint_allowed(url: &str) -> Result<(), String> {
    let raw = url.trim();
    // decode %xx so "openrouter%2Eai" cannot slip through
    let mut dec = String::with_capacity(raw.len());
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(x) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                dec.push(x as char);
                i += 3;
                continue;
            }
        }
        dec.push(b[i] as char);
        i += 1;
    }
    let low = dec.to_ascii_lowercase();
    if contains_ci(low.as_bytes(), b"openrouter") {
        return Err("refused: OpenRouter endpoints are forbidden for the nerve (user rule)".into());
    }
    let rest = low
        .strip_prefix("https://")
        .or_else(|| low.strip_prefix("http://"))
        .ok_or("endpoint must be http(s)://")?;
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = auth.rsplit('@').next().unwrap_or("");
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, p.parse::<u16>().ok()),
        _ => (host_port, None),
    };
    if host.is_empty() {
        return Err("endpoint has no host".into());
    }
    if host.contains("hermes") || port == Some(8642) {
        return Err(
            "refused: escalation via Hermes is forbidden (Hermes routes to OpenRouter)".into(),
        );
    }
    if low.starts_with("http://")
        && !(host == "127.0.0.1" || host == "localhost" || host == "[::1]")
    {
        return Err("plain http only to loopback".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Off,
    Log,
    Delegate,
    OpenAi,
}

#[derive(Debug, Clone)]
pub struct EscalateConfig {
    pub mode: Mode,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    /// Name of the env var with the key (never the key).
    pub key_env: Option<String>,
    pub budget_usd: f64,
    pub calls_per_hour: u32,
    pub price_in_per_mtok: Option<f64>,
    pub price_out_per_mtok: Option<f64>,
    pub max_tokens: u32,
    /// Prompt size estimate for the worst-case reservation.
    pub prompt_tokens_est: u32,
}

impl EscalateConfig {
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mode = match env("NERVE_EVT_ESCALATE").as_deref().map(str::trim) {
            None | Some("") | Some("delegate") => Mode::Delegate,
            Some("log") | Some("noop") => Mode::Log,
            Some("off") => Mode::Off,
            Some("openai") => Mode::OpenAi,
            Some(o) => {
                return Err(format!(
                    "NERVE_EVT_ESCALATE: unknown mode {o:?} (delegate|log|off|openai)"
                ))
            }
        };
        let f = |k: &str| {
            env(k)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|x| x.is_finite() && *x >= 0.0)
        };
        let endpoint = env("NERVE_EVT_LLM_ENDPOINT")
            .or_else(|| BUILTIN_ENDPOINT.map(str::to_string))
            .filter(|s| !s.trim().is_empty());
        if let Some(e) = &endpoint {
            endpoint_allowed(e)?;
        }
        let key_env = env("NERVE_EVT_LLM_KEY_ENV").filter(|s| !s.is_empty());
        if let Some(k) = &key_env {
            if !k
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                || k.len() > 64
            {
                return Err(
                    "NERVE_EVT_LLM_KEY_ENV must be an env var NAME (A-Z0-9_), not a key".into(),
                );
            }
        }
        Ok(EscalateConfig {
            mode,
            endpoint,
            model: env("NERVE_EVT_LLM_MODEL").filter(|s| !s.is_empty()),
            key_env,
            budget_usd: f("NERVE_EVT_LLM_BUDGET_USD").unwrap_or(0.0),
            calls_per_hour: env("NERVE_EVT_LLM_CALLS_PER_HOUR")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(4)
                .min(60),
            price_in_per_mtok: f("NERVE_EVT_LLM_PRICE_IN_PER_MTOK"),
            price_out_per_mtok: f("NERVE_EVT_LLM_PRICE_OUT_PER_MTOK"),
            max_tokens: env("NERVE_EVT_LLM_MAX_TOKENS")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(400)
                .clamp(16, 4000),
            prompt_tokens_est: 1500,
        })
    }

    /// Paid escalation fully configured (still subject to the caps).
    pub fn enabled(&self) -> bool {
        self.mode == Mode::OpenAi
            && self.endpoint.is_some()
            && self.model.is_some()
            && self.key_env.is_some()
            && self.budget_usd > 0.0
            && self.price_in_per_mtok.is_some()
            && self.price_out_per_mtok.is_some()
    }

    pub fn worst_case_usd(&self) -> f64 {
        let pi = self.price_in_per_mtok.unwrap_or(0.0);
        let po = self.price_out_per_mtok.unwrap_or(0.0);
        // prompt estimate x2 for safety
        (2.0 * self.prompt_tokens_est as f64 * pi + self.max_tokens as f64 * po) / 1e6
    }

    /// Secret-free description for /stats.
    pub fn to_json(&self) -> Value {
        json!({"mode": format!("{:?}", self.mode).to_lowercase(), "paid_enabled": self.enabled(),
               "endpoint_host": self.endpoint.as_deref().map(host_of), "model": self.model,
               "key_env": self.key_env, "budget_usd": self.budget_usd, "calls_per_hour": self.calls_per_hour,
               "max_tokens": self.max_tokens})
    }
}

fn host_of(u: &str) -> String {
    u.split("://")
        .nth(1)
        .unwrap_or(u)
        .split(['/', '?'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Dropped,
    Logged,
    Delegated,
    Called { usd: f64, advice: String },
    Refused(String),
    Failed(String),
}

impl Outcome {
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Dropped => "dropped",
            Outcome::Logged => "logged",
            Outcome::Delegated => "delegated",
            Outcome::Called { .. } => "called",
            Outcome::Refused(_) => "refused",
            Outcome::Failed(_) => "failed",
        }
    }
}

pub trait Escalator: Send {
    fn name(&self) -> &'static str;
    fn escalate(&mut self, b: &Batch, now: u64) -> Outcome;
}

/// NoOp: journal line only.
pub struct LogOnly;
impl Escalator for LogOnly {
    fn name(&self) -> &'static str {
        "log"
    }
    fn escalate(&mut self, b: &Batch, _now: u64) -> Outcome {
        eprintln!(
            "<4>openmind-nerve evt escalation batch {} (log-only): {}",
            b.id,
            b.summary()
        );
        Outcome::Logged
    }
}

/// Delegate to the cortex: the runtime publishes the batch on the stream.
pub struct Delegate;
impl Escalator for Delegate {
    fn name(&self) -> &'static str {
        "delegate"
    }
    fn escalate(&mut self, b: &Batch, _now: u64) -> Outcome {
        eprintln!(
            "<5>openmind-nerve evt escalation batch {} delegated to cortex: {}",
            b.id,
            b.summary()
        );
        Outcome::Delegated
    }
}

pub struct Off;
impl Escalator for Off {
    fn name(&self) -> &'static str {
        "off"
    }
    fn escalate(&mut self, _b: &Batch, _now: u64) -> Outcome {
        Outcome::Dropped
    }
}

/// A chat request as handed to the transport (the key is passed separately).
#[derive(Debug, Clone)]
pub struct ChatReq {
    pub endpoint: String,
    pub body: Value,
}

/// Transport: (request, key) -> response JSON. Production uses curl; tests use a closure.
pub type Transport = Box<dyn FnMut(&ChatReq, &str) -> Result<Value, String> + Send>;

/// Env var name -> value (production: `std::env::var`).
pub type KeyLookup = Box<dyn Fn(&str) -> Option<String> + Send>;

/// Persisted spend (survives restarts so the cap is really total).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Spend {
    pub total_usd: f64,
    pub calls: u64,
    pub refused: u64,
}

pub struct OpenAiCompat {
    pub cfg: EscalateConfig,
    pub spend: Spend,
    pub spend_path: Option<PathBuf>,
    hour: VecDeque<u64>,
    transport: Transport,
    key_lookup: KeyLookup,
}

impl OpenAiCompat {
    pub fn new(
        cfg: EscalateConfig,
        data_dir: Option<PathBuf>,
        transport: Transport,
        key_lookup: KeyLookup,
    ) -> Self {
        let spend_path = data_dir.map(|d| d.join("evt-llm-spend.json"));
        let spend = spend_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        OpenAiCompat {
            cfg,
            spend,
            spend_path,
            hour: VecDeque::new(),
            transport,
            key_lookup,
        }
    }

    fn save(&self) {
        if let Some(p) = &self.spend_path {
            let _ = crate::util::write_atomic(
                p,
                serde_json::to_string(&self.spend)
                    .unwrap_or_default()
                    .as_bytes(),
            );
        }
    }

    fn refuse(&mut self, why: String) -> Outcome {
        self.spend.refused += 1;
        self.save();
        Outcome::Refused(why)
    }

    pub fn request_body(&self, b: &Batch) -> Value {
        let events: Vec<Value> = b
            .events
            .iter()
            .map(|e| json!({"ts": e.ts, "kind": e.kind, "severity": e.severity, "unit": e.unit, "group": e.group, "count": e.count,
                            "msg": e.msg.chars().take(200).collect::<String>()}))
            .collect();
        json!({
            "model": self.cfg.model,
            "max_tokens": self.cfg.max_tokens,
            "temperature": 0.1,
            "messages": [
                {"role": "system", "content": "You are the Galion nerve advisor. Given a batch of host events, answer in <=120 words: likely cause, whether a human is needed now, and at most one remediation from: restart <allowlisted unit>, rollback <unit>, none."},
                {"role": "user", "content": json!({"batch": b.id, "summary": b.summary(), "events": events}).to_string()}
            ]
        })
    }
}

impl Escalator for OpenAiCompat {
    fn name(&self) -> &'static str {
        "openai"
    }
    fn escalate(&mut self, b: &Batch, now: u64) -> Outcome {
        if !self.cfg.enabled() {
            return self.refuse("paid escalation not configured (disabled by default)".into());
        }
        let endpoint = self.cfg.endpoint.clone().unwrap_or_default();
        if let Err(e) = endpoint_allowed(&endpoint) {
            return self.refuse(e);
        }
        while self.hour.front().is_some_and(|&t| t + 3600 <= now) {
            self.hour.pop_front();
        }
        if self.hour.len() as u32 >= self.cfg.calls_per_hour {
            return self.refuse(format!("per-hour cap {} reached", self.cfg.calls_per_hour));
        }
        let worst = self.cfg.worst_case_usd();
        if self.spend.total_usd + worst > self.cfg.budget_usd {
            return self.refuse(format!(
                "budget cap: spent {:.4} + worst case {:.4} > cap {:.2} USD",
                self.spend.total_usd, worst, self.cfg.budget_usd
            ));
        }
        let key_name = self.cfg.key_env.clone().unwrap_or_default();
        let Some(key) = (self.key_lookup)(&key_name).filter(|k| !k.is_empty()) else {
            return self.refuse(format!("key env {key_name} not set"));
        };
        let req = ChatReq {
            endpoint,
            body: self.request_body(b),
        };
        self.hour.push_back(now);
        self.spend.calls += 1;
        let res = (self.transport)(&req, &key);
        drop(key);
        let out = match res {
            Ok(v) => {
                let pin = v["usage"]["prompt_tokens"].as_f64();
                let pout = v["usage"]["completion_tokens"].as_f64();
                let usd = match (pin, pout) {
                    (Some(i), Some(o)) => {
                        (i * self.cfg.price_in_per_mtok.unwrap_or(0.0)
                            + o * self.cfg.price_out_per_mtok.unwrap_or(0.0))
                            / 1e6
                    }
                    _ => worst,
                };
                self.spend.total_usd += usd;
                let advice: String = v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(1000)
                    .collect();
                Outcome::Called { usd, advice }
            }
            Err(e) => {
                // unknown whether it was billed: charge the worst case
                self.spend.total_usd += worst;
                Outcome::Failed(e)
            }
        };
        self.save();
        out
    }
}

/// curl transport: key via stdin header (never on the command line), body via
/// stdin too, no redirects, 30 s cap.
pub fn curl_transport() -> Transport {
    Box::new(|req: &ChatReq, key: &str| {
        use std::io::Write;
        endpoint_allowed(&req.endpoint)?;
        let body = req.body.to_string();
        // -H @file reads headers from a file; use a 0600 temp file under /dev/shm-like tmp.
        let dir = std::env::temp_dir().join(format!("nerve-evt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let hp = dir.join("h");
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&hp)
                .map_err(|e| e.to_string())?;
            writeln!(f, "Authorization: Bearer {key}").map_err(|e| e.to_string())?;
            writeln!(f, "Content-Type: application/json").map_err(|e| e.to_string())?;
        }
        let child = std::process::Command::new("curl")
            .args([
                "-sS",
                "--max-time",
                "30",
                "--max-redirs",
                "0",
                "--proto",
                "=https,http",
                "-X",
                "POST",
                "-H",
            ])
            .arg(format!("@{}", hp.display()))
            .args(["--data-binary", "@-", &req.endpoint])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string());
        let out = child.and_then(|mut c| {
            if let Some(mut si) = c.stdin.take() {
                let _ = si.write_all(body.as_bytes());
            }
            c.wait_with_output().map_err(|e| e.to_string())
        });
        let _ = std::fs::remove_file(&hp);
        let _ = std::fs::remove_dir(&dir);
        let out = out?;
        if !out.status.success() {
            return Err(format!("curl exit {:?}", out.status.code()));
        }
        serde_json::from_slice::<Value>(&out.stdout).map_err(|e| format!("bad response: {e}"))
    })
}

/// Build the escalator for a config.
pub fn build(cfg: &EscalateConfig, data_dir: Option<PathBuf>) -> Box<dyn Escalator> {
    match cfg.mode {
        Mode::Off => Box::new(Off),
        Mode::Log => Box::new(LogOnly),
        Mode::Delegate => Box::new(Delegate),
        Mode::OpenAi => Box::new(OpenAiCompat::new(
            cfg.clone(),
            data_dir,
            curl_transport(),
            Box::new(|k| std::env::var(k).ok()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NerveEvent, Severity};
    use super::*;
    use std::sync::{Arc, Mutex};

    fn batch() -> Batch {
        Batch {
            id: 1,
            opened: 0,
            closed: 30,
            max_severity: Severity::High,
            events: vec![
                NerveEvent::new("journal", "unit.failed", Severity::High, "x")
                    .unit("hermes-rs.service"),
            ],
            folded: 1,
        }
    }

    fn cfg(budget: f64) -> EscalateConfig {
        let env = move |k: &str| -> Option<String> {
            Some(match k {
                "NERVE_EVT_ESCALATE" => "openai".into(),
                "NERVE_EVT_LLM_ENDPOINT" => "https://api.example.test/v1/chat/completions".into(),
                "NERVE_EVT_LLM_MODEL" => "test-model".into(),
                "NERVE_EVT_LLM_KEY_ENV" => "TEST_KEY".into(),
                "NERVE_EVT_LLM_BUDGET_USD" => budget.to_string(),
                "NERVE_EVT_LLM_PRICE_IN_PER_MTOK" => "1".into(),
                "NERVE_EVT_LLM_PRICE_OUT_PER_MTOK" => "2".into(),
                "NERVE_EVT_LLM_CALLS_PER_HOUR" => "2".into(),
                _ => return None,
            })
        };
        EscalateConfig::from_env(&env).unwrap()
    }

    #[test]
    fn openrouter_and_hermes_refused() {
        for u in [
            "https://openrouter.ai/api/v1/chat/completions",
            "https://OpenRouter.AI/api",
            "https://api.openrouter.ai/x",
            "https://openrouter%2Eai/x",
            "https://proxy.example/forward?to=openrouter.ai",
            "http://127.0.0.1:8642/v1/chat/completions",
            "https://hermes.galion.studio/v1",
            "ftp://x",
            "http://10.0.0.5/v1",
        ] {
            assert!(endpoint_allowed(u).is_err(), "{u}");
        }
        assert!(endpoint_allowed("https://api.example.test/v1/chat/completions").is_ok());
        assert!(endpoint_allowed("http://127.0.0.1:20750/v1/chat/completions").is_ok());
        let bad = |k: &str| {
            (k == "NERVE_EVT_LLM_ENDPOINT").then(|| "https://openrouter.ai/api/v1".to_string())
        };
        assert!(EscalateConfig::from_env(&bad)
            .unwrap_err()
            .contains("OpenRouter"));
        assert!(contains_ci(b"xxOpenRouteR", b"openrouter"));
    }

    #[test]
    fn disabled_by_default_and_key_name_only() {
        let c = EscalateConfig::from_env(&|_| None).unwrap();
        assert_eq!(c.mode, Mode::Delegate);
        assert!(!c.enabled());
        assert_eq!(c.budget_usd, 0.0);
        let k = |k: &str| (k == "NERVE_EVT_LLM_KEY_ENV").then(|| "sk-abc123".to_string());
        assert!(EscalateConfig::from_env(&k).is_err());
        // openai mode but missing budget -> not enabled -> refused, no transport call
        let half = |k: &str| match k {
            "NERVE_EVT_ESCALATE" => Some("openai".to_string()),
            _ => None,
        };
        let calls = Arc::new(Mutex::new(0));
        let c2 = calls.clone();
        let mut e = OpenAiCompat::new(
            EscalateConfig::from_env(&half).unwrap(),
            None,
            Box::new(move |_, _| {
                *c2.lock().unwrap() += 1;
                Ok(json!({}))
            }),
            Box::new(|_| Some("k".into())),
        );
        assert!(matches!(e.escalate(&batch(), 0), Outcome::Refused(_)));
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[test]
    fn hourly_and_budget_caps_hold() {
        let seen = Arc::new(Mutex::new(Vec::<(String, bool)>::new()));
        let s2 = seen.clone();
        let tr: Transport = Box::new(move |r, key| {
            s2.lock()
                .unwrap()
                .push((r.body.to_string(), r.body.to_string().contains(key)));
            Ok(
                json!({"choices":[{"message":{"content":"restart hermes-rs.service"}}],"usage":{"prompt_tokens":1000,"completion_tokens":100}}),
            )
        });
        let dir = std::env::temp_dir().join(format!("nerve-evt-esc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut e = OpenAiCompat::new(
            cfg(1.0),
            Some(dir.clone()),
            tr,
            Box::new(|k| (k == "TEST_KEY").then(|| "secret-k".into())),
        );
        let o1 = e.escalate(&batch(), 0);
        assert!(
            matches!(&o1, Outcome::Called { usd, .. } if (*usd - 0.0012).abs() < 1e-9),
            "{o1:?}"
        );
        assert!(matches!(e.escalate(&batch(), 10), Outcome::Called { .. }));
        // 3rd within the hour: per-hour cap 2
        assert!(matches!(e.escalate(&batch(), 20), Outcome::Refused(r) if r.contains("per-hour")));
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(
            seen.lock().unwrap().iter().all(|(_, has_key)| !has_key),
            "key must never be in the body"
        );
        // spend persisted, reloaded by a new instance
        let e2 = OpenAiCompat::new(
            cfg(1.0),
            Some(dir.clone()),
            Box::new(|_, _| Ok(json!({}))),
            Box::new(|_| None),
        );
        assert_eq!(e2.spend.calls, 2);
        // tiny budget: worst case (2*1500*1 + 400*2)/1e6 = 0.0038 > 0.001
        let mut e3 = OpenAiCompat::new(
            cfg(0.001),
            None,
            Box::new(|_, _| panic!("must not call")),
            Box::new(|_| Some("k".into())),
        );
        assert!(matches!(e3.escalate(&batch(), 0), Outcome::Refused(r) if r.contains("budget")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
