// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Replay a cargo-fuzz `http_parse_head` corpus against a live server.
//!
//! `replay_http ADDR DIR` sends every file in DIR (minus byte 0, which the
//! fuzz target uses to pick limits) over one TCP connection each, half-closes
//! the write side, and reads the reply (3 s cap). Prints one JSON line with
//! a status histogram. Exit 1 when a connection is refused, which means the
//! server died. Used by scripts/real/fuzz-sast.sh. Std only.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(addr), Some(dir)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: replay_http ADDR DIR");
        std::process::exit(2);
    };
    let addr: SocketAddr = addr.parse().expect("ADDR is host:port");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read DIR")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let mut hist: BTreeMap<String, u32> = BTreeMap::new();
    let mut refused = 0u32;
    for path in &files {
        let data = std::fs::read(path).unwrap_or_default();
        let wire = data.get(1..).unwrap_or(&[]);
        let mut s = match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(s) => s,
            Err(_) => {
                refused += 1;
                continue;
            }
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
        let _ = s.write_all(wire);
        let _ = s.shutdown(Shutdown::Write);
        let mut reply = Vec::new();
        let _ = s.take(64 * 1024).read_to_end(&mut reply);
        let key = match reply.get(..12) {
            Some(head) if head.starts_with(b"HTTP/1.") => {
                String::from_utf8_lossy(&head[9..12]).into_owned()
            }
            _ if reply.is_empty() => "no-reply".to_string(),
            _ => "garbage".to_string(),
        };
        *hist.entry(key).or_default() += 1;
    }
    let body: Vec<String> = hist.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    println!(
        "{{\"files\":{},\"refused\":{},\"status\":{{{}}}}}",
        files.len(),
        refused,
        body.join(",")
    );
    if refused > 0 {
        std::process::exit(1);
    }
}
