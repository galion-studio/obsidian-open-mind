// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Command line. The live unit starts the binary with no arguments, and that
//! path is unchanged: it serves. The flags exist for the cutover tooling:
//! `galion-deploy` runs `<new-binary> --version` before it installs, and the
//! live binary (which ignores arguments) would try to bind 8650 at that point.

use std::fs;

use crate::http::FILES;

/// What the process should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No arguments: bind and serve (the live behaviour).
    Serve,
    /// `--version` or `-V`: print [`version_line`] and exit 0. Never binds.
    Version,
    /// `--help` or `-h`: print [`USAGE`] and exit 0. Never binds.
    Help,
    /// `--check`: resolve settings, check the whitelisted files, exit 0 or 1. Never binds.
    Check,
}

/// Help text.
pub const USAGE: &str = "usage: tuner [--version | --help | --check]\n\
  no arguments  serve the whitelist from TUNER_ROOT on TUNER_ADDR\n\
  --version     print the version and exit (does not bind)\n\
  --check       check TUNER_ROOT holds every whitelisted file and exit (does not bind)\n\
env: TUNER_ADDR (default 127.0.0.1:8650), TUNER_ROOT (default /opt/galion/tuner/www), TUNER_CONFIG (optional file)\n";

/// `tuner <version>`, one line.
pub fn version_line() -> String {
    format!("tuner {}", env!("CARGO_PKG_VERSION"))
}

/// Parse the arguments after argv\[0\]. Unknown or extra arguments are an error,
/// so a typo never ends up binding a port.
pub fn parse_args(args: &[String]) -> Result<Action, String> {
    match args {
        [] => Ok(Action::Serve),
        [one] => match one.as_str() {
            "--version" | "-V" => Ok(Action::Version),
            "--help" | "-h" => Ok(Action::Help),
            "--check" => Ok(Action::Check),
            other => Err(format!("unknown argument {other}")),
        },
        _ => Err(String::from("expected at most one argument")),
    }
}

/// One line per whitelisted file under `root`. `Ok` when every file is a
/// readable regular file, `Err` with the same report otherwise.
pub fn check_root(root: &str) -> Result<String, String> {
    let mut report = String::new();
    let mut ok = true;
    let mut seen: Vec<&str> = Vec::new();
    for file in FILES {
        if seen.contains(&file.name) {
            continue;
        }
        seen.push(file.name);
        let path = format!("{root}/{}", file.name);
        match fs::read(&path) {
            Ok(body) => report.push_str(&format!("ok {path} {} bytes\n", body.len())),
            Err(err) => {
                ok = false;
                report.push_str(&format!("missing {path}: {err}\n"));
            }
        }
    }
    if ok {
        Ok(report)
    } else {
        Err(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn no_args_serves_like_the_live_unit() {
        assert_eq!(parse_args(&[]), Ok(Action::Serve));
    }

    #[test]
    fn flags_never_serve() {
        assert_eq!(parse_args(&args(&["--version"])), Ok(Action::Version));
        assert_eq!(parse_args(&args(&["-V"])), Ok(Action::Version));
        assert_eq!(parse_args(&args(&["--help"])), Ok(Action::Help));
        assert_eq!(parse_args(&args(&["-h"])), Ok(Action::Help));
        assert_eq!(parse_args(&args(&["--check"])), Ok(Action::Check));
        assert!(parse_args(&args(&["--serve"])).is_err());
        assert!(parse_args(&args(&["--version", "--check"])).is_err());
        assert_eq!(
            version_line(),
            format!("tuner {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn check_root_lists_each_file_once() {
        let www = format!("{}/www", env!("CARGO_MANIFEST_DIR"));
        let report = check_root(&www).expect("bundled www is complete");
        assert_eq!(report.lines().count(), 4, "{report}");
        assert!(report.contains("/index.html 2356 bytes"), "{report}");

        let err = check_root("/nonexistent-tuner-root").unwrap_err();
        assert_eq!(err.lines().filter(|l| l.starts_with("missing ")).count(), 4);
    }
}
