// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Minimal systemd unit reader for the galion-backup unit set (slot 269).
//! Only what the tests need: `[Section]`, `Key=Value`, comments, and the
//! list-reset rule for `ExecStart=` (an empty assignment clears earlier
//! values, later files override earlier ones; drop-ins apply in name order).
#![allow(dead_code)]

/// (section, key, value) in file order; comments and blank lines dropped.
pub fn parse(text: &str) -> Vec<(String, String, String)> {
    let mut sec = String::new();
    let mut out = Vec::new();
    for raw in text.lines() {
        let l = raw.trim();
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

/// Effective value list of a list-type key (ExecStart, ExecStartPost, ...)
/// after the base unit and drop-ins (already sorted by file name) apply.
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

/// Split `systemctl cat` output into (path, body) per file.
pub fn split_cat(cat: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for l in cat.lines() {
        if let Some(p) = l.strip_prefix("# /") {
            if !p.contains(' ')
                && (p.ends_with(".service") || p.ends_with(".conf") || p.ends_with(".timer"))
            {
                out.push((format!("/{p}"), String::new()));
                continue;
            }
        }
        if let Some((_, b)) = out.last_mut() {
            b.push_str(l);
            b.push('\n');
        }
    }
    out
}
