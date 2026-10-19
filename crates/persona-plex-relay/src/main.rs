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

use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::ExitCode;

use persona_plex_relay::config::RelayConfig;
use persona_plex_relay::plan::{render, Environment};

fn env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn config_path() -> Result<String, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let from_env = std::env::var("PERSONA_PLEX_RELAY_CONFIG").ok();
    config_path_from(&args, from_env.as_deref())
}

fn config_path_from(args: &[String], env_config: Option<&str>) -> Result<String, String> {
    let mut iter = args.iter();
    if let Some(arg) = iter.next() {
        match arg.as_str() {
            "--config" => {
                return iter
                    .next()
                    .cloned()
                    .ok_or_else(|| "--config needs a path".to_string());
            }
            "-h" | "--help" => {
                return Err(
                    "usage: persona-plex-relay [--config PATH]  (scaffold: prints plan, starts nothing)"
                        .into(),
                );
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(match env_config {
        Some(value) if !value.trim().is_empty() => value.to_string(),
        _ => "persona-plex-relay.toml".to_string(),
    })
}

/// No-flag run whose config file is absent.
/// A flag, or any other I/O error, stays on today's failure path.
fn quiet_missing_config(no_flags: bool, kind: ErrorKind) -> bool {
    no_flags && kind == ErrorKind::NotFound
}

/// What the default or missing-config path emits.
/// `sockets_opened` stays zero: this path never binds or connects.
struct QuietStart {
    line: String,
    sockets_opened: u32,
}

fn quiet_start(now: &str) -> QuietStart {
    QuietStart {
        line: now.to_string(),
        sockets_opened: 0,
    }
}

enum ConfigLoad {
    Quiet(QuietStart),
    Ready(String),
    Failed(String),
}

fn load_config(
    no_flags: bool,
    path: &str,
    read: Result<String, Error>,
    now: impl FnOnce() -> String,
) -> ConfigLoad {
    match read {
        Ok(src) => ConfigLoad::Ready(src),
        Err(err) if quiet_missing_config(no_flags, err.kind()) => {
            ConfigLoad::Quiet(quiet_start(&now()))
        }
        Err(err) => ConfigLoad::Failed(format!("cannot read config {path}: {err}")),
    }
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
    let src = match load_config(
        no_flags,
        &path,
        std::fs::read_to_string(&path),
        zagreb_time::now_rfc3339,
    ) {
        ConfigLoad::Quiet(quiet) => {
            if quiet.sockets_opened != 0 {
                eprintln!("refusing to open a socket");
                return ExitCode::from(1);
            }
            println!("{}", quiet.line);
            return ExitCode::SUCCESS;
        }
        ConfigLoad::Failed(message) => {
            eprintln!("{message}");
            return ExitCode::from(1);
        }
        ConfigLoad::Ready(src) => src,
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
    use super::{config_path_from, load_config, quiet_missing_config, ConfigLoad};
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

    #[test]
    fn default_or_missing_config_opens_no_socket_and_prints_no_secret() {
        let stamp = "2026-10-19T18:17:00+02:00";
        let hidden = "fixture-value";

        let default_path = config_path_from(&[], None).expect("default path");
        assert_eq!(default_path, "persona-plex-relay.toml");
        assert_eq!(
            config_path_from(&[], Some("   ")).expect("blank env is the default path"),
            "persona-plex-relay.toml"
        );

        let missing = format!("/tmp/{hidden}.toml");
        let configured = config_path_from(&[], Some(missing.as_str())).expect("env path");
        assert_eq!(configured, missing);

        let before = socket_fds();
        let default_load = load_config(
            true,
            &default_path,
            Err(std::io::Error::new(ErrorKind::NotFound, hidden)),
            || stamp.to_string(),
        );
        let missing_load = load_config(
            true,
            &configured,
            Err(std::io::Error::new(ErrorKind::NotFound, hidden)),
            || stamp.to_string(),
        );
        let after = socket_fds();
        assert_eq!(before, after, "default or missing config opened a socket");

        assert_quiet(default_load, stamp, hidden);
        assert_quiet(missing_load, stamp, hidden);
    }

    fn assert_quiet(loaded: ConfigLoad, stamp: &str, hidden: &str) {
        let ConfigLoad::Quiet(quiet) = loaded else {
            panic!("missing config must stay on the quiet path");
        };
        assert_eq!(quiet.sockets_opened, 0);
        assert_eq!(quiet.line, stamp);
        assert!(!quiet.line.contains(hidden));
        assert!(!quiet.line.contains('\n'));
        assert!(quiet.line.ends_with("+01:00") || quiet.line.ends_with("+02:00"));
        assert!(!quiet.line.contains('Z'));
    }

    fn socket_fds() -> Vec<String> {
        let mut names = Vec::new();
        let dir = std::fs::read_dir("/proc/self/fd").expect("fd");
        for entry in dir.flatten() {
            if let Ok(target) = std::fs::read_link(entry.path()) {
                let text = target.to_string_lossy().into_owned();
                if text.starts_with("socket:") {
                    names.push(text);
                }
            }
        }
        names.sort();
        names
    }
}
