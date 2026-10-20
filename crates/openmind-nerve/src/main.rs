// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! `openmind-nerve serve`: a tiny loopback-only HTTP front for the JSONL
//! nerve log in this crate.
//!
//! * `GET /health`        -> `{"status":"ok","service":"openmind-nerve",...}`
//! * `POST /event`        -> body `{"source","kind","severity","data"}`, one line appended
//! * `GET /events?n=N`    -> the last N records (default 50, max 1000) as a JSON array
//!
//! Binds to loopback only (a non-loopback `--listen` is refused). Bodies are
//! capped at 64 KiB, every connection has a 5 s read timeout, and one
//! connection is served at a time (the nerve is a low-rate log, not a bus).
//! std + serde_json only; no TLS, no auth: it never leaves 127.0.0.1.

use openmind_nerve::{read_all, Event, Writer};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const MAX_BODY: usize = 64 * 1024;
const MAX_HEAD: usize = 8 * 1024;
const MAX_TAIL: usize = 1000;

const USAGE: &str =
    "usage: openmind-nerve serve --listen 127.0.0.1:PORT --log PATH [--max-bytes N] [--keep K]
  GET /health, POST /event {source,kind,severity,data}, GET /events?n=N
  loopback only; one JSONL line per event, Europe/Zagreb timestamps";

struct Cfg {
    listen: SocketAddr,
    log: PathBuf,
    max_bytes: u64,
    keep: usize,
}

fn parse(args: &[String]) -> Result<Cfg, String> {
    if args.first().map(String::as_str) != Some("serve") {
        return Err("missing command `serve`".into());
    }
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let listen: SocketAddr = opt("--listen")
        .ok_or("--listen is required")?
        .parse()
        .map_err(|e| format!("--listen: {e}"))?;
    if !listen.ip().is_loopback() {
        return Err(format!("refusing non-loopback listen address {listen}"));
    }
    let log = PathBuf::from(opt("--log").ok_or("--log is required")?);
    let max_bytes = match opt("--max-bytes") {
        Some(v) => v.parse().map_err(|e| format!("--max-bytes: {e}"))?,
        None => 8 * 1024 * 1024,
    };
    let keep = match opt("--keep") {
        Some(v) => v.parse().map_err(|e| format!("--keep: {e}"))?,
        None => 3,
    };
    Ok(Cfg {
        listen,
        log,
        max_bytes,
        keep,
    })
}

/// One parsed request: method, path (with query), body.
struct Req {
    method: String,
    target: String,
    body: Vec<u8>,
}

fn read_req(s: &mut TcpStream) -> io::Result<Req> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut r = BufReader::new(s.take((MAX_HEAD + MAX_BODY) as u64));
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| bad("empty request"))?
        .to_string();
    let target = parts.next().ok_or_else(|| bad("no target"))?.to_string();
    let mut len = 0usize;
    let mut head = line.len();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 {
            break;
        }
        head += h.len();
        if head > MAX_HEAD {
            return Err(bad("header too large"));
        }
        let t = h.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().map_err(|_| bad("bad content-length"))?;
            }
        }
    }
    if len > MAX_BODY {
        return Err(bad("body too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Req {
        method,
        target,
        body,
    })
}

fn respond(s: &mut TcpStream, code: u16, body: &Value) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let text = body.to_string();
    let _ = write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = s.flush();
}

/// `n` from `?n=N`, clamped to 1..=MAX_TAIL.
fn tail_n(target: &str) -> usize {
    target
        .split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .find_map(|kv| kv.strip_prefix("n="))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, MAX_TAIL)
}

fn event_from(body: &[u8]) -> Result<Event, String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("json: {e}"))?;
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
    json!({
        "ts": e.ts(),
        "source": e.source(),
        "kind": e.kind(),
        "severity": e.severity(),
        "data": serde_json::from_str::<Value>(&e.data_json()).unwrap_or(Value::Null),
    })
}

