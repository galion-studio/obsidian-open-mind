// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Model-backed agents: the opt-in OpenRouter path and the mock LLM path.
//!
//! Live is OFF unless `GALION_BENCH_LIVE_MODEL=1` AND `OPENROUTER_API_KEY` is
//! present. The key is never read by this crate: hermes-rs's curl provider
//! looks it up by name at call time and passes it to curl on stdin (never
//! argv), and redacts provider errors. Spend is capped per run.
//!
//! The mock-LLM path uses the same curl provider against a loopback
//! [`crate::mockllm::MockLlm`]; its env lookup returns nothing for every
//! name, so no key can be read even if one exists.

use crate::session::{HermesSession, Limits};
use hermes_rs::provider::{CurlProvider, EnvFn, Target};
use hermes_rs::tools::Registry;
use std::sync::Arc;

pub const LIVE_FLAG: &str = "GALION_BENCH_LIVE_MODEL";
pub const KEY_ENV: &str = "OPENROUTER_API_KEY";
pub const MODEL_ENV: &str = "GALION_BENCH_MODEL";
pub const BASE_ENV: &str = "GALION_BENCH_LLM_BASE";
pub const CAP_ENV: &str = "GALION_BENCH_USD_CAP";
/// Cheap tool-calling default; which model to benchmark is an owner decision.
pub const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";
pub const DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_USD_CAP: f64 = 0.50;
pub const DEFAULT_CALL_CAP: u64 = 800;

#[derive(Debug, Clone, PartialEq)]
pub struct LiveCfg {
    pub model: String,
    pub base_url: String,
    pub usd_cap: f64,
    pub call_cap: u64,
}

fn on(v: Option<String>) -> bool {
    v.is_some_and(|v| v.trim() == "1")
}

/// Live config, or why the live path is off. Only checks that the key
/// variable is non-empty; the value is not kept.
pub fn live_cfg(env: &dyn Fn(&str) -> Option<String>) -> Result<LiveCfg, String> {
    if !on(env(LIVE_FLAG)) {
        return Err(format!("live model path is off (set {LIVE_FLAG}=1)"));
    }
    if !env(KEY_ENV).is_some_and(|k| !k.trim().is_empty()) {
        return Err(format!("{LIVE_FLAG}=1 but {KEY_ENV} is not set; refusing"));
    }
    let usd_cap = env(CAP_ENV)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|c| *c > 0.0 && *c <= 5.0)
        .unwrap_or(DEFAULT_USD_CAP);
    Ok(LiveCfg {
        model: env(MODEL_ENV)
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.into()),
        base_url: env(BASE_ENV)
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE.into()),
        usd_cap,
        call_cap: DEFAULT_CALL_CAP,
    })
}

pub fn is_loopback_url(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = rest.split('/').next().unwrap_or("");
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

/// A Hermes session on the live model. `env` resolves the key by name.
pub fn live_session(cfg: &LiveCfg, env: EnvFn, tools: Registry, system: &str) -> HermesSession {
    let target = Target {
        provider: "openrouter".into(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        requested: cfg.model.clone(),
        key_env: Some(KEY_ENV.into()),
        inline_key: None,
    };
    let mut p = CurlProvider::new(env);
    p.max_time_s = 120;
    HermesSession::new(
        Arc::new(p),
        target,
        tools,
        system,
        Limits::live(cfg.usd_cap, cfg.call_cap),
    )
}

/// A Hermes session on a loopback mock LLM (`base_url` must be loopback).
pub fn mock_llm_session(
    base_url: &str,
    tools: Registry,
    system: &str,
) -> Result<HermesSession, String> {
    if !is_loopback_url(base_url) {
        return Err(format!("mock LLM must be loopback, got {base_url}"));
    }
    let no_env: EnvFn = Arc::new(|_k: &str| None);
    let target = Target {
        provider: "mock-llm".into(),
        base_url: base_url.into(),
        model: "hermes-bench/mock-llm".into(),
        requested: "mock-llm".into(),
        key_env: None,
        inline_key: None,
    };
    let mut p = CurlProvider::new(no_env);
    p.max_time_s = 60;
    Ok(HermesSession::new(
        Arc::new(p),
        target,
        tools,
        system,
        Limits::offline(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| m.get(k).cloned()
    }

    #[test]
    fn live_is_off_by_default_and_needs_the_key() {
        assert!(live_cfg(&env(&[])).unwrap_err().contains("off"));
        assert!(live_cfg(&env(&[(LIVE_FLAG, "0")])).is_err());
        assert!(live_cfg(&env(&[(LIVE_FLAG, "1")]))
            .unwrap_err()
            .contains("not set"));
        let c = live_cfg(&env(&[
            (LIVE_FLAG, "1"),
            (KEY_ENV, "placeholder-not-a-key"),
        ]))
        .unwrap();
        assert_eq!(c.model, DEFAULT_MODEL);
        assert_eq!(c.base_url, DEFAULT_BASE);
        assert_eq!(c.usd_cap, DEFAULT_USD_CAP);
        let c = live_cfg(&env(&[(LIVE_FLAG, "1"), (KEY_ENV, "x"), (CAP_ENV, "99")])).unwrap();
        assert_eq!(c.usd_cap, DEFAULT_USD_CAP, "cap above 5 USD is ignored");
        assert!(!format!("{c:?}").contains("placeholder"));
    }

    #[test]
    fn mock_llm_must_be_loopback() {
        assert!(mock_llm_session("https://openrouter.ai/api/v1", Registry::new(), "s").is_err());
        assert!(mock_llm_session("http://127.0.0.1:19605/v1", Registry::new(), "s").is_ok());
    }
}
