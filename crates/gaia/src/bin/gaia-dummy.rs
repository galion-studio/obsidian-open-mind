// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! `gaia-dummy --port P [--mode ok|hang|flap|crash] [--up-ms MS] [--once-file F]`:
//! a throwaway loopback HTTP service for the Gaia live heal tests. `GET
//! /health` answers `{"status":"ok","service":"gaia-dummy","pid":N,
//! "started":"<Europe/Zagreb>","mode":"..."}`; everything else is 404. It
//! binds 127.0.0.1 only and refuses ports outside 19000-19999 (a real test may
//! move that inside 19000-22999 with `GAIA_TEST_PORTS` + `GALION_REAL=1`).
//!
//! Fault modes (Gaia S3.1 loop tests):
//! * `ok` (default): serves forever.
//! * `hang`: accepts connections and never answers (holds them open).
//! * `flap`: serves for `--up-ms` (default 1000) then exits with code 3;
//!   every restarted instance does the same (a crash loop).
//! * `crash`: if `--once-file F` does not exist, creates it, serves for
//!   `--up-ms` and exits with code 3; if F exists it serves forever. So the
//!   first instance crashes once and a restarted one stays up.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;
use std::time::Duration;

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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let port: Option<u16> = arg(&args, "--port").and_then(|p| p.parse().ok());
    let range = match gaia_live::heal::test_ports() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("gaia-dummy: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(port) = port.filter(|p| range.contains(p)) else {
        eprintln!(
            "usage: gaia-dummy --port P [--mode ok|hang|flap|crash] [--up-ms MS] [--once-file F]   (P in {}-{}; GAIA_TEST_PORTS with GALION_REAL=1 may move it inside 19000-22999)",
            range.start(),
            range.end()
        );
        return ExitCode::from(2);
    };
    let mode = arg(&args, "--mode").unwrap_or("ok");
    if !matches!(mode, "ok" | "hang" | "flap" | "crash") {
        eprintln!("gaia-dummy: unknown --mode {mode:?}");
        return ExitCode::from(2);
    }
    let up_ms: u64 = arg(&args, "--up-ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let l = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gaia-dummy: bind 127.0.0.1:{port}: {e}");
            return ExitCode::from(1);
        }
    };
    let started = zagreb_time::now_rfc3339();
    let pid = std::process::id();
    eprintln!("gaia-dummy: pid {pid} mode {mode} on 127.0.0.1:{port} since {started}");
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
        let Ok(mut s) = conn else { continue };
        if mode == "hang" {
            // Accept, never answer; keep the socket open so the client waits.
            held.push(s);
            continue;
        }
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0u8; 2048];
        let n = s.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let line = req.lines().next().unwrap_or("");
        let (code, body) = if line.starts_with("GET /health ") {
            (
                "200 OK",
                format!(
                    "{{\"status\":\"ok\",\"service\":\"gaia-dummy\",\"pid\":{pid},\"started\":\"{started}\",\"mode\":\"{mode}\"}}"
                ),
            )
        } else {
            ("404 Not Found", "{\"status\":\"not-found\"}".to_string())
        };
        let _ = write!(
            s,
            "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
    ExitCode::SUCCESS
}
