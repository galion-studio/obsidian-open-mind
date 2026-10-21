// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! `gaia-dummy --port P [--mode MODE] [--up-ms MS] [--once-file F]
//! [--delay-ms MS] [--slow-count N] [--bad KIND]`: a throwaway loopback HTTP
//! service for the Gaia live heal tests. `GET /health` answers
//! `{"status":"ok","service":"gaia-dummy","pid":N,
//! "started":"<Europe/Zagreb>","mode":"..."}`; everything else is 404. It
//! binds 127.0.0.1 only and refuses ports outside 19000-20999 and live VPS ports
//! (`gaia_live::heal::is_test_port`).
//!
//! Fault modes (Gaia S3.1 loop tests, S3.2 fault injection):
//! * `ok` (default): serves forever.
//! * `hang`: accepts connections and never answers (holds them open).
//! * `flap`: serves for `--up-ms` (default 1000) then exits with code 3;
//!   every restarted instance does the same (a crash loop).
//! * `crash`: if `--once-file F` does not exist, creates it, serves for
//!   `--up-ms` and exits with code 3; if F exists it serves forever. So the
//!   first instance crashes once and a restarted one stays up.
//! * `slow`: answers `/health` correctly after `--delay-ms` (default 1500);
//!   with `--slow-count N` only the first N requests are slow.
//! * `badjson`: `/health` is `200 OK` with a broken or misleading body
//!   (`--bad truncated|nested|text|html|spaced|array|empty|okfalse`).
//! * `wrong`: `/health` is `503` `{"status":"down"}` (e.g. a foreign
//!   process that took the port).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MODES: &[&str] = &["ok", "hang", "flap", "crash", "slow", "badjson", "wrong"];
const BAD: &[&str] = &[
    "truncated",
    "nested",
    "text",
    "html",
    "spaced",
    "array",
    "empty",
    "okfalse",
];

fn arg<'a>(args: &'a [String], k: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn exit_after(ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        eprintln!(
            "gaia-dummy: pid {} exits after {ms} ms (fault mode)",
            std::process::id()
        );
        std::process::exit(3);
    });
}

/// The `/health` body of a `badjson` instance. Each one contains the text
/// `"status":"ok"` or `"ok":true` somewhere, but none is a JSON object whose
/// top-level status is ok.
fn bad_body(kind: &str) -> &'static str {
    match kind {
        "truncated" => "{\"status\":\"ok\"",
        "nested" => "{\"status\":\"down\",\"last\":{\"status\":\"ok\"}}",
        "text" => "health: \"status\":\"ok\" (plain text)",
        "html" => "<html><body>\"ok\":true</body></html>",
        "spaced" => "{\"status\":\"o k\"}",
        "array" => "[{\"status\":\"ok\"}]",
        "okfalse" => "{\"ok\":false,\"checks\":{\"ok\":true}}",
        _ => "",
    }
}

struct Cfg {
    mode: String,
    pid: u32,
    started: String,
    delay_ms: u64,
    slow_count: u64,
    bad: String,
    seen: AtomicU64,
}

fn answer(mut s: TcpStream, c: &Cfg) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = [0u8; 2048];
    let n = s.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let line = req.lines().next().unwrap_or("");
    let health = line.starts_with("GET /health ");
    if health && c.mode == "slow" {
        let nth = c.seen.fetch_add(1, Ordering::SeqCst);
        if c.slow_count == 0 || nth < c.slow_count {
            std::thread::sleep(Duration::from_millis(c.delay_ms));
        }
    }
    let (code, body) = if !health {
        ("404 Not Found", "{\"status\":\"not-found\"}".to_string())
    } else if c.mode == "badjson" {
        ("200 OK", bad_body(&c.bad).to_string())
    } else if c.mode == "wrong" {
        (
            "503 Service Unavailable",
            format!("{{\"status\":\"down\",\"pid\":{}}}", c.pid),
        )
    } else {
        (
            "200 OK",
            format!(
                "{{\"status\":\"ok\",\"service\":\"gaia-dummy\",\"pid\":{},\"started\":\"{}\",\"mode\":\"{}\"}}",
                c.pid, c.started, c.mode
            ),
        )
    };
    let _ = write!(
        s,
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let port: Option<u16> = arg(&args, "--port").and_then(|p| p.parse().ok());
    let Some(port) = port.filter(|p| gaia_live::heal::is_test_port(*p)) else {
        eprintln!("usage: gaia-dummy --port P [--mode ok|hang|flap|crash|slow|badjson|wrong] [--up-ms MS] [--once-file F] [--delay-ms MS] [--slow-count N] [--bad KIND]   (P in 19000-20999, not a live VPS port)");
        return ExitCode::from(2);
    };
    let mode = arg(&args, "--mode").unwrap_or("ok");
    if !MODES.contains(&mode) {
        eprintln!("gaia-dummy: unknown --mode {mode:?}");
        return ExitCode::from(2);
    }
    let bad = arg(&args, "--bad").unwrap_or("truncated");
    if mode == "badjson" && !BAD.contains(&bad) {
        eprintln!("gaia-dummy: unknown --bad {bad:?} (one of {BAD:?})");
        return ExitCode::from(2);
    }
    let num = |k: &str, d: u64| arg(&args, k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let up_ms = num("--up-ms", 1000);
    let l = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gaia-dummy: bind 127.0.0.1:{port}: {e}");
            return ExitCode::from(1);
        }
    };
    let cfg = Arc::new(Cfg {
        mode: mode.to_string(),
        pid: std::process::id(),
        started: zagreb_time::now_rfc3339(),
        delay_ms: num("--delay-ms", 1500).min(60_000),
        slow_count: num("--slow-count", 0),
        bad: bad.to_string(),
        seen: AtomicU64::new(0),
    });
    eprintln!(
        "gaia-dummy: pid {} mode {mode} on 127.0.0.1:{port} since {}",
        cfg.pid, cfg.started
    );
    match mode {
        "flap" => exit_after(up_ms),
        "crash" => {
            let Some(f) = arg(&args, "--once-file") else {
                eprintln!("gaia-dummy: --mode crash needs --once-file F");
                return ExitCode::from(2);
            };
            let first = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(f)
                .is_ok();
            if first {
                exit_after(up_ms);
            }
        }
        _ => {}
    }
    let mut held = Vec::new();
    for conn in l.incoming() {
        let Ok(s) = conn else { continue };
        if mode == "hang" {
            // Accept, never answer; keep the socket open so the client waits.
            held.push(s);
            continue;
        }
        if mode == "slow" {
            // One thread per connection so a slow answer does not queue the
            // next probe behind it.
            let c = Arc::clone(&cfg);
            std::thread::spawn(move || answer(s, &c));
        } else {
            answer(s, &cfg);
        }
    }
    ExitCode::SUCCESS
}
