// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Stdio entry for the OpenMind MCP server.
//!
//! Flags (each also readable from env, flag wins):
//!
//! | flag | env | default |
//! |---|---|---|
//! | `--memory-url URL` | `OPENMIND_MCP_MEMORY_URL` | `http://127.0.0.1:8420` (loopback only) |
//! | `--no-memory` | | memory tools on |
//! | `--vault DIR` | `OPENMIND_MCP_VAULT` | none (vault tools off) |
//! | `--session-key K` | `OPENMIND_MCP_SESSION_KEY` | `hermes-mcp` |
//! | `--key-env NAME` | | `OPENMIND_MEMORY_API_KEY`, then `TDAI_GATEWAY_API_KEY` |
//! | `--timeout-s N` | | 10 |
//!
//! The memory Bearer key is read from the environment only (never a flag, so
//! it does not show in `ps`). hermes-rs clears the child environment; list the
//! key's name in the server's `pass_env` in config.yaml.
//!
//! Reads JSON-RPC lines from stdin, writes one reply line per request, exits
//! on EOF. Logs go to stderr only.

#![forbid(unsafe_code)]

use openmind_mcp_stack::{Server, Settings};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

fn usage() -> &'static str {
    "usage: openmind-mcp-stack [--memory-url URL | --no-memory] [--vault DIR] [--session-key K] [--key-env NAME] [--timeout-s N]"
}

fn parse(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, String> {
    let mut s = Settings::default();
    let get = |k: &str| {
        env(k)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(u) = get("OPENMIND_MCP_MEMORY_URL") {
        s.memory_url = Some(u);
    }
    if let Some(v) = get("OPENMIND_MCP_VAULT") {
        s.vault = Some(PathBuf::from(v));
    }
    if let Some(k) = get("OPENMIND_MCP_SESSION_KEY") {
        s.session_key = k;
    }
    let mut key_envs: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value\n{}", usage()))
        };
        match a.as_str() {
            "--memory-url" => s.memory_url = Some(val("--memory-url")?),
            "--no-memory" => s.memory_url = None,
            "--vault" => s.vault = Some(PathBuf::from(val("--vault")?)),
            "--no-vault" => s.vault = None,
            "--session-key" => s.session_key = val("--session-key")?,
            "--key-env" => key_envs.push(val("--key-env")?),
            "--timeout-s" => {
                let n: u64 = val("--timeout-s")?
                    .parse()
                    .map_err(|_| "--timeout-s needs a number".to_string())?;
                s.timeout = Duration::from_secs(n.clamp(1, 120));
            }
            "-h" | "--help" => return Err(usage().into()),
            other => return Err(format!("unknown argument {other}\n{}", usage())),
        }
    }
    if key_envs.is_empty() {
        key_envs = vec![
            "OPENMIND_MEMORY_API_KEY".into(),
            "TDAI_GATEWAY_API_KEY".into(),
        ];
    }
    s.memory_key = key_envs.iter().find_map(|k| get(k));
    Ok(s)
}

fn main() -> ExitCode {
    galion_common::build_info::exit_if_version("openmind-mcp-stack", env!("CARGO_PKG_VERSION"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = match parse(&args, &|k| std::env::var(k).ok()) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(io::stderr(), "{e}");
            return ExitCode::from(2);
        }
    };
    let server = match Server::new(&settings) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(io::stderr(), "openmind-mcp-stack: {e}");
            return ExitCode::from(2);
        }
    };
    let _ = writeln!(
        io::stderr(),
        "openmind-mcp-stack: tools {:?}, memory key {}",
        server.tool_names(),
        if settings.memory_key.is_some() {
            "set"
        } else {
            "unset"
        }
    );
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                let _ = writeln!(io::stderr(), "{e}");
                return ExitCode::from(1);
            }
        };
        if let Some(out) = server.handle_line(&line) {
            if writeln!(stdout, "{out}").is_err() || stdout.flush().is_err() {
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_and_env() {
        let env = |k: &str| match k {
            "OPENMIND_MCP_VAULT" => Some("/v".to_string()),
            "TDAI_GATEWAY_API_KEY" => Some("k1".to_string()),
            "OTHER" => Some("k2".to_string()),
            _ => None,
        };
        let s = parse(&[], &env).unwrap();
        assert_eq!(s.vault, Some(PathBuf::from("/v")));
        assert_eq!(s.memory_key.as_deref(), Some("k1"));
        let a: Vec<String> = [
            "--no-memory",
            "--vault",
            "/w",
            "--key-env",
            "OTHER",
            "--session-key",
            "x",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let s = parse(&a, &env).unwrap();
        assert!(s.memory_url.is_none());
        assert_eq!(s.vault, Some(PathBuf::from("/w")));
        assert_eq!(s.memory_key.as_deref(), Some("k2"));
        assert_eq!(s.session_key, "x");
        assert!(parse(&["--bogus".to_string()], &env).is_err());
    }
}
