// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Egress guard: the nerve and the cortex never reach an LLM provider.
//!
//! Policy (Mate, 2026-10-21 18:10 +02:00): Hermes on the VPS is the only
//! research/LLM path. The nerve/cortex only observe. So:
//!
//! - [`LLM_ENABLED`] is `false` at compile time and nothing reads an env var
//!   to turn it on: the built-in client ([`crate::util::http`]) has no LLM
//!   path. The one direct LLM client in the crate is the optional event-mode
//!   `openai` escalator (`NERVE_EVT_ESCALATE=openai`, off by default, own
//!   budget caps); [`crate::evt::escalate::endpoint_allowed`] refuses
//!   OpenRouter (same [`FORBIDDEN`] list) and the Hermes gateway for it.
//! - Handoff, not a call (Mate t217, 2026-10-21 22:50 +02:00): incidents reach
//!   Hermes only through the cortex. The default escalation (`delegate`)
//!   publishes an `escalation.batch` event on the nerve stream / B33 feed;
//!   the cortex hands unresolved incidents to the Hermes boss through its
//!   file inbox, and `gaia-cortex hermes-dispatch` (outside this crate)
//!   makes the one Hermes call under the $20 ceiling. Hermes owns the only
//!   OpenRouter path. [`HANDOFF_PATHS`] stay allowed for that.
//! - [`GUARD_FILES`]: the only source files allowed to name OpenRouter (to
//!   refuse it); `tests/no_openrouter.rs` scans every other file.
//! - [`check_addr`]: the built-in HTTP client talks to loopback only.
//! - [`check_path`]: even on loopback it never calls an LLM / paid endpoint
//!   (chat completions, runs, responses), so it cannot spend through
//!   hermes-rs or galion-one either. Health, metrics and the nerve's own
//!   `/signal` and `/event` stay allowed.
//! - [`check_url`]: any URL handed to an external helper (curl for Telegram
//!   alerts) must be `https://api.telegram.org/`, and a URL that mentions
//!   OpenRouter is refused outright.

use std::net::SocketAddr;

/// Compile-time switch: the nerve and cortex have no LLM path. Never `true`.
pub const LLM_ENABLED: bool = false;

/// Host fragments that are always refused, case-insensitive.
pub const FORBIDDEN: &[&str] = &["openrouter.ai", "openrouter"];

/// HTTP paths of LLM / paid endpoints (hermes-rs, galion-one, OpenAI-style).
pub const LLM_PATHS: &[&str] = &[
    "/v1/chat/completions",
    "/chat/completions",
    "/v1/completions",
    "/v1/responses",
    "/v1/runs",
    "/v1/messages",
    "/api/chat",
    "/api/v1/chat",
    "/api/generate",
];

/// Source files (relative to `src/`) that may name OpenRouter, only to refuse it.
pub const GUARD_FILES: &[&str] = &["egress.rs", "evt/escalate.rs"];

/// Nerve-side handoff channels to the cortex (and from there to Hermes):
/// never refused by [`check_path`].
pub const HANDOFF_PATHS: &[&str] = &[
    "/event",
    "/signal",
    "/events",
    "/escalations",
    "/api/v1/cortex/events",
];

/// Only external URL prefix the nerve may hand to a helper process.
pub const ALLOWED_EXTERNAL: &[&str] = &["https://api.telegram.org/"];

/// Does `s` mention a forbidden provider?
pub fn mentions_forbidden(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    FORBIDDEN.iter().any(|f| l.contains(f))
}

/// The built-in client is loopback-only.
pub fn check_addr(addr: &SocketAddr) -> Result<(), String> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(format!("egress guard: {addr} is not loopback; refused"))
    }
}

/// Refuse LLM / paid endpoint paths (query string and `/p/<profile>` mirror
/// prefix ignored) and anything naming a forbidden provider.
pub fn check_path(path: &str) -> Result<(), String> {
    if mentions_forbidden(path) {
        return Err("egress guard: path names a forbidden LLM provider; refused".into());
    }
    let p = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let p = match p.strip_prefix("/p/") {
        Some(rest) => match rest.split_once('/') {
            Some((_, tail)) => format!("/{tail}"),
            None => "/".to_string(),
        },
        None => p,
    };
    let p = p.trim_end_matches('/');
    if LLM_PATHS
        .iter()
        .any(|l| p == *l || p.starts_with(&format!("{l}/")) || p.ends_with(l))
    {
        return Err(format!("egress guard: {p} is an LLM endpoint; refused"));
    }
    Ok(())
}

/// External URLs (helper processes): allowlisted prefix only, never a
/// forbidden provider.
pub fn check_url(url: &str) -> Result<(), String> {
    if mentions_forbidden(url) {
        return Err("egress guard: URL names a forbidden LLM provider; refused".into());
    }
    if ALLOWED_EXTERNAL.iter().any(|a| url.starts_with(a)) {
        Ok(())
    } else {
        Err("egress guard: external URL not on the allowlist; refused".into())
    }
}

/// Names (never values) of environment variables that point at an LLM
/// provider. The nerve ignores them; main logs a warning naming them.
pub fn llm_env_names<I: IntoIterator<Item = String>>(names: I) -> Vec<String> {
    let mut v: Vec<String> = names
        .into_iter()
        .filter(|n| {
            let u = n.to_ascii_uppercase();
            u.starts_with("OPENROUTER") || u.contains("OPENROUTER")
        })
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_is_off_at_compile_time() {
        const { assert!(!LLM_ENABLED) };
    }

    #[test]
    fn addr_loopback_only() {
        assert!(check_addr(&"127.0.0.1:8642".parse().unwrap()).is_ok());
        assert!(check_addr(&"[::1]:8642".parse().unwrap()).is_ok());
        assert!(check_addr(&"104.18.2.115:443".parse().unwrap()).is_err());
        assert!(check_addr(&"0.0.0.0:80".parse().unwrap()).is_err());
    }

    #[test]
    fn llm_paths_refused() {
        for p in [
            "/v1/chat/completions",
            "/V1/Chat/Completions?stream=true",
            "/p/default/v1/chat/completions",
            "/v1/runs",
            "/v1/runs/abc/steer",
            "/api/v1/chat/completions",
            "/v1/responses/",
            "/openrouter/x",
        ] {
            assert!(check_path(p).is_err(), "{p}");
        }
        for p in [
            "/health",
            "/v1/health",
            "/metrics",
            "/status.json",
            "/signal",
            "/event",
            "/events?n=5",
            "/healthz",
            "/api/v1/state",
        ] {
            assert!(check_path(p).is_ok(), "{p}");
        }
    }

    #[test]
    fn handoff_paths_stay_allowed() {
        for p in HANDOFF_PATHS {
            assert!(check_path(p).is_ok(), "{p}");
            assert!(!mentions_forbidden(p), "{p}");
        }
    }

    #[test]
    fn urls() {
        assert!(check_url("https://api.telegram.org/botX/sendMessage").is_ok());
        assert!(check_url("https://openrouter.ai/api/v1/chat/completions").is_err());
        assert!(check_url("https://OpenRouter.AI/api").is_err());
        assert!(check_url("https://api.telegram.org.openrouter.ai/").is_err());
        assert!(check_url("https://example.com/").is_err());
        assert!(check_url("http://api.telegram.org/").is_err());
    }

    #[test]
    fn env_names() {
        let v = llm_env_names(
            [
                "PATH",
                "OPENROUTER_API_KEY",
                "NERVE_ADDR",
                "my_openrouter_url",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        assert_eq!(v, vec!["OPENROUTER_API_KEY", "my_openrouter_url"]);
    }
}
