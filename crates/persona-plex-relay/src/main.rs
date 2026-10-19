// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! persona-plex-relay (scaffold): load config, print the plan, exit.
//! Opens no sockets, starts no listener, connects to nothing.
//!
//! Usage: persona-plex-relay [--config PATH]
//! Config path order: --config, then $PERSONA_PLEX_RELAY_CONFIG, then
//! ./persona-plex-relay.toml.
//!
//! No flags and a missing config file: print one Europe/Zagreb RFC 3339
//! timestamp and exit 0. That path does not open a socket, read a token,
//! or print a secret. A present config keeps the plan path.

use std::io::ErrorKind;
use std::path::Path;
use std::process::ExitCode;

use persona_plex_relay::config::RelayConfig;
use persona_plex_relay::plan::{render, Environment};

fn env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn config_path() -> Result<String, String> {
    let mut args = std::env::args().skip(1);
    if let Some(a) = args.next() {
        match a.as_str() {
            "--config" => return args.next().ok_or_else(|| "--config needs a path".to_string()),
            "-h" | "--help" => {
                return Err("usage: persona-plex-relay [--config PATH]  (scaffold: prints plan, starts nothing)".into())
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(std::env::var("PERSONA_PLEX_RELAY_CONFIG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "persona-plex-relay.toml".to_string()))
}

/// No-flag run whose config file is absent.
/// A flag, or any other I/O error, stays on today's failure path.
fn quiet_missing_config(no_flags: bool, kind: ErrorKind) -> bool {
    no_flags && kind == ErrorKind::NotFound
}

fn main() -> ExitCode {
    let no_flags = std::env::args().nth(1).is_none();
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if quiet_missing_config(no_flags, e.kind()) => {
            // One Europe/Zagreb stamp. No socket, no token, no secret.
            println!("{}", zagreb_time::now_rfc3339());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("cannot read config {path}: {e}");
            return ExitCode::from(1);
        }
    };
    let cfg = match RelayConfig::from_toml(&src) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(1);
        }
    };
    let env = Environment {
        now: zagreb_time::now_rfc3339(),
        auth_token_set: env_is_set(&cfg.relay.auth_token_env),
        store_key_set: env_is_set(&cfg.speaker.store_key_env),
        role_prompt_markdown: std::fs::read_to_string(&cfg.upstream.role_prompt_file).ok(),
        speaker_model_present: Path::new(&cfg.speaker.model_path).is_file(),
    };
    println!("config: {path}");
    for line in render(&cfg, &env) {
        println!("{line}");
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::quiet_missing_config;
    use std::io::ErrorKind;

    #[test]
    fn no_flag_missing_file_is_quiet() {
        assert!(quiet_missing_config(true, ErrorKind::NotFound));
    }

    #[test]
    fn flag_or_other_io_stays_loud() {
        assert!(!quiet_missing_config(false, ErrorKind::NotFound));
        assert!(!quiet_missing_config(true, ErrorKind::PermissionDenied));
    }
}
