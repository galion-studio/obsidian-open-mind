// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only status for the phone: one GET to the nerve `/state` and one to
//! the console `/api/v1/health`, both on loopback. Only names, counts, ids
//! and grades are passed on (no titles, hostnames, ports or log text), so
//! the phone page carries no infrastructure detail beyond component names.

use galion_common::http::{self, ClientRequest};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);
const MAX_STATE: usize = 4 * 1024 * 1024;

/// Reduce a nerve `/state` document to the phone summary.
pub fn summarize_nerve(state: &Value) -> Value {
    let mut counts = serde_json::Map::new();
    let mut attention = Vec::new();
    if let Some(cs) = state.get("components").and_then(Value::as_array) {
        for c in cs {
            let st = c.get("status").and_then(Value::as_str).unwrap_or("unknown");
            let n = counts.get(st).and_then(Value::as_u64).unwrap_or(0);
            counts.insert(st.to_string(), json!(n + 1));
            if st != "healthy" {
                attention.push(json!({
                    "name": c.get("name").and_then(Value::as_str).unwrap_or("?"),
                    "status": st,
                }));
            }
        }
    }
    let open: Vec<Value> = state
        .pointer("/incidents/open")
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .map(|i| {
                    json!({
                        "id": i.get("id").and_then(Value::as_str).unwrap_or("?"),
                        "severity": i.get("severity").and_then(Value::as_str).unwrap_or("?"),
                        "cause": i.get("cause").and_then(Value::as_str).unwrap_or("?"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "ok": true,
        "ts": state.get("ts").cloned().unwrap_or(Value::Null),
        "cycle": state.get("cycle").cloned().unwrap_or(Value::Null),
        "mode": state.get("mode").cloned().unwrap_or(Value::Null),
        "score": state.pointer("/self_eval/score").cloned().unwrap_or(Value::Null),
        "grade": state.pointer("/self_eval/grade").cloned().unwrap_or(Value::Null),
        "components": counts,
        "attention": attention,
        "incidents_open": open,
    })
}

fn get(addr: SocketAddr, path: &str, host: &str) -> Result<http::ClientResponse, String> {
    http::send(addr, &ClientRequest::get(path, host), TIMEOUT, MAX_STATE).map_err(|e| e.to_string())
}

/// Nerve summary, or `{"ok":false,"error":...}` (error text is the client
/// error class only).
pub fn nerve(addr: Option<SocketAddr>) -> Value {
    let Some(addr) = addr else {
        return json!({"ok": false, "error": "not configured"});
    };
    match get(addr, "/state", "127.0.0.1") {
        Ok(r) if r.status == 200 => match serde_json::from_slice::<Value>(&r.body) {
            Ok(v) => summarize_nerve(&v),
            Err(_) => json!({"ok": false, "error": "bad json"}),
        },
        Ok(r) => json!({"ok": false, "error": format!("http {}", r.status)}),
        Err(e) => json!({"ok": false, "error": e}),
    }
}

/// Console `/api/v1/health`: status code plus `status`/`implementation`.
pub fn console(addr: Option<SocketAddr>, host: &str) -> Value {
    let Some(addr) = addr else {
        return json!({"ok": false, "error": "not configured"});
    };
    match get(addr, "/api/v1/health", host) {
        Ok(r) => {
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            json!({
                "ok": r.status == 200,
                "http": r.status,
                "status": v.get("status").cloned().unwrap_or(Value::Null),
                "implementation": v.get("implementation").cloned().unwrap_or(Value::Null),
            })
        }
        Err(e) => json!({"ok": false, "error": e}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_drops_detail() {
        let s = json!({
            "ts": "2026-10-27T21:00:01+01:00", "cycle": 19, "mode": "live",
            "self_eval": {"score": 44, "grade": "F", "deductions": [{"what": "secret-ish host x.example"}]},
            "components": [
                {"name": "console", "status": "healthy", "port": 8080},
                {"name": "galion-one", "status": "failing", "port": 8091, "last_error": "dial 10.0.0.1"}
            ],
            "incidents": {"open": [{"id": "inc-1", "severity": "error", "cause": "edge", "title": "x.example down", "why": "port 3010"}]}
        });
        let out = summarize_nerve(&s);
        let text = out.to_string();
        assert_eq!(out["grade"], "F");
        assert_eq!(out["components"]["healthy"], 1);
        assert_eq!(out["attention"][0]["name"], "galion-one");
        assert_eq!(out["incidents_open"][0]["cause"], "edge");
        for leak in ["x.example", "10.0.0.1", "3010", "8091"] {
            assert!(!text.contains(leak), "{leak} leaked: {text}");
        }
    }

    #[test]
    fn unreachable_is_reported_not_panicking() {
        let a: SocketAddr = "127.0.0.1:9".parse().unwrap();
        assert_eq!(nerve(Some(a))["ok"], false);
        assert_eq!(console(None, "x")["error"], "not configured");
    }
}
