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
    /// Wave-4 slot 417: close an open escalation (`GET /escalations`) when
    /// the cortex incident it belongs to resolves (cleared / expired / stale
    /// / quiet). Default false = only `POST /escalations/ack` closes one.
    pub escalation_autoclose: bool,
    /// Close an open escalation this many seconds after it opened, whatever
    /// its incident does (covers `info:` subjects with no incident and
    /// incidents that never resolve). 0 (default) = never.
    pub escalation_max_age_s: u64,
    /// Cortex-side restart cap per unit per hour. Default 2 = the nerve
    /// executor cap; the nerve applies its cap only to executed restarts,
    /// so this is what bounds a dry-run (simulated) restart loop.
    pub restarts_per_unit_hour: u32,
    /// Cortex-side restart cap per hour, all units together (default 6 =
    /// the nerve executor's total cap), applied in every mode.
    pub restarts_total_hour: u32,
    /// Re-read the nerve ring (`GET /evt`) every N seconds for the failure
    /// events decided on in the last `flap_window_s`, to count the repeats
    /// the nerve folds into them (its dedupe window, 600 s on the VPS,
    /// hides a crash loop from the stream). Only while such an event is
    /// watched: idle = no calls. 0 = off. Needs `nerve_url` + `nerve_stream`.
    pub repeat_poll_s: u64,
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
    /// `/health` `nerve` turns false when the last successful nerve contact
    /// is older than this (slot 347). 0 (default) = automatic: 660 s with the
    /// stream on (nerve keepalive 300 s), else 3 x `pull_every_s` (min 30 s),
    /// else 660 s.
    pub nerve_stale_s: u64,
    /// Heal verification window (gaia-heal-verify): seconds after an
    /// executed restart within which the unit must show a held recovery,
    /// else one nerve `/state` read decides (see [`crate::verify`]).
    /// Default 180; 0 = off; else 30..=3600 (anything else refuses to start).
    pub heal_verify_s: u64,
    /// Freeze heal (gaia-freeze-heal): restart an allowlisted service that
    /// runs but stopped answering its health probe (see [`crate::freeze`]).
    /// Off by default; `CORTEX_FREEZE=1` turns it on, `0` off.
    pub freeze: crate::freeze::FreezeCfg,
    /// Failed-heal phone alert via the loopback ntfy relay
    /// (`CORTEX_ALERT_RELAY`, see [`crate::phone`]). Env only; `None` = off.
    #[serde(skip)]
    pub phone: Option<crate::phone::PhoneCfg>,
    pub llm: LlmConfig,
    /// Cortex -> Hermes boss handoff (inbox files; see `handoff`).
    pub hermes: HermesConfig,
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
            restarts_per_unit_hour: nerve_cortex_contract::NERVE_RESTARTS_PER_UNIT_HOUR,
            restarts_total_hour: nerve_cortex_contract::NERVE_RESTARTS_TOTAL_HOUR,
            repeat_poll_s: 30,
            incident_quiet_s: 1800,
            incident_sticky_max_s: 0,
            escalation_autoclose: false,
            escalation_max_age_s: 0,
            flap_failures: 3,
            flap_window_s: 900,
            circuit_open_s: 3600,
            outbox: None,
            protected_units: vec![],
            restart_allow: vec![],
            nerve_stream: true,
            nerve_stale_s: 0,
            heal_verify_s: crate::verify::DEFAULT_VERIFY_S,
            freeze: crate::freeze::FreezeCfg::default(),
            phone: None,
            llm: LlmConfig::default(),
            hermes: HermesConfig::default(),
        }
    }
}

/// Hand unresolved incidents to the Hermes boss agent (Mate, 2026-10-21
/// 22:50 +02:00). OFF by default. The cortex only writes inbox files; the
/// paid call, its $20 ceiling and Mate's approval gate live in the
/// dispatcher (`gaia-cortex hermes-dispatch`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HermesConfig {
    pub enabled: bool,
    /// Inbox dir; unset = `<data_dir>/hermes-inbox`.
    pub inbox: Option<PathBuf>,
    /// Handoffs per rolling hour (clamped to 6).
    pub per_hour: u32,
    /// Same subject + rule: one handoff per this many seconds.
    pub subject_window_s: u64,
    /// Unit-name glob patterns (`*`, `?`) never handed to Hermes, e.g. the
    /// swarm's throwaway test units. Breakers, incidents and escalation
    /// records are unchanged; see [`crate::handoff_exclude`]. Default empty.
    pub exclude_units: Vec<String>,
}

