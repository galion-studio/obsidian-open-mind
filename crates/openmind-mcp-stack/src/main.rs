// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Stdio entry for the OpenMind MCP server.
//!
//! DESK-ID: openmind.B57
//!
//! ```text
//! openmind-mcp-stack [--vault <dir>] [--memory-url http://127.0.0.1:8420]
//!                    [--key-file <env-file>] [--no-memory]
//!                    [--save-session <key>] [--timeout-ms <n>] [--check]
//! ```
//!
//! Reads JSON-RPC lines from stdin, writes one response line per request
//! to stdout, exits at EOF. Diagnostics go to stderr and never contain the
//! memory key. `--check` prints the configuration (no key) and exits.
//! Exit 2 = bad arguments (non-loopback memory URL, missing vault root).

#![forbid(unsafe_code)]

use openmind_mcp_stack::memory::{key_from_env_file, key_from_env_text, MemoryClient};
use openmind_mcp_stack::vault::Vault;
use openmind_mcp_stack::{Config, Server, DEFAULT_SAVE_SESSION};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (server, check) = match build(&args) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(io::stderr(), "openmind-mcp-stack: {e}");
            return ExitCode::from(2);
        }
    };
    if check {
        println!("tools={}", server.tool_names().join(","));
        return ExitCode::SUCCESS;
    }
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(mut line) = line else {
            return ExitCode::from(1);
        };
        line.push('\n');
        let out = server.handle_bytes(line.as_bytes());
        if out.is_empty() {
            continue;
        }
        if stdout.write_all(&out).is_err() || stdout.flush().is_err() {
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

fn build(args: &[String]) -> Result<(Server, bool), String> {
    let mut vault: Option<PathBuf> = None;
    let mut url = String::from("http://127.0.0.1:8420");
    let mut key_file: Option<PathBuf> = None;
    let mut no_memory = false;
    let mut check = false;
    let mut save_session = DEFAULT_SAVE_SESSION.to_string();
    let mut timeout_ms: u64 = 5000;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--vault" => vault = Some(PathBuf::from(val("--vault")?)),
            "--memory-url" => url = val("--memory-url")?,
            "--key-file" => key_file = Some(PathBuf::from(val("--key-file")?)),
            "--save-session" => save_session = val("--save-session")?,
            "--timeout-ms" => {
                timeout_ms = val("--timeout-ms")?
                    .parse()
                    .map_err(|_| "--timeout-ms must be a number")?;
            }
            "--no-memory" => no_memory = true,
            "--check" => check = true,
            "--version" | "-V" => {
                println!("openmind-mcp-stack {}", openmind_mcp_stack::SERVER_VERSION);
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let vault = match vault {
        Some(p) => Some(Vault::open(&p)?),
        None => None,
    };
    let memory = if no_memory {
        None
    } else {
        let key = match &key_file {
            Some(p) => key_from_env_file(p)?,
            None => {
                let env: String = openmind_mcp_stack::memory::KEY_NAMES
                    .iter()
                    .filter_map(|k| std::env::var(k).ok().map(|v| format!("{k}={v}\n")))
                    .collect();
                key_from_env_text(&env)
            }
        };
        Some(MemoryClient::new(
            &url,
            key,
            Duration::from_millis(timeout_ms.clamp(100, 60_000)),
        )?)
    };
    if check {
        eprintln!(
            "vault={} memory={} key={}",
            vault
                .as_ref()
                .map_or("off".into(), |v| v.root().display().to_string()),
            if memory.is_some() {
                url.as_str()
            } else {
                "off"
            },
            memory.as_ref().is_some_and(MemoryClient::has_key)
        );
    }
    Ok((
        Server::new(Config {
            memory,
            vault,
            save_session,
            fixed_time: None,
        }),
        check,
    ))
}
