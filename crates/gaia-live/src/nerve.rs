// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Client for the loopback `openmind-nerve serve` front: `POST /event` and
//! `GET /events?n=N`. This is the only place in gaia-live that sends a POST,
//! and it only ever talks to a loopback address.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const MAX_RESP: u64 = 1024 * 1024;

fn call(addr: SocketAddr, method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    if !addr.ip().is_loopback() {
        return Err(format!("nerve address {addr} is not loopback; refusing"));
    }
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(1500))
        .map_err(|e| format!("nerve connect {addr}: {}", e.kind()))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: gaia-live/{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        env!("CARGO_PKG_VERSION"),
        body.len()
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("nerve write: {}", e.kind()))?;
    let mut buf = Vec::new();
    s.take(MAX_RESP)
        .read_to_end(&mut buf)
        .map_err(|e| format!("nerve read: {}", e.kind()))?;
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").ok_or("nerve: not HTTP")?;
    let code = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or("nerve: no status")?;
    Ok((code, body.to_string()))
}

/// Append one event. `data` must be a JSON object.
pub fn post(
    addr: SocketAddr,
    source: &str,
    kind: &str,
    severity: &str,
    data: Value,
) -> Result<(), String> {
    let body = serde_json::json!({
        "source": source,
        "kind": kind,
        "severity": severity,
        "data": data,
    })
    .to_string();
    match call(addr, "POST", "/event", &body)? {
        (200, _) => Ok(()),
        (c, b) => Err(format!("nerve POST /event: HTTP {c}: {}", b.trim())),
    }
}

/// The last `n` records (each `{ts,source,kind,severity,data}`).
pub fn tail(addr: SocketAddr, n: usize) -> Result<Vec<Value>, String> {
    match call(addr, "GET", &format!("/events?n={n}"), "")? {
        (200, b) => match serde_json::from_str::<Value>(&b) {
            Ok(Value::Array(a)) => Ok(a),
            _ => Err("nerve /events: not a JSON array".into()),
        },
        (c, b) => Err(format!("nerve GET /events: HTTP {c}: {}", b.trim())),
    }
}

/// `GET /health` is `{"status":"ok"}`.
pub fn healthy(addr: SocketAddr) -> bool {
    matches!(call(addr, "GET", "/health", ""), Ok((200, b)) if b.contains("\"status\":\"ok\""))
}

/// Convert a nerve record into GAIA's dependency-free [`gaia_core::Json`]
/// (floats are truncated to integers; GAIA has no float type).
pub fn to_gaia_json(v: &Value) -> gaia_core::Json {
    use gaia_core::Json;
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => Json::Num(
            n.as_i64()
                .unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
        ),
        Value::String(s) => Json::Str(s.clone()),
        Value::Array(a) => Json::Arr(a.iter().map(to_gaia_json).collect()),
        Value::Object(o) => Json::Obj(
            o.iter()
                .map(|(k, v)| (k.clone(), to_gaia_json(v)))
                .collect(),
        ),
    }
}

// ------------------------------------------------ nerve restarts (D-L2)

/// `GET /state`: the nerve's whole state document (the live galion-nerve
/// and `openmind-nerve serve` both answer it). Read-only.
pub fn state(addr: SocketAddr) -> Result<Value, String> {
    match call(addr, "GET", "/state", "")? {
        (200, b) => match serde_json::from_str::<Value>(&b) {
            Ok(v @ Value::Object(_)) => Ok(v),
            _ => Err("nerve /state: not a JSON object".into()),
        },
        (c, _) => Err(format!("nerve GET /state: HTTP {c}")),
    }
}

