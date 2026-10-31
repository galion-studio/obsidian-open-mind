// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Hard model lock (owner rule 2026-10-31: Hermes must not choose its own
//! model; Grok 4.7 is the fixed choice).
//!
//! With `HERMES_RS_MODEL_LOCK=x-ai/grok-4.7` (optional
//! `HERMES_RS_MODEL_LOCK_PROVIDER`, default `openrouter`) the gateway uses
//! exactly one model for every provider call, whoever asks:
//!
//! * config: `model.default`, delegation, cron, background and vision models
//!   are forced to the lock; `model_aliases` and api_server `model_routes`
//!   that point anywhere else are dropped at load (logged);
//! * API callers: a request `model` / `provider` (chat completions, runs,
//!   `POST /api/sessions/{id}/model`) that is not the lock, a generic name
//!   (`hermes-agent`, `default`, empty) or a kept route to the lock is
//!   refused with 400 `model_locked` and logged; stored session pins are
//!   ignored and rewritten to the lock;
//! * internal paths (`resolve_target`): any other name resolves to the lock
//!   (logged as forced), so delegate_task children, cron jobs with their own
//!   `model`, and vision all run on the lock;
//! * backstop: `Agent::run` and the `LockedProvider` wrapper refuse any call
//!   whose target is not the lock target, so a future code path that builds
//!   a `Target` by hand still cannot reach another model.
//!
//! The agent has no tool to change any of this: no tool takes a model
//! argument, the lock comes from the unit environment, and self-modify may
//! not change this file or remove lines that mention the lock.

use crate::provider::{ChatRequest, ChatResult, Provider, StreamEvent, Target};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const ENV: &str = "HERMES_RS_MODEL_LOCK";
pub const ENV_PROVIDER: &str = "HERMES_RS_MODEL_LOCK_PROVIDER";
pub const DEFAULT_PROVIDER: &str = "openrouter";
/// Error code returned to API callers that ask for another model.
pub const CODE: &str = "model_locked";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelLock {
    pub model: String,
    pub provider: String,
}

fn log(event: &str, msg: &str) {
    eprintln!(
        "{} hermes-gateway model-lock {event}: {msg}",
        crate::time::zagreb_rfc3339(crate::time::now_secs())
    );
    nerve_client::cap_hit("hermes-rs", event, msg);
}

impl ModelLock {
    pub fn new(model: &str, provider: &str) -> Self {
        ModelLock {
            model: model.trim().to_string(),
            provider: provider.trim().to_string(),
        }
    }

