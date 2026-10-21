// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Minimal std-only HTTP/1.1 server for the paper broker (loopback by default).
//! One connection per thread, one request per connection, 64 KiB body cap.

use crate::broker::{OrderRequest, PaperBroker, Side};
use crate::money::parse_cents;
use crate::public::{self, Tape};
use crate::{backtest, compliance, news, report, score, ticket};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_BODY: usize = 64 * 1024;

pub struct Response {
    pub status: u16,
    pub ctype: &'static str,
    pub body: String,
}

fn j(status: u16, v: Value) -> Response {
    Response {
        status,
        ctype: "application/json",
        body: v.to_string(),
    }
}

fn err(status: u16, code: &str, msg: &str) -> Response {
    j(status, json!({ "ok": false, "code": code, "error": msg }))
}

fn query<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then_some(v)
    })
}

fn body_json(body: &str) -> Result<Value, Response> {
    if body.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(body).map_err(|e| err(400, "bad_json", &e.to_string()))
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Route one request against the shared broker. Pure function of state + input,
/// so the unit tests drive it without sockets.
pub fn route(state: &Mutex<PaperBroker>, method: &str, target: &str, body: &str) -> Response {
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let mut b = match state.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match (method, path) {
        ("GET", "/health") => j(
            200,
            json!({
                "ok": true, "service": "galion-wallstreet", "mode": "paper",
                "live_broker": false, "as_of": b.today(), "data_source": b.feed.data.source,
                "tape": b.tape, "data_class": b.tape.data_class(), "attribution": b.tape.attribution(),
                "time": zagreb_time::now_rfc3339()
            }),
        ),
        ("GET", "/universe") => j(200, json!(b.universe)),
        ("GET", "/quote") => {
            let sym = query(q, "symbol").unwrap_or("");
            match b.feed.quote(sym) {
                Some(qt) => j(200, json!(qt)),
                None => err(404, "unknown_symbol", sym),
            }
        }
        ("GET", "/account") | ("GET", "/positions") => j(200, json!(b.account())),
        ("GET", "/orders") => j(200, json!(b.open_orders())),
        ("GET", "/fills") => j(200, json!(b.fills)),
        ("GET", "/news") => {
            let sym = query(q, "symbol").unwrap_or("*");
            j(200, json!(news::context(&news::fixture(), sym, &b.today())))
        }
        ("GET", "/report") | ("GET", "/report.md") => {
            let t = query(q, "ticket").unwrap_or("session");
            let r = report::build(&b, t, "session", Vec::new());
            if path.ends_with(".md") {
                Response {
                    status: 200,
                    ctype: "text/markdown; charset=utf-8",
                    body: report::markdown(&r),
                }
            } else {
                j(200, json!(r))
            }
        }
        ("GET", "/backtest") => match query(q, "data") {
            None => j(200, json!(backtest::reference())),
            Some(d) => match Tape::parse(d) {
                Ok(t) => j(200, json!(public::backtests(t))),
                Err(e) => err(400, "unknown_tape", &e),
            },
        },
        ("GET", "/data") => match Tape::parse(query(q, "tape").unwrap_or(b.tape.name())) {
            Ok(t) => j(200, json!(public::info(t))),
            Err(e) => err(400, "unknown_tape", &e),
        },
        ("GET", "/data/notice") => Response {
            status: 200,
            ctype: "text/markdown; charset=utf-8",
            body: public::NOTICE_MD.to_string(),
        },
        ("GET", "/score") => j(200, json!(score::run())),
        ("POST", "/order") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let side = match s(&v, "side").as_str() {
                "buy" => Some(Side::Buy),
                "sell" => Some(Side::Sell),
                _ => None,
            };
            let limit = match v.get("limit").and_then(Value::as_str) {
                Some(l) => match parse_cents(l) {
                    Ok(c) => Some(c),
                    Err(e) => return err(400, "invalid_limit", &e),
                },
                None => None,
            };
            let o = b.place(OrderRequest {
                symbol: s(&v, "symbol"),
                side,
                qty: v.get("qty").and_then(Value::as_i64).unwrap_or(0),
                limit,
                rationale: s(&v, "rationale"),
                ticket: {
                    let t = s(&v, "ticket");
                    if t.is_empty() {
                        "session".into()
                    } else {
                        t
                    }
                },
            });
            let status = if o.is_refused() { 422 } else { 200 };
            j(status, json!(o))
        }
        ("POST", "/cancel") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            match b.cancel(v.get("order_id").and_then(Value::as_u64).unwrap_or(0)) {
                Ok(o) => j(200, json!({ "ok": true, "cancelled": o })),
                Err(e) => err(404, "not_found", &e),
            }
        }
        ("POST", "/close") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let t = s(&v, "ticket");
            let o = b.close(&s(&v, "symbol"), if t.is_empty() { "session" } else { &t });
            j(if o.is_refused() { 422 } else { 200 }, json!(o))
        }
        ("POST", "/advance") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let days = v.get("days").and_then(Value::as_u64).unwrap_or(1).min(1000);
            let mut fills = Vec::new();
            for _ in 0..days {
                if b.feed.at_end() {
                    break;
                }
                fills.extend(b.advance());
            }
            j(
                200,
                json!({ "ok": true, "as_of": b.today(), "worked": fills }),
            )
        }
        ("POST", "/kill") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let r = s(&v, "reason");
            b.engage_kill(if r.is_empty() { "operator" } else { &r });
            j(200, json!({ "ok": true, "kill_switch": b.kill }))
        }
        ("POST", "/kill/reset") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            let op = s(&v, "operator");
            if op.is_empty() {
                return err(
                    400,
                    "operator_required",
                    "a named operator must reset the kill switch",
                );
            }
            let was = b.reset_kill(&op);
            j(
                200,
                json!({ "ok": true, "was_engaged": was, "kill_switch": b.kill }),
            )
        }
        ("POST", "/reset") => {
            let t = b.tape;
            *b = t.broker();
            j(200, json!({ "ok": true, "as_of": b.today() }))
        }
        ("POST", "/compliance/check") => {
            let v = match body_json(body) {
                Ok(v) => v,
                Err(r) => return r,
            };
            j(200, json!(compliance::screen(&s(&v, "text"))))
        }
        ("POST", "/ticket") => match ticket::parse(body) {
            Ok(t) => {
                let fresh = query(q, "fresh") != Some("0");
                let r = if fresh {
                    ticket::run_fresh(&t)
                } else {
                    ticket::run(&mut b, &t)
                };
                j(if r.status == "refused" { 422 } else { 200 }, json!(r))
            }
            Err(e) => err(400, "bad_ticket", &e),
        },
        (
            _,
            "/health" | "/universe" | "/quote" | "/account" | "/positions" | "/orders" | "/fills"
            | "/news" | "/report" | "/report.md" | "/backtest" | "/data" | "/data/notice"
            | "/score" | "/order" | "/cancel" | "/close" | "/advance" | "/kill" | "/kill/reset"
            | "/reset" | "/compliance/check" | "/ticket",
        ) => err(405, "method_not_allowed", path),
        _ => err(404, "not_found", path),
    }
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        _ => "Error",
    }
}

