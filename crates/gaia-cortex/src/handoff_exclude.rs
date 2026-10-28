// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Keep throwaway test units out of the Hermes handoff (wave-4 slot 387).
//!
//! Live-watch 2026-10-28 07:08 +01:00: the swarm's own transient test units
//! `noloop370-t1..t7-20261028062409.service` crash-looped on the VPS for six
//! minutes. The cortex did the right thing (refused to restart them, opened
//! the R2 circuits, logged the incidents) but it also handed six of them to
//! the Hermes boss: six paid Opus calls, $0.42, for units that were meant to
//! fail. The same happened on 2026-10-22 01:06 +02:00 with `sbxtest-*`.
//!
//! `hermes.exclude_units` (env `CORTEX_HERMES_EXCLUDE_UNITS`, comma list) is
//! a list of glob patterns (`*` any run, `?` one char, ASCII case-insensitive)
//! matched against the escalated **unit name**. A match only skips the
//! inbox file: the decision, alert, circuit breaker, incident and escalation
//! record are all unchanged, and the escalation says `"handoff":"excluded"`
//! plus the pattern that matched. Subjects without a unit (nerve incidents,
//! thresholds, metrics) are never excluded.
//!
//! Patterns are refused at config load (the cortex will not start) when they
//! could hide a real service:
//! - they must start with at least [`MIN_LITERAL_PREFIX`] literal characters
//!   (so `*`, `*.service`, `*test*` or `g*` are refused);
//! - they must not match any production unit name in [`canaries`]: the nerve
//!   reflex allowlist and deny list, the guard's protected units, and the
//!   live VPS service list read on 2026-10-28;
//! - at most [`MAX_PATTERNS`] patterns of at most [`MAX_PATTERN_LEN`] bytes,
//!   characters `[A-Za-z0-9@._:-*?]` only.
//!
//! The default list is empty: nothing changes until the owner sets it.

use openmind_nerve::reflex;

pub const MAX_PATTERNS: usize = 16;
pub const MAX_PATTERN_LEN: usize = 96;
pub const MIN_LITERAL_PREFIX: usize = 4;

/// Real production units on the VPS (`systemctl list-units --type=service
/// --all`, read-only, 2026-10-28 13:27 +01:00) that a test-unit pattern must
/// never match, on top of the reflex and guard lists.
pub const LIVE_UNITS: &[&str] = &[
    "gaia-cortex.service",
    "gaia-hermes-handoff.service",
    "gaia-hermes-handoff.path",
    "galion-nerve.service",
    "galion-backup.service",
    "galion-desk.service",
    "galion-health.service",
    "galion-one.service",
    "galion-voice.service",
    "console.service",
    "console-rust-primary.service",
    "hermes-rs.service",
    "hermes.service",
    "memory-gateway.service",
    "openmind-memory.service",
    "openmind-queue.service",
    "link-watch.service",
    "link-watch-rs.service",
    "guardian-tick.service",
    "cloudflared.service",
    "ssh.service",
    "fail2ban.service",
    "nftables.service",
    "postfix.service",
    "exim4.service",
    "cron.service",
    "tuner.service",
    "tuner-quick.service",
    "systemd-journald.service",
    "dbus.service",
];

/// Every name a pattern is checked against.
pub fn canaries() -> Vec<String> {
    let mut v: Vec<String> = LIVE_UNITS.iter().map(|s| s.to_string()).collect();
    v.extend(reflex::ALLOW.iter().map(|s| s.to_string()));
    for frag in reflex::DENY
        .iter()
        .chain(crate::guard::PROTECTED_UNITS.iter())
    {
        let f = frag.trim_end_matches('-');
        v.push(format!("{f}.service"));
        v.push(f.to_string());
    }
    v.sort();
    v.dedup();
    v
}

/// ASCII case-insensitive glob: `*` any run (also empty), `?` exactly one
/// character. Iterative with one backtrack point: O(len(p) * len(s)).
pub fn glob_match(pat: &str, s: &str) -> bool {
    let p = pat.as_bytes();
    let s = s.as_bytes();
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, si));
            pi += 1;
        } else if pi < p.len() && (p[pi] == b'?' || p[pi].eq_ignore_ascii_case(&s[si])) {
            pi += 1;
            si += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

fn check_one(p: &str, canaries: &[String]) -> Result<(), String> {
    if p.is_empty() || p.len() > MAX_PATTERN_LEN {
        return Err(format!(
            "hermes.exclude_units: pattern {p:?} must be 1..={MAX_PATTERN_LEN} bytes"
        ));
    }
    if let Some(c) = p
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || "@._:-*?".contains(*c)))
    {
        return Err(format!(
            "hermes.exclude_units: pattern {p:?} has a forbidden character {c:?}"
        ));
    }
    let lit = p.bytes().take_while(|b| *b != b'*' && *b != b'?').count();
    if lit < MIN_LITERAL_PREFIX {
        return Err(format!(
            "hermes.exclude_units: pattern {p:?} must start with at least {MIN_LITERAL_PREFIX} literal characters"
        ));
    }
    if let Some(c) = canaries.iter().find(|c| glob_match(p, c)) {
        return Err(format!(
            "hermes.exclude_units: pattern {p:?} matches the production unit {c:?}; refusing to hide it from Hermes"
        ));
    }
    Ok(())
}

