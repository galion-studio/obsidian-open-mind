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
