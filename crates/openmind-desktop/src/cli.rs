// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Command-line parsing. Pure: no environment reads, no sockets.
//!
//! `--probe SPEC` checks the spec here. `OPENMIND_DESKTOP_PROBES` is read
//! later, only for `--probe` without a spec and for `--listen`.

use crate::error::DesktopError;
use crate::probe::ProbeConfig;

/// Usage line printed for `--help` and for every rejected argument list.
pub const USAGE: &str = "\
usage: openmind-desktop [--listen [ADDR] | --probe [SPEC] | --help | --version]
  (no flags)     print the control-console page and exit; no socket
  --listen       serve that page on 127.0.0.1:18081
  --listen ADDR  serve it on a loopback ADDR only (for example 127.0.0.1:0)
  --probe        GET nerve, cortex, console, hermes and voice; print the report
  --probe SPEC   same, with SPEC (name=host:port/path, comma-separated)
                 timeout=MILLISECONDS (50 to 10000) is one SPEC entry
                 name=- skips that service and opens no socket for it
                 OPENMIND_DESKTOP_PROBES is used when SPEC is omitted";

/// What the operator asked the binary to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Print the page on stdout. Opens no socket.
    Page,
    /// Serve the page. `None` means [`crate::DEFAULT_BIND`].
    Listen(Option<String>),
    /// `--help` or `-h`.
    Help,
    /// `--version` or `-V` (also handled before `main` returns).
    Version,
    /// Read-only health report. `None` means defaults, or
    /// `OPENMIND_DESKTOP_PROBES` when that variable is set.
    Probe(Option<String>),
}

impl Mode {
    /// True only when this mode is allowed to open a socket.
    #[must_use]
    pub fn serves(&self) -> bool {
        matches!(self, Mode::Listen(_))
    }

    /// Address to bind, if this mode serves.
    #[must_use]
    pub fn bind_addr(&self) -> Option<&str> {
        match self {
            Mode::Listen(Some(addr)) => Some(addr.as_str()),
            Mode::Listen(None) => Some(crate::DEFAULT_BIND),
            Mode::Page | Mode::Help | Mode::Version | Mode::Probe(_) => None,
        }
    }
}

/// Parses arguments after the program name.
///
/// # Errors
/// [`DesktopError::Usage`] for any list other than the modes in [`USAGE`].
pub fn parse(args: &[String]) -> Result<Mode, DesktopError> {
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    match strs.as_slice() {
        [] => Ok(Mode::Page),
        ["--help" | "-h"] => Ok(Mode::Help),
        ["--version" | "-V"] => Ok(Mode::Version),
        ["--listen"] => Ok(Mode::Listen(None)),
        ["--listen", addr] if is_addr_token(addr) => Ok(Mode::Listen(Some((*addr).to_string()))),
        ["--listen", _] => Err(DesktopError::Usage(
            "openmind-desktop: --listen needs a loopback address".into(),
        )),
        ["--probe"] => Ok(Mode::Probe(None)),
        ["--probe", spec] if is_addr_token(spec) => {
            ProbeConfig::parse(spec)
                .map_err(|err| DesktopError::Usage(format!("openmind-desktop: {err}\n{USAGE}")))?;
            Ok(Mode::Probe(Some((*spec).to_string())))
        }
        ["--probe", _] => Err(DesktopError::Usage(format!(
            "openmind-desktop: --probe needs a spec (name=host:port/path)\n{USAGE}"
        ))),
        [first, ..] => Err(DesktopError::Usage(format!(
            "openmind-desktop: unexpected argument {first:?}\n{USAGE}"
        ))),
    }
}

fn is_addr_token(addr: &str) -> bool {
    !addr.is_empty() && !addr.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::{parse, Mode, USAGE};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn modes() {
        assert_eq!(parse(&[]).unwrap(), Mode::Page);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Mode::Help);
        assert_eq!(parse(&args(&["-h"])).unwrap(), Mode::Help);
        assert_eq!(parse(&args(&["--version"])).unwrap(), Mode::Version);
        assert_eq!(parse(&args(&["-V"])).unwrap(), Mode::Version);
        assert_eq!(parse(&args(&["--listen"])).unwrap(), Mode::Listen(None));
        assert_eq!(
            parse(&args(&["--listen", "127.0.0.1:9"])).unwrap(),
            Mode::Listen(Some("127.0.0.1:9".into()))
        );
        assert_eq!(parse(&args(&["--probe"])).unwrap(), Mode::Probe(None));
        assert_eq!(
            parse(&args(&["--probe", "voice=-"])).unwrap(),
            Mode::Probe(Some("voice=-".into()))
        );
    }

    #[test]
    fn only_listen_serves() {
        assert!(Mode::Listen(None).serves());
        assert_eq!(Mode::Listen(None).bind_addr(), Some("127.0.0.1:18081"));
        assert!(!Mode::Page.serves());
        assert!(Mode::Page.bind_addr().is_none());
        assert!(!Mode::Help.serves());
        assert!(!Mode::Version.serves());
        assert!(!Mode::Probe(None).serves());
        assert!(Mode::Probe(None).bind_addr().is_none());
    }

    #[test]
    fn bad_args_do_not_serve() {
        for bad in [
            vec!["--listen", ""],
            vec!["--listen", "-"],
            vec!["--listen", "--help"],
            vec!["--bogus"],
            vec!["listen"],
            vec!["--page"],
            vec!["--probe", ""],
            vec!["--probe", "--listen"],
            vec!["--probe", "nope=localhost:1/health"],
            vec!["--probe", "nerve=https://localhost:1/health"],
        ] {
            let err = parse(&args(&bad)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad:?}");
        }
        assert!(USAGE.contains("no socket"));
    }
}
