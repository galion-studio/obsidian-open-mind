// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Non-binding `--version`, `--help` and `--check` for every deployable
//! binary (the tuner pattern, ops plan S2.2).
//!
//! Call [`early`] first thing in `main`, before any config is loaded, any
//! file is opened, any socket is bound or any startup check runs:
//!
//! ```no_run
//! galion_common::flags::early("tuner", env!("CARGO_PKG_VERSION"), "usage: tuner\n", || {
//!     Ok(String::from("bind 127.0.0.1:8650\n"))
//! });
//! ```
//!
//! * `--version` / `-V` as the first argument: print
//!   [`crate::build_info::version_line`] and exit 0 (same rule as
//!   [`crate::build_info::exit_if_version`], so `galion-deploy`'s
//!   `<new> --version` probe always gets the stamped line).
//! * `--help` / `-h` as the ONLY argument: print the usage and exit 0.
//! * `--check` as the ONLY argument: run the binary's check closure, print
//!   its report and `check ok` (exit 0) or `check failed: ...` (exit 1).
//!   The closure only resolves settings and reads files; it must never bind,
//!   connect, spawn or write.
//!
//! `--help` and `--check` followed by more arguments fall through to the
//! binary's own parser, so no existing subcommand changes meaning.

use crate::build_info::{version_line, version_requested};

/// Which early flag the arguments ask for, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Early {
    /// `--version` or `-V` first.
    Version,
    /// `--help` or `-h`, alone.
    Help,
    /// `--check`, alone.
    Check,
}

/// Classify the full argument list (argv\[0\] included).
pub fn classify<I, S>(args: I) -> Option<Early>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let v: Vec<S> = args.into_iter().collect();
    if version_requested(v.iter().map(AsRef::as_ref)) {
        return Some(Early::Version);
    }
    if v.len() != 2 {
        return None;
    }
    match v[1].as_ref() {
        "--help" | "-h" => Some(Early::Help),
        "--check" => Some(Early::Check),
        _ => None,
    }
}

/// What the process prints and its exit code for an early flag. Pure, so it
/// is unit tested without exiting.
pub fn render<F>(
    flag: Early,
    name: &str,
    version: &str,
    usage: &str,
    check: F,
) -> (String, i32)
where
    F: FnOnce() -> Result<String, String>,
{
    match flag {
        Early::Version => (format!("{}\n", version_line(name, version)), 0),
        Early::Help => {
            let mut s = String::from(usage);
            if !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(COMMON_HELP);
            (s, 0)
        }
        Early::Check => {
            let (mut report, code, tail) = match check() {
                Ok(r) => (r, 0, String::from("check ok")),
                Err(e) => (String::new(), 1, format!("check failed: {e}")),
            };
            if !report.is_empty() && !report.ends_with('\n') {
                report.push('\n');
            }
            report.push_str(&format!("{name} {tail}\n"));
            (report, code)
        }
    }
}

/// Appended to every `--help`.
pub const COMMON_HELP: &str = "common flags (never bind, connect, spawn or write):\n  \
--version, -V  print the version line and exit 0\n  \
--help, -h     print this help and exit 0\n  \
--check        resolve settings, check the files they name, print a report; exit 0 ok, 1 not ok\n";

/// Handle `--version`, `--help` and `--check` and exit; return normally
/// when none was asked for.
pub fn early<F>(name: &str, version: &str, usage: &str, check: F)
where
    F: FnOnce() -> Result<String, String>,
{
    if let Some(flag) = classify(std::env::args()) {
        let (out, code) = render(flag, name, version, usage, check);
        use std::io::Write;
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(out.as_bytes());
        let _ = so.flush();
        std::process::exit(code);
    }
}

/// `--check` helper: one `key value` report line.
pub fn line(report: &mut String, key: &str, value: impl std::fmt::Display) {
    report.push_str(&format!("{key} {value}\n"));
}

/// `--check` helper: report a path that must exist (file or directory, as
/// asked). Never creates it. Returns false (and writes `missing`) when it is
/// absent or the wrong kind.
pub fn path_exists(report: &mut String, key: &str, path: &std::path::Path, dir: bool) -> bool {
    let ok = match std::fs::metadata(path) {
        Ok(m) => m.is_dir() == dir,
        Err(_) => false,
    };
    let kind = if dir { "dir" } else { "file" };
    let state = if ok { "ok" } else { "missing" };
    report.push_str(&format!("{key} {} ({kind} {state})\n", path.display()));
    ok
}

/// `--check` helper: a bind address must parse as `host:port` (no DNS
/// lookup, no bind).
pub fn bind_addr(report: &mut String, key: &str, addr: &str) -> Result<(), String> {
    report.push_str(&format!("{key} {addr}\n"));
    let (host, port) = addr
        .rsplit_once(':')
        .ok_or_else(|| format!("{key} {addr}: expected host:port"))?;
    port.parse::<u16>()
        .map_err(|_| format!("{key} {addr}: bad port"))?;
    if host.is_empty() {
        return Err(format!("{key} {addr}: empty host"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_rules() {
        assert_eq!(classify(["b", "--version"]), Some(Early::Version));
        assert_eq!(classify(["b", "-V", "x"]), Some(Early::Version));
        assert_eq!(classify(["b", "--help"]), Some(Early::Help));
        assert_eq!(classify(["b", "-h"]), Some(Early::Help));
        assert_eq!(classify(["b", "--check"]), Some(Early::Check));
        assert_eq!(classify(["b", "--check", "x"]), None);
        assert_eq!(classify(["b", "--help", "sub"]), None);
        assert_eq!(classify(["b"]), None);
        assert_eq!(classify(["b", "serve", "--version"]), None);
        assert_eq!(classify(["b", "--checks"]), None);
        let empty: [&str; 0] = [];
        assert_eq!(classify(empty), None);
    }

    #[test]
    fn render_version_help_check() {
        let never = || -> Result<String, String> { panic!("check must not run") };
        let (o, c) = render(Early::Version, "x", "1.0", "u", never);
        assert_eq!(c, 0);
        assert_eq!(o, format!("{}\n", version_line("x", "1.0")));
        let (o, c) = render(Early::Help, "x", "1.0", "usage: x", never);
        assert_eq!(c, 0);
        assert!(o.starts_with("usage: x\ncommon flags"), "{o}");
        assert!(o.contains("--check"));
        let (o, c) = render(Early::Check, "x", "1.0", "u", || Ok("bind a:1".into()));
        assert_eq!((o.as_str(), c), ("bind a:1\nx check ok\n", 0));
        let (o, c) = render(Early::Check, "x", "1.0", "u", || Err("no root".into()));
        assert_eq!((o.as_str(), c), ("x check failed: no root\n", 1));
    }

    #[test]
    fn helpers() {
        let mut r = String::new();
        assert!(bind_addr(&mut r, "bind", "127.0.0.1:8650").is_ok());
        assert!(bind_addr(&mut r, "bind", "[::1]:80").is_ok());
        assert!(bind_addr(&mut r, "bind", "127.0.0.1").is_err());
        assert!(bind_addr(&mut r, "bind", "h:99999").is_err());
        assert!(bind_addr(&mut r, "bind", ":80").is_err());
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(path_exists(&mut r, "d", here, true));
        assert!(!path_exists(&mut r, "f", here, false));
        assert!(!path_exists(&mut r, "f", &here.join("nope"), false));
        assert!(r.contains("(file missing)"));
        line(&mut r, "k", 5);
        assert!(r.ends_with("k 5\n"));
    }
}
