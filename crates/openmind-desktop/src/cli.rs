// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Command-line parsing. Pure: no environment reads, no sockets.

use crate::error::DesktopError;
use crate::files::DEFAULT_STACK;
use crate::http::parse_loopback;
use std::path::PathBuf;

/// Usage line printed for `--help` and for every rejected argument list.
pub const USAGE: &str = "\
usage: openmind-desktop [--listen [ADDR]] [--grant PATH]... [--inbox DIR]
                        [--stack ADDR|off] [--help | --version]
  (no flags)     print the control-console page and exit; no socket
  --listen       serve that page on 127.0.0.1:18081
  --listen ADDR  serve it on a loopback ADDR only (for example 127.0.0.1:0)
  --grant PATH   a file or directory the operator chose; repeatable
  --inbox DIR    store accepted files in this directory
  --stack ADDR   upload them to a loopback OpenMind intake (default 127.0.0.1:8091)
  --stack off    do not open a client
  --grant, --inbox and --stack are refused without --listen";

/// What the operator asked the binary to do when it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listen {
    /// `None` means [`crate::DEFAULT_BIND`].
    pub addr: Option<String>,
    /// Files and directories the operator chose. Empty means no disk reads.
    pub grants: Vec<PathBuf>,
    /// Where accepted files are stored. `None` means nothing is written.
    pub inbox: Option<PathBuf>,
    /// Loopback intake. `None` means do not dial.
    pub stack: Option<String>,
}

/// What the operator asked the binary to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Print the page on stdout. Opens no socket.
    Page,
    /// Serve the page on loopback.
    Listen(Listen),
    /// `--help` or `-h`.
    Help,
    /// `--version` or `-V` (also handled before `main` returns).
    Version,
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
            Mode::Listen(opts) => match &opts.addr {
                Some(addr) => Some(addr.as_str()),
                None => Some(crate::DEFAULT_BIND),
            },
            Mode::Page | Mode::Help | Mode::Version => None,
        }
    }
}

/// Parses arguments after the program name.
///
/// # Errors
/// [`DesktopError::Usage`] for any list other than the modes in [`USAGE`].
/// [`DesktopError::Refused`] when `--stack` is not loopback.
pub fn parse(args: &[String]) -> Result<Mode, DesktopError> {
    if args.is_empty() {
        return Ok(Mode::Page);
    }
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        return Ok(Mode::Help);
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        return Ok(Mode::Version);
    }
    let mut listen = false;
    let mut addr: Option<String> = None;
    let mut grants = Vec::new();
    let mut inbox: Option<PathBuf> = None;
    let mut stack_set = false;
    let mut stack: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--listen" => {
                if listen {
                    return unexpected("--listen");
                }
                listen = true;
                index += 1;
                if let Some(next) = args.get(index) {
                    if is_addr_token(next) {
                        addr = Some(next.clone());
                        index += 1;
                    }
                }
            }
            "--grant" => {
                grants.push(PathBuf::from(need_value(args, &mut index, "--grant")?));
            }
            "--inbox" => {
                if inbox.is_some() {
                    return unexpected("--inbox");
                }
                inbox = Some(PathBuf::from(need_value(args, &mut index, "--inbox")?));
            }
            "--stack" => {
                if stack_set {
                    return unexpected("--stack");
                }
                stack_set = true;
                let value = need_value(args, &mut index, "--stack")?;
                if value == "off" {
                    stack = None;
                } else {
                    parse_loopback(value)?;
                    stack = Some(value.to_string());
                }
            }
            other => return unexpected(other),
        }
    }
    if !listen {
        return unexpected(args[0].as_str());
    }
    if !stack_set {
        stack = Some(DEFAULT_STACK.to_string());
    }
    Ok(Mode::Listen(Listen {
        addr,
        grants,
        inbox,
        stack,
    }))
}

fn need_value<'a>(
    args: &'a [String],
    index: &mut usize,
    flag: &str,
) -> Result<&'a str, DesktopError> {
    *index += 1;
    match args.get(*index) {
        Some(value) if is_addr_token(value) => {
            *index += 1;
            Ok(value.as_str())
        }
        _ => Err(DesktopError::Usage(format!(
            "openmind-desktop: {flag} needs a value\n{USAGE}"
        ))),
    }
}

fn unexpected(arg: &str) -> Result<Mode, DesktopError> {
    Err(DesktopError::Usage(format!(
        "openmind-desktop: unexpected argument {arg:?}\n{USAGE}"
    )))
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

    fn listen(mode: Mode) -> super::Listen {
        match mode {
            Mode::Listen(opts) => opts,
            other => panic!("not listen: {other:?}"),
        }
    }

    #[test]
    fn modes() {
        assert_eq!(parse(&[]).unwrap(), Mode::Page);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Mode::Help);
        assert_eq!(parse(&args(&["-h"])).unwrap(), Mode::Help);
        assert_eq!(parse(&args(&["--version"])).unwrap(), Mode::Version);
        assert_eq!(parse(&args(&["-V"])).unwrap(), Mode::Version);
        let plain = listen(parse(&args(&["--listen"])).unwrap());
        assert_eq!(plain.addr, None);
        assert!(plain.grants.is_empty());
        assert_eq!(plain.stack.as_deref(), Some("127.0.0.1:8091"));
        let with_addr = listen(parse(&args(&["--listen", "127.0.0.1:9"])).unwrap());
        assert_eq!(with_addr.addr.as_deref(), Some("127.0.0.1:9"));
    }

    #[test]
    fn grants_inbox_and_stack_need_listen() {
        let opts = listen(
            parse(&args(&[
                "--grant",
                "notes.txt",
                "--inbox",
                "inbox",
                "--stack",
                "off",
                "--listen",
                "127.0.0.1:0",
            ]))
            .unwrap(),
        );
        assert_eq!(opts.grants.len(), 1);
        assert_eq!(
            opts.inbox.as_deref().map(|p| p.as_os_str()),
            Some(std::ffi::OsStr::new("inbox"))
        );
        assert!(opts.stack.is_none());
        assert_eq!(opts.addr.as_deref(), Some("127.0.0.1:0"));
        for bad in [
            vec!["--grant", "notes.txt"],
            vec!["--inbox", "inbox"],
            vec!["--stack", "off"],
            vec!["--stack", "10.1.2.3:8091", "--listen"],
        ] {
            let err = parse(&args(&bad)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad:?}");
        }
    }

    #[test]
    fn only_listen_serves() {
        assert!(Mode::Listen(super::Listen {
            addr: None,
            grants: Vec::new(),
            inbox: None,
            stack: None,
        })
        .serves());
        assert_eq!(
            Mode::Listen(super::Listen {
                addr: None,
                grants: Vec::new(),
                inbox: None,
                stack: None,
            })
            .bind_addr(),
            Some("127.0.0.1:18081")
        );
        assert!(!Mode::Page.serves());
        assert!(Mode::Page.bind_addr().is_none());
        assert!(!Mode::Help.serves());
        assert!(!Mode::Version.serves());
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
            vec!["--listen", "--grant"],
            vec!["--listen", "--stack"],
        ] {
            let err = parse(&args(&bad)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad:?}");
        }
        assert!(USAGE.contains("no socket"));
        assert!(USAGE.contains("loopback"));
    }
}
