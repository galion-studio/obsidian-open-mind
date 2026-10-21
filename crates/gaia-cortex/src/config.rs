// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex configuration: a JSON file (`--config PATH` / `CORTEX_CONFIG`),
//! then `CORTEX_*` env overrides. Every default is the safe one: dry-run
//! actions, LLM advisor disabled, loopback only.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Loopback listen address of the cortex API.
    pub listen: String,
    /// Optional unix socket for NDJSON event streams.
    pub socket: Option<PathBuf>,
    /// State dir: decisions.jsonl, alerts.jsonl, escalations.jsonl, llm-spend.json.
    pub data_dir: PathBuf,
    /// Nerve base URL (loopback http). Unset = no nerve link: actions are
    /// simulated locally and alerts stay in the cortex's own files.
    pub nerve_url: Option<String>,
    /// Pull adapter: poll `GET <nerve_url>/events` every N seconds (0 = off,
    /// the default; push or socket is the zero-idle path).
    pub pull_every_s: u64,
    /// `"dry-run"` (default) or `"live"`. Live also needs the nerve live.
    pub actions: String,
    /// Wait this long after the first event of a burst before deciding.
    pub coalesce_ms: u64,
    /// Same unit, same rule: one decision per this many seconds.
    pub decision_cooldown_s: u64,
    /// One-shot subjects (process exits, task results, unclassified
    /// processes, other events) carry no "cleared" signal: their incident
    /// resolves after this many seconds without a new event. Units, nerve
    /// incidents and thresholds resolve only when their own signal clears
    /// (T5.1 F3: never because other subjects had bursts).
    pub incident_quiet_s: u64,
    /// Units, nerve incidents and thresholds whose own "cleared" event never
    /// comes (the nerve publishes start/stop only for lifecycle-tracked
    /// groups; a postfix or ssh recovery never reaches the cortex) resolve as
    /// `stale` after this many seconds without an event of their own.
    /// 0 (default) = never: they wait for their own signal.
    pub incident_sticky_max_s: u64,
    /// Cortex-side restart cap per unit per hour (the nerve has its own).
    pub restarts_per_unit_hour: u32,
    /// Failures of one unit within `flap_window_s` that open the circuit.
    pub flap_failures: u32,
    pub flap_window_s: u64,
    /// How long an open circuit stays open.
    pub circuit_open_s: u64,
    /// Optional outbox file: one JSON line per alert for a mail/chat relay.
    pub outbox: Option<PathBuf>,
    /// Extra protected units (substring match), on top of the built-in list.
    pub protected_units: Vec<String>,
    /// Units the cortex may ask to restart. Empty = the nerve reflex
    /// allowlist (`openmind_nerve::reflex::ALLOW`) minus protected units.
    pub restart_allow: Vec<String>,
    /// Subscribe to `GET <nerve_url>/events/stream` (event-mode nerve). Default on
    /// when `nerve_url` is set; this is the zero-idle-work transport.
    pub nerve_stream: bool,
    pub llm: LlmConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "127.0.0.1:8674".into(),
            socket: None,
            data_dir: PathBuf::from("/var/lib/gaia-cortex"),
            nerve_url: None,
            pull_every_s: 0,
            actions: "dry-run".into(),
            coalesce_ms: 1500,
            decision_cooldown_s: 120,
            incident_quiet_s: 1800,
            incident_sticky_max_s: 0,
            restarts_per_unit_hour: 3,
            flap_failures: 3,
            flap_window_s: 900,
            circuit_open_s: 3600,
            outbox: None,
            protected_units: vec![],
            restart_allow: vec![],
            nerve_stream: true,
            llm: LlmConfig::default(),
        }
    }
}

