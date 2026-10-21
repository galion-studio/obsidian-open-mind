// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Reads the staged systemd text for `gaia heal-tick` (wave-4 slot 188,
//! deploy/staged/gaia-heal-tick@.{service,timer} + deploy/staged/gaia/*) and
//! expands `ExecStart=` the way systemd does for the few forms the unit uses:
//! `%i`, `${VAR}` (one argument) and a bare `$VAR` word (split on
//! whitespace). Anything else (quotes, other specifiers, `\` continuation)
//! is refused so the expansion cannot silently differ from systemd's.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

pub const INSTANCE: &str = "gaia-canary";

pub fn staged() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/staged")
}

pub fn read(rel: &str) -> String {
    let p = staged().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// `(section, key, value)` in file order; comments and blanks dropped.
pub fn parse_unit(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut sec = String::new();
    for (n, line) in text.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        assert!(
            !l.ends_with('\\'),
            "line {}: continuation not supported",
            n + 1
        );
        if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            sec = s.to_string();
            continue;
        }
        let (k, v) = l
            .split_once('=')
            .unwrap_or_else(|| panic!("line {}: not key=value: {l}", n + 1));
        out.push((sec.clone(), k.trim().to_string(), v.trim().to_string()));
    }
    out
}

pub fn values<'a>(u: &'a [(String, String, String)], sec: &str, key: &str) -> Vec<&'a str> {
    u.iter()
        .filter(|(s, k, _)| s == sec && k == key)
        .map(|(_, _, v)| v.as_str())
        .collect()
}

/// The single value of `key`; panics if absent or repeated.
pub fn one<'a>(u: &'a [(String, String, String)], sec: &str, key: &str) -> &'a str {
    let v = values(u, sec, key);
    assert_eq!(v.len(), 1, "[{sec}] {key}= expected once, got {v:?}");
    v[0]
}

/// `KEY=VALUE` lines of an EnvironmentFile (no quotes used in ours).
pub fn parse_env(text: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for l in text.lines().map(str::trim) {
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let (k, v) = l.split_once('=').expect("env line KEY=VALUE");
        assert!(
            !v.contains('"') && !v.contains('\''),
            "quotes are not used in the staged env file"
        );
        m.insert(k.trim().to_string(), v.trim().to_string());
    }
    m
}

/// Expand `ExecStart=` for `instance` with `env`.
pub fn expand_exec(exec: &str, instance: &str, env: &BTreeMap<String, String>) -> Vec<String> {
    assert!(
        !exec.contains('"') && !exec.contains('\''),
        "ExecStart must not need quote handling"
    );
    let mut out = Vec::new();
    for w in exec.split_whitespace() {
        let w = w.replace("%i", instance);
        assert!(!w.contains('%'), "unsupported specifier in {w}");
        if let Some(var) = w.strip_prefix('$').filter(|v| !v.starts_with('{')) {
            let v = env
                .get(var)
                .unwrap_or_else(|| panic!("${var} not in env file"));
            out.extend(v.split_whitespace().map(str::to_string));
            continue;
        }
        let mut s = w.clone();
        while let Some(i) = s.find("${") {
            let j = s[i..].find('}').expect("closing }") + i;
            let var = &s[i + 2..j];
            let v = env
                .get(var)
                .unwrap_or_else(|| panic!("${{{var}}} not in env file"));
            s = format!("{}{}{}", &s[..i], v, &s[j + 1..]);
        }
        assert!(!s.contains('$'), "unexpanded $ in {s}");
        out.push(s);
    }
    out
}

/// Full argv of the canary instance as systemd would run it.
pub fn canary_argv() -> Vec<String> {
    let svc = parse_unit(&read("gaia-heal-tick@.service"));
    let env = parse_env(&read("gaia/heal-gaia-canary.env"));
    expand_exec(one(&svc, "Service", "ExecStart"), INSTANCE, &env)
}

/// Value after `flag` in `argv` (before `--`).
pub fn flag<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    let end = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
    argv[..end]
        .windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}
