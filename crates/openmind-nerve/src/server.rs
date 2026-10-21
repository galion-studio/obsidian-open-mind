// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Loopback HTTP API: one JSON API plus the dashboard. std only.
//!
//! JSON API (`/api/v1`):
//! - `GET /api/v1/state`      everything: system, incidents, cortex, self-eval, components
//! - `GET /api/v1/incidents`  open + recently resolved incidents
//! - `GET /api/v1/cortex`     vulnerabilities, bleeding, ddos, exposure
//! - `GET /api/v1/ddos`       per-stream request-rate verdicts
//! - `GET /api/v1/self-eval`  the scored report (`.md` for markdown)
//! - `GET /api/v1/signals` `/api/v1/alerts` `/api/v1/audit`
//! - `POST /api/v1/signal`    push one signal (nerve-client wire format)
//!
//! Compatibility: the routes of the live galion-nerve (`/state`,
//! `/self-eval(.md)`, `/alerts`, `/audit`, `/signals`, `/healthz`,
//! `POST /signal`) and of swarm/gaia-live's `openmind-nerve serve`
//! (`GET /health`, `POST /event`, `GET /events?n=N`) answer the same way.
//!
//! Every request is also counted in the `nerve-api` request stream, so a
//! flood against the nerve itself shows up in its own DDoS detector.

use crate::access::Hit;
use crate::engine::Shared;
use crate::hub::{parse_signal, Signal, MAX_BODY};
use crate::util::now_secs;
use crate::{read_lossy, Event, Writer};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_CONNS: usize = 16;
const MAX_EVENTS_TAIL: usize = 1000;
/// Stack of one connection thread. serde_json parses up to 128 nested
/// levels recursively; measured need for that (T3.1, slot 212): release
/// 64-96 KiB, debug 256-384 KiB with serde_json `preserve_order` (the
/// workspace feature set). 256 KiB overflowed (process abort) in the debug
/// workspace build; 512 KiB covers every build. Pages are only touched
/// when used, so the 16 threads cost no more RSS than before.
pub const CONN_STACK: usize = 512 * 1024;

/// The JSONL event log behind `POST /event` / `GET /events`.
pub struct EventLog {
    pub writer: Writer,
    pub path: PathBuf,
    pub appended: u64,
}

pub struct Req {
    pub method: String,
    pub path: String,
    pub query: String,
    pub body: String,
}

/// Largest accepted request head, the blank line (`\r\n\r\n`) included.
pub const MAX_HEAD: usize = 8192;