/// Validate (and trim) a pattern list. Errors name the bad pattern.
pub fn validate(pats: &mut Vec<String>) -> Result<(), String> {
    for p in pats.iter_mut() {
        *p = p.trim().to_string();
    }
    pats.retain(|p| !p.is_empty());
    if pats.len() > MAX_PATTERNS {
        return Err(format!(
            "hermes.exclude_units: at most {MAX_PATTERNS} patterns, got {}",
            pats.len()
        ));
    }
    let can = canaries();
    for p in pats.iter() {
        check_one(p, &can)?;
    }
    pats.sort();
    pats.dedup();
    Ok(())
}

/// Parse `CORTEX_HERMES_EXCLUDE_UNITS` (comma separated; empty = none).
pub fn parse_env(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The unit a handoff ticket is about: `unit`, else a `unit:` subject.
pub fn ticket_unit(ticket: &serde_json::Value) -> Option<&str> {
    ticket["unit"]
        .as_str()
        .filter(|u| !u.is_empty())
        .or_else(|| {
            ticket["subject"]
                .as_str()
                .and_then(|s| s.strip_prefix("unit:"))
        })
}

/// The first pattern matching `unit`, if any.
pub fn matching<'a>(pats: &'a [String], unit: &str) -> Option<&'a str> {
    pats.iter()
        .find(|p| glob_match(p, unit))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn glob_basics() {
        assert!(glob_match("noloop*", "noloop370-t1-20261028062409.service"));
        assert!(glob_match("NOLOOP*", "noloop370-t1.service"));
        assert!(glob_match("sbxtest*", "sbxtest.slice"));
        assert!(glob_match("noloop370-t?-*", "noloop370-t7-x.service"));
        assert!(!glob_match("noloop370-t?-*", "noloop370-t4b-x.service"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
        assert!(glob_match("abc", "abc"));
        assert!(!glob_match("abc", "abcd"));
        assert!(glob_match("ab**", "ab"));
        assert!(!glob_match("noloop*", "hermes-rs.service"));
        // pathological input stays fast (no exponential backtracking)
        let s = "a".repeat(4000);
        assert!(!glob_match(&format!("aaaa{}b", "*a".repeat(40)), &s));
    }

    #[test]
    fn good_patterns_pass_and_are_normalised() {
        let mut p = v(&[" noloop* ", "sbxtest*", "swarmtest-*", "sbxtest*", ""]);
        validate(&mut p).unwrap();
        assert_eq!(p, v(&["noloop*", "sbxtest*", "swarmtest-*"]));
    }

    #[test]
    fn dangerous_patterns_are_refused() {
        for bad in [
            "*",
            "*.service",
            "*test*",
            "g*",
            "gal*",
            "galion-*",
            "hermes*",
            "hermes-rs.service",
            "console*",
            "gaia-*",
            "openmind-*",
            "link-watch*",
            "cloudflared*",
            "ssh*",
            "systemd-*",
            "backup*",
            "tuner*",
            "noloop *",
            "noloop,x",
            "noloop/../x",
        ] {
            let r = validate(&mut v(&[bad]));
            assert!(r.is_err(), "{bad:?} must be refused");
        }
        let many: Vec<String> = (0..=MAX_PATTERNS).map(|i| format!("noloop{i}*")).collect();
        assert!(validate(&mut many.clone()).is_err());
        assert!(validate(&mut v(&[&format!("noloop{}", "x".repeat(MAX_PATTERN_LEN))])).is_err());
    }

    #[test]
    fn ticket_unit_and_matching() {
        let pats = v(&["noloop*", "sbxtest*"]);
        let t = json!({"unit": "noloop370-t2-20261028062409.service", "subject": "unit:noloop370-t2-20261028062409.service"});
        assert_eq!(ticket_unit(&t), Some("noloop370-t2-20261028062409.service"));
        assert_eq!(matching(&pats, ticket_unit(&t).unwrap()), Some("noloop*"));
        let t = json!({"unit": null, "subject": "unit:sbxtest.slice"});
        assert_eq!(matching(&pats, ticket_unit(&t).unwrap()), Some("sbxtest*"));
        let t = json!({"unit": null, "subject": "nerve-incident:ddos:edge-cloudflared"});
        assert_eq!(ticket_unit(&t), None);
        assert_eq!(matching(&pats, "hermes-rs.service"), None);
        assert_eq!(
            parse_env(" noloop*, ,sbxtest* ,"),
            v(&["noloop*", "sbxtest*"])
        );
        assert!(parse_env("").is_empty());
    }
}
