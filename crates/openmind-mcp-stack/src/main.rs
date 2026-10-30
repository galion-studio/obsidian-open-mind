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
//!                    [--stack-conf <file>] [--no-stack]
//! ```
//!
//! Reads JSON-RPC lines from stdin, writes one response line per request
//! to stdout, exits at EOF. Diagnostics go to stderr and never contain the
//! memory key. `--check` prints the configuration (no key) and exits.
//! Stack tools are on by default with loopback/VPS defaults (see
//! `STACK_DEFAULTS`); `/etc/openmind-mcp-stack/stack.conf` (or `--stack-conf`)
//! can override them (`key = value`). The only write tool,
//! `vault_note_create`, stays off unless that file says `vault_note_create = on`.
//! The key file is read once: the memory key and Hermes `API_SERVER_KEY`
//! (for read-only Galion One thread listing) are kept, nothing else.
//! Exit 2 = bad arguments (non-loopback memory URL, missing vault root).

#![forbid(unsafe_code)]

use openmind_mcp_stack::memory::{key_from_env_text, named_key_from_env_text, MemoryClient};
use openmind_mcp_stack::notes::NoteCreator;
use openmind_mcp_stack::stack::{SpendRules, Stack};
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
    let mut stack_conf: Option<PathBuf> = None;
    let mut no_stack = false;
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
            "--stack-conf" => stack_conf = Some(PathBuf::from(val("--stack-conf")?)),
            "--no-stack" => no_stack = true,
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
    let key_text = match &key_file {
        Some(p) => Some(std::fs::read_to_string(p).map_err(|e| format!("key file: {e}"))?),
        None => None,
    };
    let timeout = Duration::from_millis(timeout_ms.clamp(100, 60_000));
    let memory = if no_memory {
        None
    } else {
        let key = match &key_text {
            Some(t) => key_from_env_text(t),
            None => {
                let env: String = openmind_mcp_stack::memory::KEY_NAMES
                    .iter()
                    .filter_map(|k| std::env::var(k).ok().map(|v| format!("{k}={v}\n")))
                    .collect();
                key_from_env_text(&env)
            }
        };
        Some(MemoryClient::new(&url, key, timeout)?)
    };
    let (stack, notes) = if no_stack {
        (None, None)
    } else {
        let conf_text = match &stack_conf {
            Some(p) => std::fs::read_to_string(p).map_err(|e| format!("stack conf: {e}"))?,
            None => std::fs::read_to_string(DEFAULT_STACK_CONF).unwrap_or_default(),
        };
        let conf = parse_stack_conf(&conf_text)?;
        let hermes_key = key_text
            .as_deref()
            .and_then(|t| named_key_from_env_text(t, "API_SERVER_KEY"));
        let (s, n) = build_stack(&conf, hermes_key, timeout, vault.as_ref())?;
        (Some(s), n)
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
            stack,
            notes,
        }),
        check,
    ))
}

/// Default stack config file (optional; missing = built-in defaults).
const DEFAULT_STACK_CONF: &str = "/etc/openmind-mcp-stack/stack.conf";

/// Built-in defaults (live VPS layout, loopback only).
const STACK_DEFAULTS: [(&str, &str); 12] = [
    ("nerve_url", "http://127.0.0.1:8670"),
    ("cortex_url", "http://127.0.0.1:8674"),
    ("queue_url", "http://127.0.0.1:19120"),
    ("hermes_url", "http://127.0.0.1:8642"),
    ("spend_file", "/var/lib/hermes-rs/rs-spend.json"),
    ("backup_dir", "/var/backups/galion"),
    ("docs_root", "/opt/galion/galion-light/docs"),
    (
        "readiness_file",
        "/var/lib/openmind-mcp-stack/hermes-readiness.json",
    ),
    ("spend_baseline_usd", "3.449"),
    ("spend_limit_usd", "5"),
    ("spend_stop_usd", "4.5"),
    ("vault_note_create", "off"),
];

/// Parse `key = value` lines (`#` comments). Unknown keys are an error so a
/// typo cannot silently leave a default (or a write switch) in place.
fn parse_stack_conf(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .ok_or(format!("stack conf line {}: expected key = value", i + 1))?;
        let k = k.trim();
        let known = STACK_DEFAULTS.iter().any(|(d, _)| *d == k) || k == "vault_note_folder";
        if !known {
            return Err(format!("stack conf line {}: unknown key {k}", i + 1));
        }
        out.push((k.to_string(), v.trim().trim_matches('"').to_string()));
    }
    Ok(out)
}

fn off(v: &str) -> bool {
    matches!(
        v.to_ascii_lowercase().as_str(),
        "" | "off" | "no" | "false" | "0"
    )
}

fn build_stack(
    conf: &[(String, String)],
    hermes_key: Option<String>,
    timeout: Duration,
    vault: Option<&Vault>,
) -> Result<(Stack, Option<NoteCreator>), String> {
    let get = |k: &str| -> String {
        conf.iter()
            .rev()
            .find(|(ck, _)| ck == k)
            .map(|(_, v)| v.clone())
            .or_else(|| {
                STACK_DEFAULTS
                    .iter()
                    .find(|(d, _)| *d == k)
                    .map(|(_, v)| (*v).to_string())
            })
            .unwrap_or_default()
    };
    let client = |k: &str,
                  label: &'static str,
                  key: Option<String>|
     -> Result<Option<MemoryClient>, String> {
        let url = get(k);
        if off(&url) {
            return Ok(None);
        }
        MemoryClient::new(&url, key, timeout)
            .map(|c| Some(c.with_label(label)))
            .map_err(|e| format!("{k}: {}", e.replace("memory url", "url")))
    };
    let path = |k: &str| -> Option<PathBuf> {
        let v = get(k);
        (!off(&v)).then(|| PathBuf::from(v))
    };
    let usd = |k: &str| -> Result<f64, String> {
        get(k)
            .parse::<f64>()
            .ok()
            .filter(|x| x.is_finite() && *x >= 0.0)
            .ok_or(format!("{k} must be a number"))
    };
    let docs = match path("docs_root") {
        Some(p) if p.is_dir() => Some(Vault::open(&p)?),
        _ => None,
    };
    let stack = Stack {
        nerve: client("nerve_url", "nerve", None)?,
        cortex: client("cortex_url", "cortex", None)?,
        queue: client("queue_url", "queue", None)?,
        hermes: client("hermes_url", "hermes", hermes_key)?,
        spend_file: path("spend_file"),
        spend: SpendRules {
            baseline_usd: usd("spend_baseline_usd")?,
            limit_usd: usd("spend_limit_usd")?,
            stop_usd: usd("spend_stop_usd")?,
        },
        backup_dir: path("backup_dir"),
        docs,
        readiness_file: path("readiness_file"),
    };
    let switch = get("vault_note_create").to_ascii_lowercase();
    let notes = if off(&switch) {
        None
    } else if !matches!(switch.as_str(), "on" | "yes" | "true" | "1") {
        return Err(format!("vault_note_create must be on or off, not {switch}"));
    } else {
        let v = vault.ok_or("vault_note_create needs --vault")?;
        let folder = conf
            .iter()
            .rev()
            .find(|(k, _)| k == "vault_note_folder")
            .map_or(
                openmind_mcp_stack::notes::DEFAULT_FOLDER.to_string(),
                |(_, v)| v.clone(),
            );
        Some(NoteCreator::new(v.root(), &folder)?)
    };
    Ok((stack, notes))
}
