// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! `github-mcp-rs [--token-file PATH] [--allow-writes] [--api-url URL]`: stdio MCP server.
#[allow(dead_code)]
mod server;
use server::Server;
use std::io::{BufRead, Write};

fn main() {
    let mut s = Server::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--token-file" => s.token_file = args.next(),
            "--allow-writes" => s.allow_writes = true,
            "--api-url" => s.api = args.next().unwrap_or(s.api),
            "--timeout" => {
                s.timeout_s = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(s.timeout_s)
            }
            "--list-tools" => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&server::tools_list()).unwrap_or_default()
                );
                return;
            }
            "--help" | "-h" => {
                println!("github-mcp-rs [--token-file PATH] [--allow-writes] [--api-url URL] [--timeout S] [--list-tools]");
                return;
            }
            o => {
                eprintln!("github-mcp-rs: unknown argument {o}");
                std::process::exit(2);
            }
        }
    }
    eprintln!(
        "github-mcp-rs: ready ({} tools, writes {})",
        server::reference_tools().len(),
        if s.allow_writes { "ON" } else { "off" }
    );
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(m) => s.handle(&m),
            Err(_) => Some(
                serde_json::json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}),
            ),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
}
