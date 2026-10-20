// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Test dummy for scripts/real/survival-heal.sh: a loopback HTTP service that
//! answers 200 "ok" and, with `--die-after-s N`, exits after N seconds (a
//! service that dies). Not a product binary; only built as an example.
//!
//! dummy_service --port P [--die-after-s N] [--pidfile F]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let get = |k: &str| {
        a.iter()
            .position(|x| x == k)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    let port: u16 = get("--port")
        .and_then(|p| p.parse().ok())
        .expect("--port P");
    let die: u64 = get("--die-after-s")
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    if let Some(p) = get("--pidfile") {
        std::fs::write(p, format!("{}\n", std::process::id())).expect("pidfile");
    }
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    l.set_nonblocking(true).expect("nonblocking");
    let t0 = Instant::now();
    loop {
        if die > 0 && t0.elapsed() >= Duration::from_secs(die) {
            eprintln!("dummy {port}: dying after {die}s");
            std::process::exit(1);
        }
        match l.accept() {
            Ok((mut s, _)) => {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}
