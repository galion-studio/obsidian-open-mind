// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! `gaia-dummy --port P`: a throwaway loopback HTTP service for the Gaia live
//! heal test. `GET /health` answers `{"status":"ok","service":"gaia-dummy",
//! "pid":N,"started":"<Europe/Zagreb>"}`; everything else is 404. It binds
//! 127.0.0.1 only and refuses ports outside 19000-19999.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let port: Option<u16> = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse().ok());
    let Some(port) = port.filter(|p| (19000..=19999).contains(p)) else {
        eprintln!("usage: gaia-dummy --port P   (P in 19000-19999)");
        return ExitCode::from(2);
    };
    let l = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gaia-dummy: bind 127.0.0.1:{port}: {e}");
            return ExitCode::from(1);
        }
    };
    let started = zagreb_time::now_rfc3339();
    let pid = std::process::id();
    eprintln!("gaia-dummy: pid {pid} on 127.0.0.1:{port} since {started}");
    for conn in l.incoming() {
        let Ok(mut s) = conn else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = [0u8; 2048];
        let n = s.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let line = req.lines().next().unwrap_or("");
        let (code, body) = if line.starts_with("GET /health ") {
            (
                "200 OK",
                format!(
                    "{{\"status\":\"ok\",\"service\":\"gaia-dummy\",\"pid\":{pid},\"started\":\"{started}\"}}"
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
