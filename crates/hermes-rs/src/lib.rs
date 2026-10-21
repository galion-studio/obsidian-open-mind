// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! hermes-rs: the Rust port of the parts of upstream `hermes-agent` 0.21.4
//! (Nous Research, MIT) that Galion actually uses.
//!
//! Portable Rust only, std threads, no async runtime, no TLS stack: HTTPS to
//! model providers goes through the system `curl` binary (key passed on
//! stdin, never argv). See docs/ws-hermes.md for the done / stubbed list.

pub mod agent;
pub mod api;
pub mod config;
pub mod cron;
pub mod delegate;
pub mod http;
pub mod mcp;
pub mod persona;
pub mod provider;
pub mod runs;
pub mod session;
#[cfg(feature = "sqlite")]
pub mod session_sqlite;
pub mod spend;
pub mod time;
pub mod tools;
pub mod yaml;

/// Version string reported on /health. `compat` is the upstream release the
/// wire format follows.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const UPSTREAM_COMPAT: &str = "0.21.4";

/// Short random-ish hex id from the clock, a counter and the pid. Not
/// cryptographic; ids only need to be unique on one node.
pub fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = t ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ ((std::process::id() as u64) << 32);
    // splitmix64 finaliser so consecutive ids do not look alike
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    format!("{prefix}{x:016x}")
}

/// Upstream `_hdr`: collapse whitespace, cap length. Used on every value put
/// into a response header (Galion patch on api_server_openai_routes.py).
pub fn hdr_value(v: &str, limit: usize) -> String {
    let s = v.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().filter(|c| !c.is_control()).take(limit).collect()
}

/// Redact things that look like secrets before text leaves the process
/// (tool previews, provider errors, cron output). Conservative: long
/// token-like runs after well-known prefixes, and `Bearer <x>`.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let words: Vec<&str> = text.split_inclusive(char::is_whitespace).collect();
    let mut prev_bearer = false;
    for w in words {
        let core = w.trim_end();
        let tail = &w[core.len()..];
        let looks_secret = prev_bearer
            || ["sk-", "or-v1-", "ghp_", "gho_", "xoxb-", "xoxp-", "AKIA"]
                .iter()
                .any(|p| {
                    core.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .starts_with(p)
                        && core.len() >= 16
                })
            || key_value_secret(core);
        if looks_secret && !core.is_empty() {
            if let Some(eq) = core.find('=') {
                out.push_str(&core[..=eq]);
                out.push_str("[REDACTED]");
            } else {
                out.push_str("[REDACTED]");
            }
        } else {
            out.push_str(core);
        }
        out.push_str(tail);
        prev_bearer = core.eq_ignore_ascii_case("bearer");
    }
    out
}

fn key_value_secret(word: &str) -> bool {
    let Some(eq) = word.find('=') else {
        return false;
    };
    let name = word[..eq].to_ascii_uppercase();
    let val = &word[eq + 1..];
    val.len() >= 8
        && ["KEY", "TOKEN", "SECRET", "PASSWORD"]
            .iter()
            .any(|k| name.ends_with(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = new_id("run_");
        let b = new_id("run_");
        assert_ne!(a, b);
        assert!(a.starts_with("run_") && a.len() == 20);
    }

    #[test]
    fn hdr_collapses_and_caps() {
        assert_eq!(hdr_value("a\n b\tc", 200), "a b c");
        assert_eq!(hdr_value(&"x".repeat(300), 200).len(), 200);
    }

    #[test]
    fn redacts_tokens() {
        let r = redact(concat!(
            "key sk",
            "-or-v1-abcdefghijklmnop and Bearer abc123 OPENROUTER_API_KEY=supersecretvalue ok"
        ));
        assert!(!r.contains("abcdefghijklmnop"));
        assert!(!r.contains("abc123"));
        assert!(!r.contains("supersecretvalue"));
        assert!(r.contains("OPENROUTER_API_KEY=[REDACTED]"));
        assert!(r.ends_with(" ok"));
    }
}