    /// Read the lock from the environment (None = no lock).
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        let model = get(ENV)
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())?;
        let provider = get(ENV_PROVIDER)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| DEFAULT_PROVIDER.to_string());
        Some(ModelLock::new(&model, &provider))
    }

    /// May a caller name this model / provider? Generic names, the locked id
    /// and route/alias names that (after `enforce`) point at the lock.
    pub fn allows_request(
        &self,
        cfg: &crate::config::Config,
        model: Option<&str>,
        provider: Option<&str>,
    ) -> Result<(), String> {
        if let Some(p) = provider.map(str::trim).filter(|p| !p.is_empty()) {
            if p != self.provider {
                return Err(format!(
                    "provider {p:?} refused: this gateway is locked to {} on {}",
                    self.model, self.provider
                ));
            }
        }
        let Some(m) = model.map(str::trim) else {
            return Ok(());
        };
        if crate::provider::is_generic_model(m) || m == self.model {
            return Ok(());
        }
        let via_route = cfg
            .model_routes
            .get(m)
            .or_else(|| cfg.aliases.get(m))
            .is_some_and(|r| r.model == self.model);
        if via_route {
            return Ok(());
        }
        Err(format!(
            "model {m:?} refused: this gateway is locked to {} (owner rule); send model \"{}\" or omit it",
            self.model, self.model
        ))
    }

    /// The single target every provider call must use.
    pub fn target(&self, cfg: &crate::config::Config, requested: &str) -> Target {
        let pc = cfg.providers.get(&self.provider).cloned();
        Target {
            provider: self.provider.clone(),
            base_url: pc
                .as_ref()
                .map(|p| p.base_url.clone())
                .unwrap_or_else(|| crate::config::DEFAULT_OPENROUTER_BASE.to_string()),
            model: self.model.clone(),
            requested: requested.to_string(),
            key_env: pc
                .as_ref()
                .and_then(|p| p.key_env.clone())
                .or_else(|| Some("OPENROUTER_API_KEY".to_string())),
            inline_key: pc.and_then(|p| p.inline_key),
        }
    }

    /// Same model, provider and endpoint as the lock target.
    pub fn target_ok(&self, cfg: &crate::config::Config, t: &Target) -> bool {
        let want = self.target(cfg, "");
        t.model == want.model && t.provider == want.provider && t.base_url == want.base_url
    }

    /// Rewrite a loaded config so nothing in it names another model.
    /// Returns one note per change (logged at startup by the caller).
    pub fn enforce(&self, cfg: &mut crate::config::Config) -> Vec<String> {
        let mut notes = vec![];
        let mut set = |what: &str, slot: &mut Option<String>, val: &str| {
            if slot.as_deref() != Some(val) {
                notes.push(format!("{what}: {:?} -> {val}", slot.as_deref()));
                *slot = Some(val.to_string());
            }
        };
        set("delegation.model", &mut cfg.delegation.model, &self.model);
        set(
            "delegation.provider",
            &mut cfg.delegation.provider,
            &self.provider,
        );
        set("cron.model", &mut cfg.cron_model, &self.model);
        set(
            "cron.model_provider",
            &mut cfg.cron_provider,
            &self.provider,
        );
        set(
            "auxiliary.openrouter_model",
            &mut cfg.background_model,
            &self.model,
        );
        if cfg.default_model != self.model {
            notes.push(format!(
                "model.default: {} -> {}",
                cfg.default_model, self.model
            ));
            cfg.default_model = self.model.clone();
        }
        if cfg.default_provider != self.provider {
            notes.push(format!(
                "model.provider: {} -> {}",
                cfg.default_provider, self.provider
            ));
            cfg.default_provider = self.provider.clone();
        }
        let keep = |r: &crate::config::Route| {
            r.model == self.model
                && r.base_url.is_none()
                && r.provider.as_deref().is_none_or(|p| p == self.provider)
        };
        for (kind, map) in [
            ("model_aliases", &mut cfg.aliases),
            ("model_routes", &mut cfg.model_routes),
        ] {
            let drop: Vec<String> = map
                .iter()
                .filter(|(_, r)| !keep(r))
                .map(|(k, r)| format!("{kind}.{k} ({}) dropped", r.model))
                .collect();
            map.retain(|_, r| keep(r));
            notes.extend(drop);
        }
        for p in cfg.providers.values_mut() {
            if p.default_model.as_deref().is_some_and(|m| m != self.model) {
                notes.push(format!(
                    "providers.{}.default_model ({}) ignored",
                    p.name,
                    p.default_model.as_deref().unwrap_or("")
                ));
                p.default_model = None;
            }
        }
        notes
    }
}

/// Log a refused caller request (API) and return the message.
pub fn refused(who: &str, msg: &str) -> String {
    log("model_lock_refused", &format!("{who}: {msg}"));
    msg.to_string()
}

/// Log an internal name that was forced to the lock.
pub fn forced(requested: &str, lock: &ModelLock) {
    log(
        "model_lock_forced",
        &format!("{requested:?} resolved to the locked {}", lock.model),
    );
}

/// Provider wrapper: refuses (never forwards) any call whose target is not
/// the lock target. Wraps the real provider in hermes-gateway main.
pub struct LockedProvider {
    pub inner: Arc<dyn Provider>,
    pub lock: ModelLock,
    pub cfg: Arc<crate::config::Config>,
}