/// Read one request (head <= [`MAX_HEAD`] bytes, body <= MAX_BODY).
///
/// The head limit is judged on the head itself, not on how much happened
/// to arrive in one `read`: before T3.1 (slot 212 fuzz find) a head of
/// 8193..10240 bytes was accepted when it came in large segments and
/// refused with 431 when it came in small ones.
pub fn read_req(s: &mut impl Read) -> Result<Req, (u16, &'static str)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        let n = s.read(&mut chunk).map_err(|_| (400, "read error"))?;
        if n == 0 {
            return Err((400, "incomplete request"));
        }
        // Rescan only the new bytes (plus 3 for a terminator split across reads).
        let from = buf.len().saturating_sub(3);
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf[from..].windows(4).position(|w| w == b"\r\n\r\n") {
            let p = from + p;
            if p + 4 > MAX_HEAD {
                return Err((431, "headers too large"));
            }
            break p;
        }
        if buf.len() >= MAX_HEAD {
            return Err((431, "headers too large"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or("");
    let mut it = first.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let target = it.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let len: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err((413, "body too large"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut chunk).map_err(|_| (400, "read error"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Ok(Req {
        method,
        path: path.to_string(),
        query: query.to_string(),
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// One `write_all` per reply: `write!` on the raw socket sent the status
/// line in several small segments (T3.6: clients that read once saw
/// `HTTP/1.1 ` without a code).
fn write_resp(s: &mut TcpStream, code: u16, ctype: &str, body: &str) {
    let msg = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        reason(code),
        body.len()
    );
    let _ = s.write_all(msg.as_bytes());
}

/// `n` from `n=N`, clamped to 1..=1000 (default 50).
pub fn tail_n(query: &str) -> usize {
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("n="))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, MAX_EVENTS_TAIL)
}

/// gaia-live's event body: `{"source","kind","severity","data"}`.
pub fn event_from(body: &str) -> Result<Event, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;
    let field = |k: &str| -> Result<String, String> {
        match v.get(k).and_then(Value::as_str) {
            Some(s) if !s.is_empty() && s.len() <= 128 => Ok(s.to_string()),
            _ => Err(format!(
                "`{k}` must be a non-empty string of at most 128 bytes"
            )),
        }
    };
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    Event::new(
        field("source")?,
        field("kind")?,
        field("severity")?,
        &data.to_string(),
    )
    .map_err(|e| e.to_string())
}

fn record_json(e: &Event) -> Value {
    json!({"ts": e.ts(), "source": e.source(), "kind": e.kind(), "severity": e.severity(),
           "data": serde_json::from_str::<Value>(&e.data_json()).unwrap_or(Value::Null)})
}

/// An event as a hub signal, so events feed the same correlation path.
fn event_signal(e: &Event, now: u64) -> Option<Signal> {
    let src: String = e
        .source()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let kind = match e.severity() {
        "error" | "err" | "crit" | "critical" | "fatal" => "error",
        _ => "event",
    };
    Some(Signal {
        t: now,
        source: src,
        kind: kind.into(),
        name: e.kind().chars().take(64).collect(),
        value: None,
        msg: e.data_json().chars().take(300).collect(),
    })
}

/// Route a request against the shared state: (status, content type, body).
pub fn route(
    req: &Req,
    sh: &Arc<Mutex<Shared>>,
    ev: &Option<Arc<Mutex<EventLog>>>,
) -> (u16, &'static str, String) {
    let j = "application/json";
    let md = "text/markdown; charset=utf-8";
    let p = req
        .path
        .strip_prefix("/api/v1")
        .filter(|r| !r.is_empty())
        .unwrap_or(&req.path);
    let api = req.path.starts_with("/api/v1/");
    match (req.method.as_str(), p) {
        ("POST", "/event") => {
            let Some(ev) = ev else {
                return (
                    404,
                    j,
                    r#"{"ok":false,"error":"no event log configured (--log / NERVE_EVENT_LOG)"}"#
                        .into(),
                );
            };
            match event_from(&req.body) {
                Ok(e) => {
                    let mut l = ev.lock().unwrap_or_else(|x| x.into_inner());
                    match l.writer.append(&e) {
                        Ok(()) => {
                            l.appended += 1;
                            let n = l.appended;
                            drop(l);
                            if let Some(s) = event_signal(&e, now_secs()) {
                                let _ = sh.lock().unwrap_or_else(|x| x.into_inner()).hub.ingest(s);
                            }
                            (200, j, format!("{{\"ok\":true,\"appended\":{n}}}"))
                        }
                        Err(e) => (
                            500,
                            j,
                            json!({"ok": false, "error": e.to_string()}).to_string(),
                        ),
                    }
                }
                Err(e) => (400, j, json!({"ok": false, "error": e}).to_string()),
            }
        }
        ("GET", "/events") => {
            let Some(ev) = ev else {
                return (200, j, "[]".into());
            };
            let path = ev.lock().unwrap_or_else(|x| x.into_inner()).path.clone();
            let n = tail_n(&req.query);
            match read_lossy(&path) {
                Ok((all, skipped)) => {
                    if skipped > 0 {
                        eprintln!("<4>openmind-nerve: GET /events skipped {skipped} unreadable line(s) in {}", path.display());
                    }
                    let from = all.len().saturating_sub(n);
                    (
                        200,
                        j,
                        Value::Array(all[from..].iter().map(record_json).collect()).to_string(),
                    )
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (200, j, "[]".into()),
                Err(e) => (
                    500,
                    j,
                    json!({"ok": false, "error": e.to_string()}).to_string(),
                ),
            }
        }
        (_, "/event" | "/events") => (
            405,
            j,
            r#"{"ok":false,"error":"method not allowed"}"#.into(),
        ),
        _ => {
            let mut g = sh.lock().unwrap_or_else(|e| e.into_inner());
            match (req.method.as_str(), p) {
                ("GET", "/state") | ("GET", "/state.json") => (200, j, g.state.to_string()),
                ("GET", "/incidents") => (200, j, g.state["incidents"].to_string()),
                ("GET", "/cortex") => (200, j, g.state["cortex"].to_string()),
                ("GET", "/ddos") => (200, j, g.state["cortex"]["ddos"].to_string()),
                ("GET", "/self-eval") | ("GET", "/self-eval.json") => {
                    (200, j, g.report.to_string())
                }
                ("GET", "/self-eval.md") => (200, md, g.report_md.clone()),
                ("GET", "/alerts") => (200, j, g.alerts.to_string()),
                ("GET", "/audit") => (200, j, g.audit.to_string()),
                ("GET", "/signals") => (200, j, g.hub.to_json(now_secs()).to_string()),
                ("GET", "/healthz") if !api => (200, "text/plain", "ok\n".into()),
                ("GET", "/health") => {
                    let appended = ev
                        .as_ref()
                        .map(|e| e.lock().unwrap_or_else(|x| x.into_inner()).appended)
                        .unwrap_or(0);
                    (200, j, json!({"status": "ok", "service": "openmind-nerve", "version": env!("CARGO_PKG_VERSION"),
                        "appended": appended, "ts": zagreb_time::now_rfc3339(),
                        "system": g.state["system"]["status"], "score": g.state["system"]["score"],
                        "cycle": g.state["cycle"]}).to_string())
                }
                ("GET", "/") if !api => (200, "text/html; charset=utf-8", dashboard(&g.state)),
                ("POST", "/signal") => match parse_signal(&req.body, now_secs()) {
                    Ok(s) => match g.hub.ingest(s) {
                        Ok(()) => (202, j, r#"{"ok":true}"#.into()),
                        Err(e) => (429, j, json!({"ok": false, "error": e}).to_string()),
                    },
                    Err(e) => (400, j, json!({"ok": false, "error": e}).to_string()),
                },
                (_, "/signal") => (405, j, r#"{"error":"POST only"}"#.into()),
                _ => (404, "text/plain", "not found\n".into()),
            }
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn findings_html(arr: &Value) -> String {
    let mut out = String::new();
    for f in arr.as_array().into_iter().flatten() {
        let ev: Vec<String> = f["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(esc))
            .collect();
        out.push_str(&format!(
            "<li class=\"{}\"><b>{}</b> {}: {}<br><small>{}</small>{}</li>",
            esc(f["severity"].as_str().unwrap_or("")),
            esc(f["severity"].as_str().unwrap_or("")),
            esc(f["component"].as_str().unwrap_or("")),
            esc(f["title"].as_str().unwrap_or("")),
            ev.join("<br>"),
            f["fix"]
                .as_str()
                .map(|x| format!("<br><small>fix: {}</small>", esc(x)))
                .unwrap_or_default()
        ));
    }
    if out.is_empty() {
        out.push_str("<li>none</li>");
    }
    out
}

pub fn dashboard(state: &Value) -> String {
    let mut rows = String::new();
    for c in state["components"].as_array().into_iter().flatten() {
        let st = c["status"].as_str().unwrap_or("?");
        let v = |k: &str| match &c[k] {
            Value::Null => "-".to_string(),
            Value::String(s) => esc(s),
            o => esc(&o.to_string()),
        };
        let mem = c["mem_kb"]
            .as_u64()
            .map(crate::util::kib)
            .unwrap_or("-".into());
        let reasons: Vec<String> = c["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(esc))
            .collect();
        rows.push_str(&format!(
            "<tr class=\"{st}\"><td>{}</td><td>{}</td><td><b>{st}</b></td><td>{}</td><td>{}</td><td>{}</td><td>{mem}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            v("name"), esc(c["gaia"]["function"].as_str().unwrap_or("")), v("unit_state"), v("http"), v("latency_ms"),
            v("cpu_pct"), v("restarts_1h"), v("connection"), reasons.join("<br>")
        ));
    }
    let mut incs = String::new();
    for i in state["incidents"]["open"].as_array().into_iter().flatten() {
        let sigs: Vec<String> = i["signals"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                format!(
                    "{} {}: {}",
                    esc(s["severity"].as_str().unwrap_or("")),
                    esc(s["component"].as_str().unwrap_or("")),
                    esc(s["msg"].as_str().unwrap_or(""))
                )
            })
            .collect();
        incs.push_str(&format!(
            "<li class=\"{sev}\"><b>{sev}</b> {id} {title}<br><small>since {since} &middot; why: {why}<br>next: {next}<br>{sigs}</small></li>",
            sev = esc(i["severity"].as_str().unwrap_or("")), id = esc(i["id"].as_str().unwrap_or("")),
            title = esc(i["title"].as_str().unwrap_or("")), since = esc(i["opened"].as_str().unwrap_or("")),
            why = esc(i["why"].as_str().unwrap_or("")), next = esc(i["next"].as_str().unwrap_or("")), sigs = sigs.join("<br>")
        ));
    }
    if incs.is_empty() {
        incs.push_str("<li>none open</li>");
    }
    let mut ddos = String::new();
    for d in state["cortex"]["ddos"]["streams"]
        .as_array()
        .into_iter()
        .flatten()
    {
        ddos.push_str(&format!(
            "<li class=\"{0}\"><b>{0}</b> {1}</li>",
            esc(d["level"].as_str().unwrap_or("")),
            esc(d["explain"].as_str().unwrap_or(""))
        ));
    }
    if ddos.is_empty() {
        ddos.push_str("<li>no request streams yet</li>");
    }
    let mut ded = String::new();
    for d in state["self_eval"]["deductions"]
        .as_array()
        .into_iter()
        .flatten()
    {
        ded.push_str(&format!(
            "<li>-{} {}</li>",
            d["points"],
            esc(d["what"].as_str().unwrap_or(""))
        ));
    }
    let mut al = String::new();
    for a in state["alerts"]["recent"].as_array().into_iter().flatten() {
        al.push_str(&format!(
            "<li>{} {} {}: {}</li>",
            esc(a["ts"].as_str().unwrap_or("")),
            esc(a["severity"].as_str().unwrap_or("")),
            esc(a["component"].as_str().unwrap_or("")),
            esc(a["msg"].as_str().unwrap_or(""))
        ));
    }
    let sys = &state["system"];
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta http-equiv=refresh content=15><title>galion-nerve (openmind-nerve)</title>\
<style>body{{font:13px system-ui;background:#0f1115;color:#ddd;margin:1.5em}}table{{border-collapse:collapse}}td,th{{padding:3px 8px;text-align:left;border-bottom:1px solid #222;vertical-align:top}}\
tr.healthy b{{color:#4c4}}tr.degrading b{{color:#ec4}}tr.failing-soon b{{color:#f84}}tr.failing b{{color:#f44}}tr.absent{{color:#777}}a{{color:#8af}}\
li.critical b,li.flood b,li.error b{{color:#f44}}li.warn b,li.elevated b{{color:#ec4}}li.normal b,li.info b{{color:#4c4}}small{{color:#999}}.score{{font-size:22px}}</style></head><body>\
<h2>NERVE + CORTEX &middot; Obsidian Open Mind, made by Galion</h2>\
<p class=score>system <b>{}</b> &middot; self-eval <b>{}/100 ({})</b> &middot; {}</p>\
<p>{}</p><p>mode <b>{}</b> &middot; cycle {} &middot; {} (Europe/Zagreb) &middot; DDoS firing: <b>{}</b> &middot; \
<a href=/api/v1/state>/api/v1/state</a> <a href=/api/v1/incidents>incidents</a> <a href=/api/v1/cortex>cortex</a> <a href=/api/v1/self-eval.md>self-eval</a></p>\
<h3>Incidents</h3><ul>{incs}</ul><h3>Score deductions</h3><ul>{ded}</ul>\
<h3>Cortex: vulnerabilities</h3><ul>{}</ul><h3>Cortex: bleeding</h3><ul>{}</ul><h3>Cortex: DDoS</h3><ul>{ddos}</ul>\
<h3>Components</h3><table><tr><th>component</th><th>Gaia</th><th>status</th><th>unit</th><th>http</th><th>ms</th><th>mem</th><th>cpu%</th><th>restarts 1h</th><th>link</th><th>reasons</th></tr>{rows}</table>\
<h3>Alerts</h3><ul>{}</ul></body></html>",
        esc(sys["status"].as_str().unwrap_or("starting")),
        sys["score"], esc(sys["grade"].as_str().unwrap_or("-")),
        if sys["pass"] == true { "PASS" } else { "FAIL" },
        esc(sys["verdict"].as_str().unwrap_or("")),
        esc(state["mode"].as_str().unwrap_or("")),
        state["cycle"],
        esc(state["ts"].as_str().unwrap_or("")),
        sys["ddos_firing"],
        findings_html(&state["cortex"]["vulnerabilities"]),
        findings_html(&state["cortex"]["bleeding"]),
        if al.is_empty() { "<li>none</li>".to_string() } else { al },
    )
}

fn record_hit(sh: &Arc<Mutex<Shared>>, ip: &str, path: &str, status: u16) {
    let h = Hit {
        t: now_secs(),
        ip: ip.to_string(),
        path: path.chars().take(200).collect(),
        status,
    };
    sh.lock()
        .unwrap_or_else(|e| e.into_inner())
        .detector
        .stream("nerve-api", Some("galion-nerve"))
        .record(&h);
}

/// Accept loop (blocking). Bounded concurrency; loopback peers only.
pub fn serve(listener: TcpListener, sh: Arc<Mutex<Shared>>, ev: Option<Arc<Mutex<EventLog>>>) {
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut s) = conn else { continue };
        let peer = s.peer_addr().ok();
        if !peer.map(|a| a.ip().is_loopback()).unwrap_or(false) {
            continue;
        }
        let ip = peer.map(|a| a.ip().to_string()).unwrap_or_default();
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
        if active.load(Ordering::SeqCst) >= MAX_CONNS {
            record_hit(&sh, &ip, "-", 503);
            write_resp(&mut s, 503, "text/plain", "busy\n");
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (a, sh, ev) = (active.clone(), sh.clone(), ev.clone());
        let spawned = std::thread::Builder::new()
            .stack_size(CONN_STACK)
            .spawn(move || {
                match read_req(&mut s) {
                    Ok(r) => {
                        let (c, t, b) = route(&r, &sh, &ev);
                        write_resp(&mut s, c, t, &b);
                        record_hit(&sh, &ip, &r.path, c);
                    }
                    Err((c, m)) => {
                        write_resp(&mut s, c, "text/plain", m);
                        record_hit(&sh, &ip, "-", c);
                    }
                }
                a.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(m: &str, p: &str, b: &str) -> Req {
        let (path, query) = p.split_once('?').unwrap_or((p, ""));
        Req {
            method: m.into(),
            path: path.into(),
            query: query.into(),
            body: b.into(),
        }
    }

    #[test]
    fn parse_and_route() {
        let raw = b"POST /signal?x=1 HTTP/1.1\r\nHost: x\r\nContent-Length: 45\r\n\r\n{\"source\":\"hermes-rs\",\"kind\":\"heartbeat\"}    ";
        let r = read_req(&mut &raw[..]).unwrap();
        assert_eq!(
            (r.method.as_str(), r.path.as_str(), r.query.as_str()),
            ("POST", "/signal", "x=1")
        );
        let sh = Arc::new(Mutex::new(Shared::default()));
        assert_eq!(route(&r, &sh, &None).0, 202);
        assert_eq!(sh.lock().unwrap().hub.sources["hermes-rs"].total, 1);
        assert_eq!(
            route(
                &req("POST", "/api/v1/signal", r#"{"source":"m","kind":"error"}"#),
                &sh,
                &None
            )
            .0,
            202
        );
        assert_eq!(route(&req("POST", "/signal", "{}"), &sh, &None).0, 400);
        assert_eq!(route(&req("GET", "/signal", ""), &sh, &None).0, 405);
        assert!(route(&req("GET", "/", ""), &sh, &None).2.contains("NERVE"));
        for p in [
            "/api/v1/state",
            "/api/v1/incidents",
            "/api/v1/cortex",
            "/api/v1/ddos",
            "/api/v1/self-eval",
            "/state",
            "/healthz",
        ] {
            assert_eq!(route(&req("GET", p, ""), &sh, &None).0, 200, "{p}");
        }
        assert_eq!(route(&req("GET", "/api/v1/", ""), &sh, &None).0, 404);
        let h: Value =
            serde_json::from_str(&route(&req("GET", "/health", ""), &sh, &None).2).unwrap();
        assert_eq!(
            (h["status"].as_str(), h["service"].as_str()),
            (Some("ok"), Some("openmind-nerve"))
        );
        let big = format!(
            "POST /signal HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(read_req(&mut big.as_bytes()).err().map(|e| e.0), Some(413));
    }

    #[test]
    fn gaia_live_event_api() {
        let dir = std::env::temp_dir().join(format!("nerve-ev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let ev = Some(Arc::new(Mutex::new(EventLog {
            writer: Writer::open(&path, 1 << 20, 2).unwrap(),
            path: path.clone(),
            appended: 0,
        })));
        let sh = Arc::new(Mutex::new(Shared::default()));
        assert_eq!(route(&req("GET", "/events", ""), &sh, &ev).2, "[]");
        let (c, _, b) = route(
            &req(
                "POST",
                "/event",
                r#"{"source":"gaia-live","kind":"probe","severity":"error","data":{"port":8002}}"#,
            ),
            &sh,
            &ev,
        );
        assert_eq!((c, b.as_str()), (200, r#"{"ok":true,"appended":1}"#));
        assert_eq!(
            route(
                &req(
                    "POST",
                    "/event",
                    r#"{"source":"","kind":"k","severity":"s"}"#
                ),
                &sh,
                &ev
            )
            .0,
            400
        );
        assert_eq!(route(&req("GET", "/event", ""), &sh, &ev).0, 405);
        let (c, _, b) = route(&req("GET", "/events?n=5", ""), &sh, &ev);
        let v: Value = serde_json::from_str(&b).unwrap();
        assert_eq!(c, 200);
        assert_eq!(v[0]["data"]["port"], 8002);
        assert!(v[0]["ts"].as_str().unwrap().ends_with(":00"));
        assert_eq!(
            sh.lock().unwrap().hub.errors_since("gaia-live", 0),
            1,
            "event became a hub signal"
        );
        assert_eq!(tail_n("n=5000"), 1000);
        assert_eq!(tail_n(""), 50);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dashboard_names_galion_nerve_before_the_first_cycle() {
        // T3.6: galion-realtest's live probe wants Contains("galion-nerve") on `/`.
        // The live build's <title> is `galion-nerve`; before the first cycle
        // (state null) there is no component row, so the title must carry it.
        let h = dashboard(&Value::Null);
        assert!(h.contains("<title>galion-nerve"), "{h}");
    }

    #[test]
    fn dashboard_escapes() {
        let st = json!({"system": {"status": "healthy"}, "mode": "dry-run", "cycle": 1, "ts": "t",
            "components": [{"name": "<x>", "status": "healthy", "gaia": {"function": "AETHER"}, "reasons": ["a<b"]}],
            "incidents": {"open": [{"severity": "error", "id": "i", "title": "<t>", "signals": []}]},
            "reflex": {"recent": []}, "alerts": {"recent": []}});
        let h = dashboard(&st);
        assert!(h.contains("&lt;x&gt;"));
        assert!(h.contains("&lt;t&gt;"));
        assert!(!h.contains("<x>"));
    }

    #[test]
    fn live_socket_roundtrip_counts_requests() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let sh = Arc::new(Mutex::new(Shared::default()));
        let sh2 = sh.clone();
        std::thread::spawn(move || serve(l, sh2, None));
        let r = crate::util::http(
            crate::util::lo(port),
            "/signal",
            Some(r#"{"source":"t","kind":"error","msg":"x"}"#),
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(r.code, 202);
        let r = crate::util::http(
            crate::util::lo(port),
            "/healthz",
            None,
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!((r.code, r.body.as_str()), (200, "ok\n"));
        assert_eq!(sh.lock().unwrap().hub.errors_since("t", 0), 1);
        std::thread::sleep(Duration::from_millis(50));
        assert!(sh.lock().unwrap().detector.streams["nerve-api"].total >= 2);
    }
}
