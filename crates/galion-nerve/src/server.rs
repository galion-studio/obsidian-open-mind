// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Loopback HTTP API: GET / (dashboard), GET /state, GET /self-eval(.md),
//! GET /alerts, GET /audit, GET /healthz, POST /signal. std only.

use crate::engine::Shared;
use crate::hub::{parse_signal, MAX_BODY};
use crate::util::now_secs;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_CONNS: usize = 16;

pub struct Req {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// Read one request (head <= 8 KiB, body <= MAX_BODY).
pub fn read_req(s: &mut impl Read) -> Result<Req, (u16, &'static str)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        let n = s.read(&mut chunk).map_err(|_| (400, "read error"))?;
        if n == 0 {
            return Err((400, "incomplete request"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if buf.len() > 8192 {
            return Err((431, "headers too large"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or("");
    let mut it = first.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it
        .next()
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
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
        path,
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
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn write_resp(s: &mut TcpStream, code: u16, ctype: &str, body: &str) {
    let _ = write!(
        s,
        "HTTP/1.1 {code} {}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        reason(code),
        body.len()
    );
}

/// Route a request against the shared state: (status, content type, body).
pub fn route(req: &Req, sh: &Arc<Mutex<Shared>>) -> (u16, &'static str, String) {
    let j = "application/json";
    let mut g = sh.lock().unwrap_or_else(|e| e.into_inner());
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/state") | ("GET", "/state.json") => (200, j, g.state.to_string()),
        ("GET", "/self-eval") | ("GET", "/self-eval.json") => (200, j, g.report.to_string()),
        ("GET", "/self-eval.md") => (200, "text/markdown; charset=utf-8", g.report_md.clone()),
        ("GET", "/alerts") => (200, j, g.alerts.to_string()),
        ("GET", "/audit") => (200, j, g.audit.to_string()),
        ("GET", "/signals") => (200, j, g.hub.to_json(now_secs()).to_string()),
        ("GET", "/healthz") => (200, "text/plain", "ok\n".into()),
        ("GET", "/") => (200, "text/html; charset=utf-8", dashboard(&g.state)),
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

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
    let mut acts = String::new();
    for a in state["reflex"]["recent"].as_array().into_iter().flatten() {
        acts.push_str(&format!(
            "<li>{} [{}] {} {}: {}</li>",
            esc(a["ts"].as_str().unwrap_or("")),
            esc(a["mode"].as_str().unwrap_or("")),
            esc(a["action"].as_str().unwrap_or("")),
            esc(a["unit"].as_str().unwrap_or("")),
            esc(a["result"].as_str().unwrap_or(""))
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
    format!(
        "<!doctype html><html><head><meta charset=utf-8><meta http-equiv=refresh content=15><title>galion-nerve</title>\
<style>body{{font:13px system-ui;background:#0f1115;color:#ddd;margin:1.5em}}table{{border-collapse:collapse}}td,th{{padding:3px 8px;text-align:left;border-bottom:1px solid #222;vertical-align:top}}\
tr.healthy b{{color:#4c4}}tr.degrading b{{color:#ec4}}tr.failing-soon b{{color:#f84}}tr.failing b{{color:#f44}}tr.absent{{color:#777}}a{{color:#8af}}</style></head><body>\
<h2>NERVE &middot; Obsidian Open Mind, made by Galion</h2><p>system <b>{}</b> &middot; mode <b>{}</b> &middot; cycle {} &middot; {} (Europe/Zagreb) &middot; \
<a href=/state>state</a> <a href=/self-eval.md>self-eval</a> <a href=/alerts>alerts</a> <a href=/audit>audit</a></p>\
<table><tr><th>component</th><th>Gaia</th><th>status</th><th>unit</th><th>http</th><th>ms</th><th>mem</th><th>cpu%</th><th>restarts 1h</th><th>link</th><th>reasons</th></tr>{rows}</table>\
<h3>Reflex actions</h3><ul>{}</ul><h3>Alerts</h3><ul>{}</ul></body></html>",
        esc(state["system"]["status"].as_str().unwrap_or("starting")),
        esc(state["mode"].as_str().unwrap_or("")),
        state["cycle"],
        esc(state["ts"].as_str().unwrap_or("")),
        if acts.is_empty() { "<li>none</li>".to_string() } else { acts },
        if al.is_empty() { "<li>none</li>".to_string() } else { al },
    )
}

/// Accept loop (blocking). Bounded concurrency; loopback peers only.
pub fn serve(listener: TcpListener, sh: Arc<Mutex<Shared>>) {
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut s) = conn else { continue };
        if !s.peer_addr().map(|a| a.ip().is_loopback()).unwrap_or(false) {
            continue;
        }
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
        if active.load(Ordering::SeqCst) >= MAX_CONNS {
            write_resp(&mut s, 503, "text/plain", "busy\n");
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (a, sh) = (active.clone(), sh.clone());
        let spawned = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                match read_req(&mut s) {
                    Ok(r) => {
                        let (c, t, b) = route(&r, &sh);
                        write_resp(&mut s, c, t, &b);
                    }
                    Err((c, m)) => write_resp(&mut s, c, "text/plain", m),
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

    #[test]
    fn parse_and_route() {
        let raw = b"POST /signal?x=1 HTTP/1.1\r\nHost: x\r\nContent-Length: 45\r\n\r\n{\"source\":\"hermes-rs\",\"kind\":\"heartbeat\"}    ";
        let r = read_req(&mut &raw[..]).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/signal"));
        let sh = Arc::new(Mutex::new(Shared::default()));
        let (c, _, _) = route(&r, &sh);
        assert_eq!(c, 202);
        assert_eq!(sh.lock().unwrap().hub.sources["hermes-rs"].total, 1);
        let bad = Req {
            method: "POST".into(),
            path: "/signal".into(),
            body: "{}".into(),
        };
        assert_eq!(route(&bad, &sh).0, 400);
        let get = Req {
            method: "GET".into(),
            path: "/signal".into(),
            body: String::new(),
        };
        assert_eq!(route(&get, &sh).0, 405);
        let html = Req {
            method: "GET".into(),
            path: "/".into(),
            body: String::new(),
        };
        assert!(route(&html, &sh).2.contains("NERVE"));
        let big = format!(
            "POST /signal HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(read_req(&mut big.as_bytes()).err().map(|e| e.0), Some(413));
    }

    #[test]
    fn dashboard_escapes() {
        let st = json!({"system": {"status": "healthy"}, "mode": "dry-run", "cycle": 1, "ts": "t",
            "components": [{"name": "<x>", "status": "healthy", "gaia": {"function": "AETHER"}, "reasons": ["a<b"]}],
            "reflex": {"recent": []}, "alerts": {"recent": []}});
        let h = dashboard(&st);
        assert!(h.contains("&lt;x&gt;"));
        assert!(!h.contains("<x>"));
    }

    #[test]
    fn live_socket_roundtrip() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let sh = Arc::new(Mutex::new(Shared::default()));
        let sh2 = sh.clone();
        std::thread::spawn(move || serve(l, sh2));
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
    }
}