impl Default for HermesConfig {
    fn default() -> Self {
        HermesConfig {
            enabled: false,
            inbox: None,
            per_hour: 6,
            subject_window_s: 3600,
            exclude_units: vec![],
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
    /// Effective `/health` nerve staleness (see `nerve_stale_s`).
    pub fn nerve_stale_eff(&self) -> u64 {
        if self.nerve_stale_s > 0 {
            self.nerve_stale_s
        } else if self.nerve_url.is_some() && self.nerve_stream {
            crate::contact::STREAM_STALE_S
        } else if self.pull_every_s > 0 {
            (self.pull_every_s * 3).max(30)
        } else {
            crate::contact::STREAM_STALE_S
        }
    }

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
        if let Some(v) = get("CORTEX_ESCALATION_AUTOCLOSE") {
            c.escalation_autoclose = v == "1" || v == "true";
        }
        if let Some(v) = get("CORTEX_ESCALATION_MAX_AGE_S").and_then(|v| v.parse().ok()) {
            c.escalation_max_age_s = v;
        }
        if let Some(v) = get("CORTEX_NERVE_STREAM") {
            c.nerve_stream = v != "0" && v != "false";
        }
        if let Some(v) = get("CORTEX_NERVE_STALE_S").and_then(|v| v.parse().ok()) {
            c.nerve_stale_s = v;
        }
        if let Some(v) = get("CORTEX_HEAL_VERIFY_S") {
            c.heal_verify_s = v.trim().parse().map_err(|_| {
                format!("CORTEX_HEAL_VERIFY_S must be a number of seconds, got {v:?}")
            })?;
        }
        if let Some(v) = get("CORTEX_FREEZE") {
            c.freeze.enabled = match v.trim() {
                "1" | "true" => true,
                "0" | "false" | "" => false,
                _ => return Err(format!("CORTEX_FREEZE must be 0 or 1, got {v:?}")),
            };
        }
        let num = |k: &str| -> Result<Option<u64>, String> {
            match get(k) {
                Some(v) => v
                    .trim()
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("{k} must be a whole number, got {v:?}")),
                None => Ok(None),
            }
        };
        if let Some(v) = num("CORTEX_FREEZE_PROBE_S")? {
            c.freeze.probe_s = v;
        }
        if let Some(v) = num("CORTEX_FREEZE_FAILS")? {
            c.freeze.fails = u32::try_from(v).unwrap_or(u32::MAX);
        }
        if let Some(v) = num("CORTEX_FREEZE_MIN_S")? {
            c.freeze.min_s = v;
        }
        if let Some(v) = num("CORTEX_FREEZE_COOLDOWN_S")? {
            c.freeze.cooldown_s = v;
        }
        if let Some(v) = num("CORTEX_FREEZE_PER_HOUR")? {
            c.freeze.per_unit_hour = u32::try_from(v).unwrap_or(u32::MAX);
        }
        if let Some(v) = num("CORTEX_FREEZE_TIMEOUT_S")? {
            c.freeze.timeout_s = v;
        }
        if let Some(v) = get("CORTEX_FREEZE_SKIP") {
            c.freeze.skip = crate::freeze::parse_skip(&v)?;
        }
        c.phone = crate::phone::PhoneCfg::from_env(get)?;
        if let Some(v) = get("CORTEX_REPEAT_POLL_S").and_then(|v| v.parse().ok()) {
            c.repeat_poll_s = v;
        }
        if let Some(v) = get("CORTEX_OUTBOX") {
            c.outbox = Some(v.into());
        }
        if let Some(v) = get("CORTEX_LLM_ENABLED") {
            c.llm.enabled = v == "1" || v == "true";
        }
        if let Some(v) = get("CORTEX_HERMES_HANDOFF") {
            c.hermes.enabled = v == "1" || v == "true";
        }
        if let Some(v) = get("CORTEX_HERMES_INBOX") {
            c.hermes.inbox = Some(v.into());
        }
        if let Some(v) = get("CORTEX_HERMES_PER_HOUR").and_then(|v| v.parse().ok()) {
            c.hermes.per_hour = v;
        }
        if let Some(v) = get("CORTEX_HERMES_EXCLUDE_UNITS") {
            c.hermes.exclude_units = crate::handoff_exclude::parse_env(&v);
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
        let (lo, hi) = crate::verify::VERIFY_S_RANGE;
        if self.heal_verify_s != 0 && !(lo..=hi).contains(&self.heal_verify_s) {
            return Err(format!(
                "heal_verify_s must be 0 (off) or {lo}..={hi}, got {}",
                self.heal_verify_s
            ));
        }
        if self.freeze.enabled {
            self.freeze.validate()?;
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
        self.hermes.per_hour = self.hermes.per_hour.min(crate::handoff::HARD_MAX_PER_HOUR);
        crate::handoff_exclude::validate(&mut self.hermes.exclude_units)?;
        self.coalesce_ms = self.coalesce_ms.min(60_000);
        // At most one ring read per second; never more than one per window.
        if self.repeat_poll_s > 0 {
            self.repeat_poll_s = self.repeat_poll_s.clamp(1, self.flap_window_s.max(1));
        }
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
    fn freeze_skip_and_phone_relay_from_env() {
        let c = Config::load(None, &env(&[])).unwrap();
        assert!(c.freeze.skip.is_empty() && c.phone.is_none());
        let c = Config::load(
            None,
            &env(&[
                ("CORTEX_FREEZE_SKIP", "hermes-rs.service,console.service"),
                ("CORTEX_ALERT_RELAY", "http://127.0.0.1:2586/gaia-cortex"),
            ]),
        )
        .unwrap();
        assert_eq!(c.freeze.skip, vec!["hermes-rs.service", "console.service"]);
        assert_eq!(
            c.phone.as_ref().map(|p| p.path.as_str()),
            Some("/gaia-cortex")
        );
        assert!(Config::load(None, &env(&[("CORTEX_FREEZE_SKIP", "hermes-rs")])).is_err());
        assert!(Config::load(
            None,
            &env(&[("CORTEX_ALERT_RELAY", "http://192.0.2.1:2586/x")])
        )
        .is_err());
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

    #[test]
    fn escalation_autoclose_env_and_json_default_off() {
        let c = Config::load(None, &env(&[])).unwrap();
        assert!(!c.escalation_autoclose, "default: ack only");
        assert_eq!(c.escalation_max_age_s, 0, "default: never by age");
        let c = Config::load(
            None,
            &env(&[
                ("CORTEX_ESCALATION_AUTOCLOSE", "1"),
                ("CORTEX_ESCALATION_MAX_AGE_S", "86400"),
            ]),
        )
        .unwrap();
        assert!(c.escalation_autoclose);
        assert_eq!(c.escalation_max_age_s, 86_400);
        let c = Config::load(None, &env(&[("CORTEX_ESCALATION_MAX_AGE_S", "x")])).unwrap();
        assert_eq!(c.escalation_max_age_s, 0, "unparsable = unchanged");
        let c: Config =
            serde_json::from_str(r#"{"escalation_autoclose":true,"escalation_max_age_s":60}"#)
                .unwrap();
        assert!(c.escalation_autoclose);
        assert_eq!(c.escalation_max_age_s, 60);
    }

    #[test]
    fn hermes_exclude_units_env_and_guard() {
        let c = Config::load(None, &env(&[])).unwrap();
        assert!(
            c.hermes.exclude_units.is_empty(),
            "default: nothing excluded"
        );
        let c = Config::load(
            None,
            &env(&[("CORTEX_HERMES_EXCLUDE_UNITS", "sbxtest*, noloop*,,")]),
        )
        .unwrap();
        assert_eq!(c.hermes.exclude_units, vec!["noloop*", "sbxtest*"]);
        for bad in ["*", "hermes*", "galion-*", "*test*"] {
            let e = Config::load(None, &env(&[("CORTEX_HERMES_EXCLUDE_UNITS", bad)])).unwrap_err();
            assert!(e.contains("exclude_units"), "{bad}: {e}");
        }
        // JSON file form
        let mut c: Config =
            serde_json::from_str(r#"{"hermes":{"exclude_units":["noloop*"]}}"#).unwrap();
        c.validate().unwrap();
        assert_eq!(c.hermes.exclude_units, vec!["noloop*"]);
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