impl Provider for LockedProvider {
    fn chat(
        &self,
        target: &Target,
        req: &ChatRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<ChatResult, String> {
        if !self.lock.target_ok(&self.cfg, target) {
            let msg = format!(
                "model lock: call to {} on {} ({}) refused; locked to {} on {}",
                target.model, target.provider, target.base_url, self.lock.model, self.lock.provider
            );
            log("model_lock_refused", &msg);
            return Err(msg);
        }
        self.inner.chat(target, req, cancel, sink)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Route};
    use crate::provider::{resolve_target, MockProvider};
    use std::path::Path;

    const LOCK: &str = "x-ai/grok-4.7";

    /// The live shape (2026-10-31): boss -> Opus, worker -> Sonnet, River
    /// aliases, galion route -> Haiku, delegation -> river-flash via env.
    fn live_cfg() -> Config {
        let mut c = Config::defaults(Path::new("/tmp"));
        c.default_model = LOCK.into();
        let r = |m: &str, p: &str, b: Option<&str>| Route {
            model: m.into(),
            provider: Some(p.into()),
            base_url: b.map(String::from),
            key_env: None,
        };
        c.aliases.insert(
            "boss".into(),
            r("anthropic/claude-opus-5.5", "openrouter", None),
        );
        c.aliases.insert(
            "worker".into(),
            r("anthropic/claude-sonnet-5.5", "openrouter", None),
        );
        c.aliases.insert(
            "river-flash".into(),
            r(
                "deepseek-ai/DeepSeek-V4.1-Flash",
                "custom",
                Some("https://api.river.ai/v1"),
            ),
        );
        c.model_routes.insert(
            "galion".into(),
            r("anthropic/claude-haiku-5.5", "openrouter", None),
        );
        c.delegation.model = Some("anthropic/claude-sonnet-5.5".into());
        c.cron_model = Some("deepseek-ai/DeepSeek-V4.1-Flash".into());
        c.cron_provider = Some("river".into());
        c
    }

    fn locked() -> Config {
        let mut c = live_cfg();
        c.apply_env(&|k| match k {
            "HERMES_RS_MODEL_LOCK" => Some(LOCK.into()),
            "HERMES_RS_DELEGATION_MODEL" => Some("river-flash".into()),
            _ => None,
        })
        .unwrap();
        c
    }

    #[test]
    fn no_env_no_lock() {
        let mut c = live_cfg();
        c.apply_env(&|_| None).unwrap();
        assert!(c.model_lock.is_none());
        assert_eq!(
            resolve_target(&c, Some("boss"), None).model,
            "anthropic/claude-opus-5.5"
        );
    }

    #[test]
    fn lock_rewrites_config_and_drops_other_aliases() {
        let c = locked();
        assert_eq!(c.model_lock, Some(ModelLock::new(LOCK, "openrouter")));
        assert_eq!(c.default_model, LOCK);
        assert_eq!(c.default_provider, "openrouter");
        assert_eq!(
            c.delegation.model.as_deref(),
            Some(LOCK),
            "river-flash env is overridden"
        );
        assert_eq!(c.cron_model.as_deref(), Some(LOCK));
        assert_eq!(c.background_model.as_deref(), Some(LOCK));
        assert!(
            c.aliases.is_empty(),
            "{:?}",
            c.aliases.keys().collect::<Vec<_>>()
        );
        assert!(c.model_routes.is_empty());
    }

    #[test]
    fn boss_and_opus_requests_are_refused() {
        let c = locked();
        let l = c.model_lock.clone().unwrap();
        for m in [
            "boss",
            "anthropic/claude-opus-5.5",
            "worker",
            "river-flash",
            "galion",
            "openrouter/auto",
        ] {
            let e = l.allows_request(&c, Some(m), None).unwrap_err();
            assert!(e.contains("refused") && e.contains(LOCK), "{m}: {e}");
        }
        assert!(l.allows_request(&c, Some(LOCK), None).is_ok());
        assert!(l.allows_request(&c, Some("hermes-agent"), None).is_ok());
        assert!(l.allows_request(&c, None, None).is_ok());
        assert!(l.allows_request(&c, None, Some("river")).is_err());
        assert!(l.allows_request(&c, Some(LOCK), Some("openrouter")).is_ok());
    }

    #[test]
    fn every_internal_name_resolves_to_the_lock() {
        let c = locked();
        for (m, p) in [
            (Some("boss"), None),
            (Some("anthropic/claude-opus-5.5"), None),
            (Some("river-flash"), None),
            (Some("deepseek-ai/DeepSeek-V4.1-Flash"), Some("river")),
            (None, Some("river")),
            (None, None),
        ] {
            let t = resolve_target(&c, m, p);
            assert_eq!(t.model, LOCK, "{m:?}/{p:?}");
            assert_eq!(t.provider, "openrouter");
            assert_eq!(t.base_url, crate::config::DEFAULT_OPENROUTER_BASE);
            assert!(c.model_lock.as_ref().unwrap().target_ok(&c, &t));
        }
    }

    #[test]
    fn locked_provider_refuses_other_targets() {
        let c = Arc::new(locked());
        let l = c.model_lock.clone().unwrap();
        let mock = Arc::new(MockProvider::echo());
        let p = LockedProvider {
            inner: mock.clone(),
            lock: l.clone(),
            cfg: c.clone(),
        };
        let cancel = AtomicBool::new(false);
        let req = ChatRequest {
            messages: vec![serde_json::json!({"role": "user", "content": "hi"})],
            ..Default::default()
        };
        let mut opus = l.target(&c, "boss");
        opus.model = "anthropic/claude-opus-5.5".into();
        let e = p.chat(&opus, &req, &cancel, &mut |_| {}).unwrap_err();
        assert!(e.contains("refused"), "{e}");
        let mut river = l.target(&c, "x");
        river.base_url = "https://api.river.ai/v1".into();
        assert!(p.chat(&river, &req, &cancel, &mut |_| {}).is_err());
        assert_eq!(
            mock.calls.lock().unwrap().len(),
            0,
            "nothing reached the provider"
        );
        assert!(p
            .chat(&l.target(&c, "hermes-agent"), &req, &cancel, &mut |_| {})
            .is_ok());
        assert_eq!(mock.calls.lock().unwrap().len(), 1);
    }
}