/// OpenAI-compatible advisor. DISABLED by default; never holds a key, only
/// the NAME of the env var that does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmConfig {
    pub enabled: bool,
    /// Base URL; `/chat/completions` is appended.
    pub endpoint: String,
    pub model: String,
    /// Name of the env var holding the key (never the key).
    pub key_env: String,
    /// Hard lifetime cap in USD (persisted ledger; refuse beyond).
    pub budget_usd_total: f64,
    pub calls_per_hour: u32,
    pub max_tokens: u32,
    /// Prompt is cut to this many characters.
    pub max_prompt_chars: usize,
    pub timeout_s: u64,
    /// USD per 1M tokens.
    pub price_in_per_m: f64,
    pub price_cached_in_per_m: f64,
    pub price_out_per_m: f64,
    /// Only incidents at or above this severity may be sent.
    pub min_severity: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        // River AI (verified by the parent 2026-10-21 18:16 +02:00):
        // OpenAI-compatible, model DeepSeek-V4.1-Flash, $0.12/M in,
        // $0.0024/M cached in, $0.48/M out. Key lives in RIVER_API_KEY.
        LlmConfig {
            enabled: false,
            endpoint: "https://api.river.ai/v1".into(),
            model: "deepseek-ai/DeepSeek-V4.1-Flash".into(),
            key_env: "RIVER_API_KEY".into(),
            budget_usd_total: 15.0,
            calls_per_hour: 6,
            max_tokens: 400,
            max_prompt_chars: 6000,
            timeout_s: 30,
            price_in_per_m: 0.12,
            price_cached_in_per_m: 0.0024,
            price_out_per_m: 0.48,
            min_severity: "error".into(),
        }
    }
}

/// Never above this, whatever the config says.
pub const HARD_BUDGET_CEILING_USD: f64 = 15.0;

impl Config {
    pub fn dry_run(&self) -> bool {
        self.actions != "live"
    }

    /// Load the JSON file (if any) and apply env overrides.
    pub fn load(
        path: Option<&str>,
        get: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Config, String> {
        let mut c = match path.map(str::to_string).or_else(|| get("CORTEX_CONFIG")) {
            Some(p) => {
                let t = std::fs::read_to_string(&p).map_err(|e| format!("config {p}: {e}"))?;
                serde_json::from_str(&t).map_err(|e| format!("config {p}: {e}"))?
            }
            None => Config::default(),
        };
        if let Some(v) = get("CORTEX_LISTEN") {
            c.listen = v;
        }
        if let Some(v) = get("CORTEX_SOCKET") {
            c.socket = Some(v.into());
        }
        if let Some(v) = get("CORTEX_DATA_DIR") {
            c.data_dir = v.into();
        }
        if let Some(v) = get("CORTEX_NERVE_URL") {
            c.nerve_url = Some(v).filter(|s| !s.is_empty());
        }
        if let Some(v) = get("CORTEX_PULL_EVERY_S").and_then(|v| v.parse().ok()) {
            c.pull_every_s = v;
        }
        if let Some(v) = get("CORTEX_ACTIONS") {
            c.actions = v;
        }
        if let Some(v) = get("CORTEX_COALESCE_MS").and_then(|v| v.parse().ok()) {
            c.coalesce_ms = v;
        }
        if let Some(v) = get("CORTEX_INCIDENT_QUIET_S").and_then(|v| v.parse().ok()) {
            c.incident_quiet_s = v;
        }
        if let Some(v) = get("CORTEX_INCIDENT_STICKY_MAX_S").and_then(|v| v.parse().ok()) {
            c.incident_sticky_max_s = v;
        }
        if let Some(v) = get("CORTEX_NERVE_STREAM") {
            c.nerve_stream = v != "0" && v != "false";
        }
        if let Some(v) = get("CORTEX_OUTBOX") {
            c.outbox = Some(v.into());
        }
        if let Some(v) = get("CORTEX_LLM_ENABLED") {
            c.llm.enabled = v == "1" || v == "true";
        }
        c.validate()?;
        Ok(c)
    }

    pub fn validate(&mut self) -> Result<(), String> {
        if self.actions != "dry-run" && self.actions != "live" {
            return Err(format!(
                "actions must be dry-run or live, not {:?}",
                self.actions
            ));
        }
        let addr: std::net::SocketAddr = self
            .listen
            .parse()
            .map_err(|e| format!("listen {}: {e}", self.listen))?;
        if !addr.ip().is_loopback() {
            return Err("listen must be a loopback address".into());
        }
        if let Some(u) = &self.nerve_url {
            if nerve_cortex_addr(u).is_none() {
                return Err(format!("nerve_url must be http://<loopback>:port, got {u}"));
            }
        }
        if !(self.llm.budget_usd_total.is_finite() && self.llm.budget_usd_total >= 0.0) {
            return Err("llm.budget_usd_total must be >= 0".into());
        }
        // Never more than the owner's $15 cap.
        self.llm.budget_usd_total = self.llm.budget_usd_total.min(HARD_BUDGET_CEILING_USD);
        if self.llm.key_env.is_empty()
            || !self
                .llm
                .key_env
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("llm.key_env must be an env var NAME like RIVER_API_KEY".into());
        }
        if self.llm.enabled && !crate::llm::COMPILED {
            return Err("llm.enabled needs a build with the llm-advisor cargo feature (off by default; needs Mate's approval)".into());
        }
        if let Some(r) = crate::llm::endpoint_refusal(&self.llm.endpoint) {
            if self.llm.enabled {
                return Err(format!("llm.endpoint refused: {r}"));
            }
        }
        self.coalesce_ms = self.coalesce_ms.min(60_000);
        self.incident_quiet_s = self.incident_quiet_s.clamp(1, 7 * 86_400);
        Ok(())
    }
}

