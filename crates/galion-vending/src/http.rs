// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Minimal blocking HTTP/1.1 front-end (std only, one connection at a time,
//! `Connection: close`). This is the surface a Hermes tool adapter calls.
//!
//! | method | path | body / query | answer |
//! |---|---|---|---|
//! | GET | `/health` | | `{"ok":true,...}` |
//! | GET | `/v1/tools` | | tool list |
//! | GET | `/v1/status` | | book view (journaled as a `status` call) |
//! | POST | `/v1/call` | a `ToolCall` JSON | `{"ok":true,"result":..}` or `{"ok":false,"error":..}` |
//! | GET | `/v1/score` | | score of the live session so far |
//! | POST | `/v1/reset` | `?seed=N&days=M[&level=standard|hard]` | starts a fresh session (journal restarts) |
//! | GET | `/v1/bench` | `?seed=N&days=M[&level=standard|hard]` | scripted-operator run on a scratch sim |
//! | POST | `/v1/shutdown` | | stops the server |
//!
//! Binds 127.0.0.1 by default. Nothing here touches money or the network
//! beyond the listening socket.

use crate::journal;
use crate::sim::Sim;
use crate::tools::{tool_specs, ToolCall};
use crate::world::{Level, BENCH_SEED, DEFAULT_DAYS};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

const MAX_BODY: usize = 1 << 20;

pub struct Server {
    sim: Sim,
    log: Option<PathBuf>,
}

fn query(path: &str, key: &str) -> Option<u64> {
    let q = path.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.parse().ok()).flatten()
    })
}

fn query_level(path: &str) -> Result<Level, String> {
    let Some(q) = path.split_once('?').map(|p| p.1) else {
        return Ok(Level::Standard);
    };
    match q.split('&').find_map(|kv| kv.strip_prefix("level=")) {
        None => Ok(Level::Standard),
        Some(v) => Level::parse(v).ok_or_else(|| format!("unknown level '{v}' (standard|hard)")),
    }
}

fn new_sim(seed: u64, days: u32, level: Level, log: &Option<PathBuf>) -> std::io::Result<Sim> {
    match log {
        Some(p) => journal::create_level(p, seed, days, level),
        None => Ok(Sim::with_level(seed, days, level)),
    }
}

impl Server {
    pub fn new(seed: u64, days: u32, log: Option<PathBuf>) -> std::io::Result<Self> {
        Self::with_level(seed, days, Level::Standard, log)
    }

    pub fn with_level(
        seed: u64,
        days: u32,
        level: Level,
        log: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        Ok(Server {
            sim: new_sim(seed, days, level, &log)?,
            log,
        })
    }

    /// Handle one request; returns (status, body, stop).
    pub fn handle(&mut self, method: &str, path: &str, body: &[u8]) -> (u16, Value, bool) {
        let route = path.split('?').next().unwrap_or("");
        match (method, route) {
            ("GET", "/health") => (
                200,
                json!({"ok": true, "crate": "galion-vending", "version": env!("CARGO_PKG_VERSION"),
                       "seed": self.sim.seed(), "day": self.sim.day(), "finished": self.sim.finished(),
                       "level": self.sim.level()}),
                false,
            ),
            ("GET", "/v1/tools") => (200, tool_specs(), false),
            ("GET", "/v1/status") => self.call(ToolCall::Status),
            ("POST", "/v1/call") => match serde_json::from_slice::<ToolCall>(body) {
                Ok(c) => self.call(c),
                Err(e) => (
                    400,
                    json!({"ok": false, "error": {"code": "bad_request", "message": e.to_string()}}),
                    false,
                ),
            },
            ("GET", "/v1/score") => (200, json!(self.sim.score()), false),
            ("POST", "/v1/reset") => {
                let seed = query(path, "seed").unwrap_or(BENCH_SEED);
                let days = query(path, "days").map_or(DEFAULT_DAYS, |d| d as u32);
                let level = match query_level(path) {
                    Ok(l) => l,
                    Err(e) => return (400, json!({"ok": false, "error": e}), false),
                };
                match new_sim(seed, days, level, &self.log) {
                    Ok(s) => {
                        self.sim = s;
                        (
                            200,
                            json!({"ok": true, "seed": seed, "days": self.sim.days(), "level": level}),
                            false,
                        )
                    }
                    Err(e) => (500, json!({"ok": false, "error": e.to_string()}), false),
                }
            }
            ("GET", "/v1/bench") => {
                let seed = query(path, "seed").unwrap_or(BENCH_SEED);
                let days = query(path, "days").map_or(DEFAULT_DAYS, |d| d as u32);
                match query_level(path) {
                    Ok(level) => (200, json!(crate::bench_level(seed, days, level)), false),
                    Err(e) => (400, json!({"ok": false, "error": e}), false),
                }
            }
            ("POST", "/v1/shutdown") => (200, json!({"ok": true, "stopping": true}), true),
            _ => (404, json!({"ok": false, "error": "not found"}), false),
        }
    }

    fn call(&mut self, c: ToolCall) -> (u16, Value, bool) {
        match self.sim.call(c) {
            Ok(r) => (200, json!({"ok": true, "result": r}), false),
            Err(e) => (200, json!({"ok": false, "error": e}), false),
        }
    }

    /// Serve until `/v1/shutdown`.
    pub fn serve(mut self, listener: TcpListener) -> std::io::Result<()> {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
            let stop = match read_request(&mut s) {
                Ok((m, p, b)) => {
                    let (code, v, stop) = self.handle(&m, &p, &b);
                    let _ = respond(&mut s, code, &v);
                    stop
                }
                Err(e) => {
                    let _ = respond(&mut s, 400, &json!({"ok": false, "error": e}));
                    false
                }
            };
            if stop {
                break;
            }
        }
        Ok(())
    }
}

fn read_request(s: &mut TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let mut r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("no method")?.to_string();
    let path = parts.next().ok_or("no path")?.to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().map_err(|_| "bad content-length")?;
            }
        }
    }
    if len > MAX_BODY {
        return Err("body too large".into());
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok((method, path, body))
}

fn respond(s: &mut TcpStream, code: u16, v: &Value) -> std::io::Result<()> {
    let body = v.to_string();
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    s.flush()
}
