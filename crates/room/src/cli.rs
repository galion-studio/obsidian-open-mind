// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Command-line parsing. Pure: no environment reads, no I/O.

use crate::error::RoomError;
use std::path::PathBuf;

/// The usage line printed with `--help` and with every usage error.
pub const USAGE: &str = "usage: room [--fixture PATH | --listen | --help | --version]";

/// What the operator asked room to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// No flags: print the JSON page of the configured roots and exit.
    Page,
    /// `--fixture PATH`: print the JSON page of one file or directory and exit.
    Fixture(PathBuf),
    /// `--listen`: serve the page over HTTP. The only mode that opens a socket.
    Listen,
    /// `--help` or `-h`.
    Help,
    /// `--version` or `-V`.
    Version,
}

impl Mode {
    /// True only for [`Mode::Listen`]. Every other mode stays off the network.
    #[must_use]
    pub fn serves(&self) -> bool {
        matches!(self, Mode::Listen)
    }
}

/// Parses the arguments after the program name.
///
/// Parsing is strict: an unknown flag, a missing `--fixture` path, or extra
/// arguments are a [`RoomError::Usage`] and never start the server.
///
/// # Errors
/// [`RoomError::Usage`] for any argument list other than the five modes.
pub fn parse(args: &[String]) -> Result<Mode, RoomError> {
    let strs: Vec<&str> = args.iter().map(String::as_str).collect();
    match strs.as_slice() {
        [] => Ok(Mode::Page),
        ["--listen"] => Ok(Mode::Listen),
        ["--help" | "-h"] => Ok(Mode::Help),
        ["--version" | "-V"] => Ok(Mode::Version),
        ["--fixture", path] if !path.is_empty() => Ok(Mode::Fixture(PathBuf::from(path))),
        ["--fixture"] | ["--fixture", _] => {
            Err(RoomError::Usage("room: --fixture needs a path".into()))
        }
        ["--listen", ..] => Err(RoomError::Usage(
            "room: --listen takes no arguments (set ROOM_BIND for the address)".into(),
        )),
        [first, ..] => Err(RoomError::Usage(format!(
            "room: unexpected argument {first:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Mode};
    use std::path::PathBuf;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn modes() {
        assert_eq!(parse(&[]).unwrap(), Mode::Page);
        assert_eq!(parse(&args(&["--listen"])).unwrap(), Mode::Listen);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Mode::Help);
        assert_eq!(parse(&args(&["-h"])).unwrap(), Mode::Help);
        assert_eq!(parse(&args(&["--version"])).unwrap(), Mode::Version);
        assert_eq!(parse(&args(&["-V"])).unwrap(), Mode::Version);
        assert_eq!(
            parse(&args(&["--fixture", "/tmp/x"])).unwrap(),
            Mode::Fixture(PathBuf::from("/tmp/x"))
        );
    }

    #[test]
    fn only_listen_serves() {
        assert!(Mode::Listen.serves());
        for mode in [
            Mode::Page,
            Mode::Fixture(PathBuf::from("x")),
            Mode::Help,
            Mode::Version,
        ] {
            assert!(!mode.serves(), "{mode:?}");
        }
    }

    #[test]
    fn usage_errors_never_serve() {
        for bad in [
            vec!["--fixture"],
            vec!["--fixture", ""],
            vec!["--fixture", "a", "b"],
            vec!["--listen", "0.0.0.0:80"],
            vec!["--bogus"],
            vec!["listen"],
            vec!["--fixture", "a", "--listen"],
        ] {
            let err = parse(&args(&bad)).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad:?}");
        }
    }
}
