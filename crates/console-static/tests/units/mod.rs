// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Minimal systemd unit reader for the console unit set (slot 313).
//! Adapted from crates/galion-backup/tests/units/mod.rs (slot 269, same
//! project, MIT) plus backslash line continuation (the live console
//! ExecStart spans lines) and argv splitting. Only what the tests need:
//! `[Section]`, `Key=Value`, comments, continuation, and the list-reset rule
//! (an empty assignment clears earlier values; drop-ins apply in name order).
#![allow(dead_code)]

/// Join `\`-continued lines like systemd does (a trailing backslash joins
/// the next line with one space). Comment lines inside are dropped.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for raw in text.lines() {
        let l = raw.trim();
        if cur.is_some() && (l.starts_with('#') || l.starts_with(';')) {
            continue;
        }
        let (body, more) = match l.strip_suffix('\\') {
            Some(b) => (b.trim_end(), true),
            None => (l, false),
        };
        let joined = match cur.take() {
            Some(mut c) => {
                c.push(' ');
                c.push_str(body);
                c
            }
            None => body.to_string(),
        };
        if more {
            cur = Some(joined);
        } else {
            out.push(joined);
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

/// (section, key, value) in file order; comments and blank lines dropped.
pub fn parse(text: &str) -> Vec<(String, String, String)> {
    let mut sec = String::new();
    let mut out = Vec::new();
    for l in logical_lines(text) {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') {
            sec = l[1..l.len() - 1].to_string();
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            out.push((sec.clone(), k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// Effective value list of a list-type key after base + drop-ins.
pub fn effective_list(files: &[&str], section: &str, key: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for f in files {
        for (s, k, val) in parse(f) {
            if s == section && k == key {
                if val.is_empty() {
                    v.clear();
                } else {
                    v.push(val);
                }
            }
        }
    }
    v
}

/// Effective scalar (last assignment wins).
pub fn effective(files: &[&str], section: &str, key: &str) -> Option<String> {
    effective_list(files, section, key).pop()
}

/// Path-list keys (ReadWritePaths, ReadOnlyPaths) accumulate across lines.
pub fn effective_paths(files: &[&str], key: &str) -> Vec<String> {
    effective_list(files, "Service", key)
        .iter()
        .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

/// The single effective ExecStart split into argv (no quoting is used in
/// these units; a quote would be a test failure, not silently mis-split).
pub fn exec_argv(files: &[&str]) -> Vec<String> {
    let ex = effective_list(files, "Service", "ExecStart");
    assert_eq!(ex.len(), 1, "exactly one ExecStart: {ex:?}");
    assert!(!ex[0].contains('"') && !ex[0].contains('\''), "unquoted");
    ex[0].split_whitespace().map(str::to_string).collect()
}

/// Value after `--flag` in argv.
pub fn flag<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == name)
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str)
}

/// True when `path` is at or below one of `roots` (systemd path semantics).
pub fn covered(path: &str, roots: &[String]) -> bool {
    roots.iter().any(|r| {
        let r = r.trim_start_matches(['-', '+']).trim_end_matches('/');
        path == r || path.starts_with(&format!("{r}/"))
    })
}

/// Body of an as-built mirror below the `# ---- live body ----` marker.
pub fn live_body(mirror: &str) -> &str {
    let m = "# ---- live body ----\n";
    let i = mirror.find(m).expect("marker");
    &mirror[i + m.len()..]
}