fn handle(mut stream: TcpStream, state: &Mutex<PaperBroker>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let resp = if len > MAX_BODY {
        err(413, "too_large", "body over 64 KiB")
    } else {
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        route(state, &method, &target, &String::from_utf8_lossy(&buf))
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        resp.status,
        reason(resp.status),
        resp.ctype,
        resp.body.len(),
        resp.body
    )?;
    stream.flush()
}

pub fn serve(bind: &str) -> std::io::Result<()> {
    serve_tape(bind, Tape::Synthetic)
}

/// Serve the paper broker on a chosen data tape (synthetic, ecb-fx or ust).
pub fn serve_tape(bind: &str, tape: Tape) -> std::io::Result<()> {
    let l = TcpListener::bind(bind)?;
    eprintln!(
        "galion-wallstreet: PAPER broker listening on {} (tape {}, {}) ({})",
        l.local_addr()?,
        tape.name(),
        tape.data_class(),
        zagreb_time::now_rfc3339()
    );
    let state = Arc::new(Mutex::new(tape.broker()));
    for conn in l.incoming() {
        let Ok(c) = conn else { continue };
        let st = Arc::clone(&state);
        std::thread::spawn(move || {
            let _ = handle(c, &st);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_without_sockets() {
        let st = Mutex::new(PaperBroker::fixture());
        assert_eq!(route(&st, "GET", "/health", "").status, 200);
        assert_eq!(route(&st, "GET", "/quote?symbol=GLXA", "").status, 200);
        assert_eq!(route(&st, "GET", "/quote?symbol=ZZZ", "").status, 404);
        let r = route(
            &st,
            "POST",
            "/order",
            r#"{"symbol":"GLXA","side":"buy","qty":10}"#,
        );
        assert_eq!(r.status, 200, "{}", r.body);
        let r = route(
            &st,
            "POST",
            "/order",
            r#"{"symbol":"GLXA","side":"buy","qty":100000}"#,
        );
        assert_eq!(r.status, 422);
        assert!(r.body.contains("kill_switch_oversized_order"));
        assert_eq!(route(&st, "POST", "/kill/reset", "{}").status, 400);
        assert_eq!(route(&st, "GET", "/order", "").status, 405);
        assert_eq!(route(&st, "GET", "/nope", "").status, 404);
        assert_eq!(route(&st, "POST", "/order", "{bad").status, 400);
    }
}
