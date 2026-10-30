// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Output redaction for every tool result.
//!
//! Tool results come from live services (nerve, cortex, queue, Hermes
//! sessions), files (spend ledger, readiness snapshot) and notes. Before
//! anything reaches the model, this module:
//! - replaces the value of any JSON field whose *name* looks secret
//!   (`token`, `secret`, `password`, `passwd`, `cookie`, `authorization`,
//!   `api_key`, `apikey`, `private_key`, `access_key`, `signing_key`, `master_key`,
//!   `admin_key`; `session_key` is a plain id and stays) with
//!   `[REDACTED]` (numbers and booleans are kept: `"key": true` is not a secret);
//! - in every string: `Bearer <x>`, `NAME=value` with a secret-looking name,
//!   well-known token prefixes (`sk-`, `or-v1-`, `ghp_`, `gho_`,
//!   `github_pat_`, `glpat-`, `xoxb-`, `xoxp-`, `AKIA`) and long mixed
//!   base64-like runs become `[REDACTED]`;
//! - every IPv4 address that is not loopback (`127.*`) or `0.0.0.0`
//!   becomes `<ip>` (the VPS address must never leave the box).

use serde_json::{Map, Value};

const SECRET_PREFIXES: [&str; 9] = [
    "sk-",
    "or-v1-",
    "ghp_",
    "gho_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "AKIA",
];

const SECRET_NAME_PARTS: [&str; 14] = [
    "token",
    "secret",
    "password",
    "passwd",
    "cookie",
    "authorization",
    "api_key",
    "apikey",
    "private_key",
    "access_key",
    "signing_key",
    "master_key",
    "admin_key",
    "server_key",
];

/// Redact a JSON value (recursively).
#[must_use]
pub fn value(v: Value) -> Value {
    match v {
        Value::String(s) => Value::String(text(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(value).collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                let v = if secret_name(&k) && matches!(v, Value::String(_)) {
                    Value::String("[REDACTED]".into())
                } else {
                    value(v)
                };
                out.insert(k, v);
            }
            Value::Object(out)
        }
        other => other,
    }
}

/// True for a field or variable name that looks like it holds a secret.
#[must_use]
pub fn secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SECRET_NAME_PARTS.iter().any(|p| n.contains(p))
}

/// `NAME=value` names in free text: [`secret_name`], or an env-style name
/// that contains KEY, TOKEN, SECRET or PASS (any case).
fn env_secret(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    secret_name(name)
        || ["KEY", "TOKEN", "SECRET", "PASS"]
            .iter()
            .any(|p| u.contains(p))
}

/// Redact one string.
#[must_use]
pub fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_bearer = false;
    for w in s.split_inclusive(char::is_whitespace) {
        let core = w.trim_end();
        let tail = &w[core.len()..];
        if core.is_empty() {
            out.push_str(tail);
            continue;
        }
        let bare = core.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if prev_bearer && !bare.is_empty() {
            out.push_str("[REDACTED]");
        } else if let Some((name, val)) = core.split_once('=') {
            if env_secret(name.trim_start_matches(['"', '\'', '`'])) && !val.is_empty() {
                out.push_str(name);
                out.push_str("=[REDACTED]");
            } else {
                out.push_str(&word(core, bare));
            }
        } else {
            out.push_str(&word(core, bare));
        }
        out.push_str(tail);
        prev_bearer = bare.eq_ignore_ascii_case("bearer");
    }
    out
}

fn word(core: &str, bare: &str) -> String {
    let token_prefix = bare.len() >= 16 && SECRET_PREFIXES.iter().any(|p| bare.starts_with(p));
    if token_prefix || base64ish(bare) {
        return core.replace(bare, "[REDACTED]");
    }
    ipv4(core)
}

/// A long run of mixed-case letters and digits (48+ chars, no spaces) that
/// is not plain hex (commit hashes and sha256 sums stay readable).
fn base64ish(s: &str) -> bool {
    s.len() >= 48
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-'))
        && s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars().any(|c| c.is_ascii_lowercase())
        && s.chars().any(|c| c.is_ascii_digit())
        && !s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Replace non-loopback dotted IPv4 addresses with `<ip>`.
fn ipv4(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !(b[i - 1].is_ascii_digit() || b[i - 1] == b'.')) {
            if let Some((end, first)) = dotted_quad(b, i) {
                if first == 127 || &s[i..end] == "0.0.0.0" {
                    out.push_str(&s[i..end]);
                } else {
                    out.push_str("<ip>");
                }
                i = end;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Parse `a.b.c.d` (each 0..=255) at `start`; returns end index and `a`.
fn dotted_quad(b: &[u8], start: usize) -> Option<(usize, u32)> {
    let mut i = start;
    let mut first = 0;
    for part in 0..4 {
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() && i - s < 3 {
            i += 1;
        }
        if i == s {
            return None;
        }
        let n: u32 = std::str::from_utf8(&b[s..i]).ok()?.parse().ok()?;
        if n > 255 {
            return None;
        }
        if part == 0 {
            first = n;
        }
        if part < 3 {
            if i >= b.len() || b[i] != b'.' {
                return None;
            }
            i += 1;
        }
    }
    if i < b.len()
        && (b[i].is_ascii_digit() || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)))
    {
        return None;
    }
    Some((i, first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_tokens_names_and_ips() {
        assert_eq!(
            text("Authorization: Bearer abc.def"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(text("API_SERVER_KEY=hunter22"), "API_SERVER_KEY=[REDACTED]");
        assert_eq!(
            text("use sk-or-v1-0123456789abcdef now"),
            "use [REDACTED] now"
        );
        assert_eq!(text("host 203.0.113.7:22 ok"), "host <ip>:22 ok");
        assert_eq!(text("bind 127.0.0.1:8670"), "bind 127.0.0.1:8670");
        assert_eq!(text("v1.2.3 and 1.2.3.4.5"), "v1.2.3 and 1.2.3.4.5");
        let sha = "9d44c751bfc55d01036ed4ea9ed9f6292f19a75bd0b63bee0c7be9709a5cfeb9";
        assert_eq!(text(sha), sha);
        assert_eq!(
            text("2026-10-30T12:16:01+01:00"),
            "2026-10-30T12:16:01+01:00"
        );
        assert_eq!(
            text("TDAI_GATEWAY_API_KEY=x1"),
            "TDAI_GATEWAY_API_KEY=[REDACTED]"
        );
        let v = value(
            json!({"api_token":"abc","key":"incident:1","key_set":true,"session_key":"galion-one",
                             "nested":[{"password":"p","n":3}],"msg":"from 198.51.100.9"}),
        );
        assert_eq!(v["api_token"], json!("[REDACTED]"));
        assert_eq!(v["key"], json!("incident:1"));
        assert_eq!(v["key_set"], json!(true));
        assert_eq!(v["session_key"], json!("galion-one"));
        assert_eq!(v["nested"][0]["password"], json!("[REDACTED]"));
        assert_eq!(v["nested"][0]["n"], json!(3));
        assert_eq!(v["msg"], json!("from <ip>"));
    }
}