/// What the nerve says about its own restarts of one service in the last
/// hour (decision D-L2, accepted 2026-10-21: Gaia counts them, so the
/// combined Gaia + nerve total per service stays at 3 per rolling hour).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NerveRestarts {
    /// Restarts the nerve reports for the service in the last hour: the
    /// larger of the component's `restarts_1h` and the reflex budget's
    /// `live_1h` for its unit (the two describe the same restarts, so they
    /// are not added).
    pub count: usize,
    /// Name of the matched `/state` component, if any.
    pub component: Option<String>,
    /// Its systemd unit, if the nerve lists one.
    pub unit: Option<String>,
    /// The raw `restarts_1h` of the component.
    pub restarts_1h: Option<u64>,
    /// The raw `reflex.budgets[unit].live_1h`.
    pub live_1h: Option<u64>,
    /// The nerve cycle the numbers come from.
    pub cycle: Option<u64>,
}

impl NerveRestarts {
    /// Evidence for reports and nerve events (no secrets in `/state`).
    #[must_use]
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "count": self.count, "component": self.component, "unit": self.unit,
            "restarts_1h": self.restarts_1h, "reflex_live_1h": self.live_1h, "cycle": self.cycle,
        })
    }
}

/// Count the nerve's restarts of `service` in a `/state` document.
///
/// A component matches when its `name` is `service`, or its `unit` is
/// `service` or `service.service`. No match means the nerve does not watch
/// the service (every box test service): count 0. Fails closed (`Err`) when
/// `/state` has no `components` list, or a matched component's
/// `restarts_1h` / budget `live_1h` is present but not a number, so a
/// changed nerve format can never silently grant extra restarts.
pub fn restarts_for(state: &Value, service: &str) -> Result<NerveRestarts, String> {
    let comps = state
        .get("components")
        .and_then(Value::as_array)
        .ok_or("nerve /state has no components list; failing closed")?;
    let unit_name = format!("{service}.service");
    let budgets = state.pointer("/reflex/budgets").and_then(Value::as_object);
    let num = |v: Option<&Value>, what: &str| -> Result<Option<u64>, String> {
        match v {
            None | Some(Value::Null) => Ok(None),
            Some(x) => x
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("nerve /state {what} is not a count; failing closed")),
        }
    };
    let mut best = NerveRestarts {
        count: 0,
        component: None,
        unit: None,
        restarts_1h: None,
        live_1h: None,
        cycle: state.get("cycle").and_then(Value::as_u64),
    };
    for c in comps {
        let name = c.get("name").and_then(Value::as_str);
        let unit = c.get("unit").and_then(Value::as_str);
        let hit = name == Some(service) || unit == Some(service) || unit == Some(&unit_name);
        if !hit {
            continue;
        }
        let r1h = num(c.get("restarts_1h"), "restarts_1h")?;
        let live = match (unit, budgets) {
            (Some(u), Some(b)) => num(b.get(u).and_then(|x| x.get("live_1h")), "live_1h")?,
            _ => None,
        };
        let n = r1h.unwrap_or(0).max(live.unwrap_or(0));
        let n = usize::try_from(n).unwrap_or(usize::MAX);
        if best.component.is_none() || n > best.count {
            best = NerveRestarts {
                count: n,
                component: name.map(str::to_string),
                unit: unit.map(str::to_string),
                restarts_1h: r1h,
                live_1h: live,
                cycle: best.cycle,
            };
        }
    }
    // A unit budget with no component row still counts (the reflex acted).
    if best.component.is_none() {
        if let Some(b) = budgets {
            for key in [service, unit_name.as_str()] {
                if let Some(live) = num(b.get(key).and_then(|x| x.get("live_1h")), "live_1h")? {
                    let n = usize::try_from(live).unwrap_or(usize::MAX);
                    if n > best.count || best.unit.is_none() {
                        best.count = best.count.max(n);
                        best.unit = Some(key.to_string());
                        best.live_1h = Some(live);
                    }
                }
            }
        }
    }
    Ok(best)
}

/// [`state`] then [`restarts_for`].
pub fn restarts(addr: SocketAddr, service: &str) -> Result<NerveRestarts, String> {
    restarts_for(&state(addr)?, service)
}