/// `http://127.0.0.1:8673[/...]` -> socket address, loopback only.
pub fn nerve_cortex_addr(url: &str) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    let rest = url.trim().strip_prefix("http://")?;
    let hp = rest.split('/').next()?;
    let a = hp.to_socket_addrs().ok()?.next()?;
    a.ip().is_loopback().then_some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let v: Vec<(String, String)> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        move |k| v.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone())
    }

    #[test]
    fn defaults_are_safe() {
        let c = Config::load(None, &env(&[])).unwrap();
        assert!(c.dry_run());
        assert!(!c.llm.enabled);
        assert_eq!(c.llm.key_env, "RIVER_API_KEY");
        assert_eq!(c.llm.budget_usd_total, 15.0);
        assert_eq!(c.llm.calls_per_hour, 6);
        assert!(!c.llm.endpoint.contains("openrouter"));
    }

    #[test]
    fn budget_clamped_and_bad_values_refused() {
        let mut c = Config::default();
        c.llm.budget_usd_total = 500.0;
        c.validate().unwrap();
        assert_eq!(c.llm.budget_usd_total, 15.0);
        let mut c = Config {
            listen: "0.0.0.0:8674".into(),
            ..Config::default()
        };
        assert!(c.validate().is_err());
        let mut c = Config {
            actions: "yolo".into(),
            ..Config::default()
        };
        assert!(c.validate().is_err());
        let mut c = Config {
            nerve_url: Some("http://10.0.0.1:8670".into()),
            ..Config::default()
        };
        assert!(c.validate().is_err());
        let mut c = Config::default();
        c.llm.key_env = "sk-live-abc".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn env_overrides() {
        let c = Config::load(
            None,
            &env(&[
                ("CORTEX_LISTEN", "127.0.0.1:20791"),
                ("CORTEX_ACTIONS", "live"),
            ]),
        )
        .unwrap();
        assert_eq!(c.listen, "127.0.0.1:20791");
        assert!(!c.dry_run());
    }
}

#[cfg(test)]
mod llm_gate {
    use super::*;

    #[test]
    fn enabling_needs_the_feature_and_a_clean_endpoint() {
        let mut c = Config::default();
        c.llm.enabled = true;
        let r = c.validate();
        if crate::llm::COMPILED {
            assert!(r.is_ok());
            c.llm.endpoint = "https://openrouter.ai/api/v1".into();
            assert!(c.validate().is_err());
        } else {
            assert!(r.unwrap_err().contains("llm-advisor"));
        }
    }
}
