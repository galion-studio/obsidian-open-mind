// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Golden comparison helpers shared by the cortex replay tests.
//!
//! The report embeds health bodies as JSON text (`[health body {...}]`). Their
//! key order depends on whether serde_json's `preserve_order` feature is on,
//! which Cargo turns on in workspace-wide builds (feature unification) but
//! not in `-p openmind-nerve` builds. JSON goldens are compared with every
//! embedded object re-serialized with sorted keys; the markdown golden is
//! picked per feature set (object iteration order also changes table rows).

use serde_json::{Map, Value};

fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut kv: Vec<(String, Value)> = m.into_iter().collect();
            kv.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                kv.into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        x => x,
    }
}

/// `s` with every embedded JSON object rewritten with sorted keys.
fn canon_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let mut it = serde_json::Deserializer::from_str(tail).into_iter::<Value>();
        match it.next() {
            Some(Ok(v @ Value::Object(_))) => {
                let n = it.byte_offset();
                out.push_str(&sorted(v).to_string());
                rest = &tail[n..];
            }
            _ => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drop the additive finding keys `kind` and `source` and canonicalize
/// embedded JSON text in every string.
pub fn canon(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.remove("kind");
            m.remove("source");
            m.values_mut().for_each(canon);
        }
        Value::Array(a) => a.iter_mut().for_each(canon),
        Value::String(s) => *s = canon_text(s),
        _ => {}
    }
}

/// True when serde_json keeps insertion order (workspace-wide builds).
pub fn preserve_order() -> bool {
    serde_json::json!({"b": 0, "a": 0})
        .to_string()
        .starts_with("{\"b\"")
}

/// The markdown golden for this build's serde_json feature set (both were
/// produced by the pre-trait binary, built `-p` and `--workspace`).
pub fn expected_md(fixtures: &std::path::Path) -> String {
    let name = if preserve_order() {
        "cortex-replay.expected.preserve-order.md"
    } else {
        "cortex-replay.expected.md"
    };
    std::fs::read_to_string(fixtures.join(name)).unwrap()
}

/// Owner evidence that nerve-wiring (8d7a924/d97dab9) adds to a listener
/// finding: `:8652 unknown owner (documented: ...) -- <process>`.
#[allow(dead_code)] // used by cortex_replay only, not real_cortex_sensor
fn is_owner_evidence(s: &str) -> bool {
    s.starts_with(':') && s[1..].starts_with(|c: char| c.is_ascii_digit()) && s.contains(" -- ")
}

/// Undo the documented, additive nerve-wiring changes in a replay report so
/// the rest can still be compared with the pre-trait golden:
/// - `gaia` on findings and incidents (their owning Gaia functions),
/// - `owners` on incidents, `exposure.ownership` (the port ownership map),
/// - the cortex's own `incidents` list next to `exposure`,
/// - owner evidence on listener findings (own lines, or `; `-joined in
///   `why` / `msg` text).
///
/// Everything else (scores, grades, findings, titles, other evidence) must
/// still match exactly.
#[allow(dead_code)]
pub fn strip_nerve_wiring(v: &mut Value) {
    match v {
        Value::Object(m) => {
            if m.contains_key("incident_cause") || m.contains_key("owners") {
                m.remove("gaia");
            }
            if m.contains_key("exposure") {
                m.remove("incidents");
            }
            m.remove("owners");
            m.remove("ownership");
            m.values_mut().for_each(strip_nerve_wiring);
        }
        Value::Array(a) => {
            a.retain(|x| !x.as_str().is_some_and(is_owner_evidence));
            a.iter_mut().for_each(strip_nerve_wiring);
        }
        Value::String(s) => *s = strip_nerve_wiring_md(s),
        _ => {}
    }
}

/// The pre-trait build printed `(last: )` when the last journal error was
/// empty; nerve-wiring drops the empty suffix. Applied to the golden only.
#[allow(dead_code)]
pub fn strip_empty_last(v: &mut Value) {
    match v {
        Value::Object(m) => m.values_mut().for_each(strip_empty_last),
        Value::Array(a) => a.iter_mut().for_each(strip_empty_last),
        Value::String(s) => *s = s.replace(" (last: )", ""),
        _ => {}
    }
}

/// Markdown form of [`strip_nerve_wiring`] + [`strip_empty_last`]: drops
/// owner evidence (own `  - :port ...` lines and `; `-joined fragments) and
/// the empty `(last: )` suffix, on both sides.
#[allow(dead_code)]
pub fn strip_nerve_wiring_md(md: &str) -> String {
    let md = md.replace(" (last: )", "");
    let mut out = String::with_capacity(md.len());
    for line in md.split_inclusive('\n') {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("- ") {
            if is_owner_evidence(rest.trim_end()) {
                continue;
            }
        }
        // `| :8652 owner -- proc; b |` and `(:8652 owner -- proc; b)`.
        let mut parts: Vec<String> = Vec::new();
        for (n, part) in line.split("; ").enumerate() {
            if n == 0 {
                let start = part.match_indices(':').map(|(i, _)| i).find(|&i| {
                    let before = &part[..i];
                    (before.is_empty() || before.ends_with("| ") || before.ends_with('('))
                        && is_owner_evidence(&part[i..])
                });
                if let (Some(i), true) = (start, line.contains("; ")) {
                    // Keep the table cell / bracket opening for the next part.
                    parts.push(part[..i].to_string());
                    continue;
                }
            } else if is_owner_evidence(part) {
                continue;
            }
            parts.push(part.to_string());
        }
        // A dropped first fragment leaves its prefix to glue onto the next.
        let mut kept = String::with_capacity(line.len());
        let mut glue = false;
        for (i, p) in parts.iter().enumerate() {
            if i > 0 && !glue {
                kept.push_str("; ");
            }
            kept.push_str(p);
            glue = p.is_empty() || p.ends_with('(') || p.ends_with('|') || p.ends_with("| ");
        }
        out.push_str(&kept);
    }
    out
}

/// Remove the per-hostname edge streams (`edge-host:*`, T2.12) the
/// pre-trait golden predates: from `cortex.ddos.streams` in a report.
/// The rest of the report must stay byte-identical.
pub fn drop_edge_hosts(report: &mut Value) {
    if let Some(a) = report
        .pointer_mut("/cortex/ddos/streams")
        .and_then(Value::as_array_mut)
    {
        a.retain(|s| {
            !s["stream"]
                .as_str()
                .is_some_and(|n| n.starts_with("edge-host:"))
        });
    }
}

/// The same for the markdown report: drop `- [level] edge-host:...` lines.
pub fn drop_edge_hosts_md(md: &str) -> String {
    md.split_inclusive('\n')
        .filter(|l| !(l.starts_with("- [") && l.contains("] edge-host:")))
        .collect()
}