fn handle(s: &mut TcpStream, cfg: &Cfg, w: &mut Writer, appended: &mut u64) {
    let req = match read_req(s) {
        Ok(r) => r,
        Err(e) => return respond(s, 400, &json!({"ok": false, "error": e.to_string()})),
    };
    let path = req.target.split('?').next().unwrap_or("");
    match (req.method.as_str(), path) {
        ("GET", "/health") => respond(
            s,
            200,
            &json!({
                "status": "ok",
                "service": "openmind-nerve",
                "version": env!("CARGO_PKG_VERSION"),
                "appended": *appended,
                "ts": zagreb_time::now_rfc3339(),
            }),
        ),
        ("POST", "/event") => match event_from(&req.body) {
            Ok(ev) => match w.append(&ev) {
                Ok(()) => {
                    *appended += 1;
                    respond(s, 200, &json!({"ok": true, "appended": *appended}))
                }
                Err(e) => respond(s, 500, &json!({"ok": false, "error": e.to_string()})),
            },
            Err(e) => respond(s, 400, &json!({"ok": false, "error": e})),
        },
        ("GET", "/events") => {
            let n = tail_n(&req.target);
            match read_all(&cfg.log) {
                Ok(all) => {
                    let from = all.len().saturating_sub(n);
                    let arr: Vec<Value> = all[from..].iter().map(record_json).collect();
                    respond(s, 200, &Value::Array(arr))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    respond(s, 200, &Value::Array(vec![]))
                }
                Err(e) => respond(s, 500, &json!({"ok": false, "error": e.to_string()})),
            }
        }
        (_, "/health" | "/event" | "/events") => {
            respond(s, 405, &json!({"ok": false, "error": "method not allowed"}))
        }
        _ => respond(s, 404, &json!({"ok": false, "error": "not found"})),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let cfg = match parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("openmind-nerve: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut w = match Writer::open(&cfg.log, cfg.max_bytes, cfg.keep) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("openmind-nerve: open {}: {e}", cfg.log.display());
            return ExitCode::from(1);
        }
    };
    let l = match TcpListener::bind(cfg.listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("openmind-nerve: bind {}: {e}", cfg.listen);
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "openmind-nerve: listening on {} log {} ({})",
        cfg.listen,
        cfg.log.display(),
        zagreb_time::now_rfc3339()
    );
    let mut appended = 0u64;
    for conn in l.incoming() {
        let Ok(mut s) = conn else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
        handle(&mut s, &cfg, &mut w, &mut appended);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn refuses_non_loopback() {
        let e = parse(&a(&[
            "serve",
            "--listen",
            "0.0.0.0:19031",
            "--log",
            "/tmp/x",
        ]))
        .err()
        .unwrap();
        assert!(e.contains("non-loopback"), "{e}");
        assert!(parse(&a(&[
            "serve",
            "--listen",
            "127.0.0.1:19031",
            "--log",
            "/tmp/x"
        ]))
        .is_ok());
        assert!(parse(&a(&["serve", "--log", "/tmp/x"])).is_err());
    }

    #[test]
    fn tail_is_clamped() {
        assert_eq!(tail_n("/events"), 50);
        assert_eq!(tail_n("/events?n=3"), 3);
        assert_eq!(tail_n("/events?x=1&n=0"), 1);
        assert_eq!(tail_n("/events?n=99999"), MAX_TAIL);
    }

    #[test]
    fn event_body_is_validated() {
        let ok = event_from(br#"{"source":"gaia","kind":"k","severity":"info","data":{"a":1}}"#)
            .unwrap();
        assert_eq!(ok.kind(), "k");
        assert_eq!(ok.data_json(), r#"{"a":1}"#);
        assert!(event_from(br#"{"source":"gaia","kind":"","severity":"info"}"#).is_err());
        assert!(event_from(b"not json").is_err());
    }
}
