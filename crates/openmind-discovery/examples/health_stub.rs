// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Test-only stand-in for a device's HTTP port, used by
//! `scripts/real/lan-discovery.sh`: `health_stub PORT SECONDS` listens on
//! 127.0.0.1:PORT and answers every request with `{"status":"ok"}` until
//! SECONDS pass. Loopback only.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).expect("PORT");
    let secs: u64 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
        .min(600);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let until = Instant::now() + Duration::from_secs(secs);
    let mut served = 0u32;
    while Instant::now() < until {
        match listener.accept() {
            Ok((mut conn, _)) => {
                let _ = conn.set_nonblocking(false);
                let _ = conn.set_read_timeout(Some(Duration::from_millis(300)));
                let mut buf = [0u8; 1024];
                let n = conn.read(&mut buf).unwrap_or(0);
                if n > 0 {
                    let body = "{\"status\":\"ok\"}";
                    let _ = write!(
                        conn,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                served += 1;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    eprintln!("health_stub 127.0.0.1:{port} served {served} connections");
}
