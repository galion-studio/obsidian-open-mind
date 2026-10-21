// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Test-only mock of an OpenAI-compatible model server, used by
//! `scripts/real/lan-routing.sh`: `mock_openai NAME PORT SECONDS` listens on
//! 127.0.0.1:PORT until SECONDS pass. No model runs: `/v1/chat/completions`
//! and `/v1/completions` answer a canned reply that names NAME, `/v1/models`
//! lists one model `mock-NAME`, `/health` says ok. Each request is counted
//! and one line goes to stderr (method, path, count; never the payload).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let name: String = args
        .next()
        .expect("NAME")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect();
    let port: u16 = args.next().and_then(|p| p.parse().ok()).expect("PORT");
    let secs: u64 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60)
        .min(600);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(secs));
        std::process::exit(0);
    });
    eprintln!("mock_openai {name} listening on 127.0.0.1:{port} for {secs} s");
    let mut served = 0u64;
    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let Ok(clone) = conn.try_clone() else {
            continue;
        };
        let mut reader = BufReader::new(clone);
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            continue;
        }
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                if k.trim().eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap_or(0).min(1 << 20);
                }
            }
        }
        let mut body = vec![0; len];
        let _ = reader.read_exact(&mut body);
        served += 1;
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        let (status, out) = match (method, path) {
            ("GET", "/health") => (200, r#"{"status":"ok"}"#.to_string()),
            ("GET", "/v1/models") => (
                200,
                format!(
                    r#"{{"object":"list","data":[{{"id":"mock-{name}","object":"model","owned_by":"{name}"}}]}}"#
                ),
            ),
            ("POST", "/v1/chat/completions") => (
                200,
                format!(
                    r#"{{"id":"chatcmpl-{name}-{served}","object":"chat.completion","model":"mock-{name}","choices":[{{"index":0,"message":{{"role":"assistant","content":"reply from {name}"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":0,"completion_tokens":3,"total_tokens":3}}}}"#
                ),
            ),
            ("POST", "/v1/completions") => (
                200,
                format!(
                    r#"{{"id":"cmpl-{name}-{served}","object":"text_completion","model":"mock-{name}","choices":[{{"index":0,"text":"reply from {name}","finish_reason":"stop"}}]}}"#
                ),
            ),
            _ => (404, r#"{"error":"not found"}"#.to_string()),
        };
        eprintln!("mock_openai {name} #{served} {method} {path} -> {status}");
        let _ = write!(
            conn,
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
            out.len()
        );
    }
}
