// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The one error type of the room crate.

use std::fmt;
use std::io;

/// Everything that can make the room binary stop with a non-zero exit code.
///
/// Per-request failures inside the server (a client that hangs up, a file that
/// cannot be read) are not errors of this type: they become HTTP status codes
/// and never stop the listener.
#[derive(Debug)]
pub enum RoomError {
    /// The command line was not understood. The string names the problem.
    Usage(String),
    /// The listen address could not be bound (port in use, bad address, ...).
    Bind {
        /// The address that was tried, exactly as configured.
        addr: String,
        /// The underlying OS error.
        source: io::Error,
    },
    /// Any other I/O failure that stops the process.
    Io(io::Error),
}

impl RoomError {
    /// Process exit code for this error: 2 for usage errors, 1 otherwise.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            RoomError::Usage(_) => 2,
            RoomError::Bind { .. } | RoomError::Io(_) => 1,
        }
    }
}

impl fmt::Display for RoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoomError::Usage(msg) => write!(f, "{msg}\n{}", crate::cli::USAGE),
            RoomError::Bind { addr, source } => write!(f, "room: cannot bind {addr}: {source}"),
            RoomError::Io(err) => write!(f, "room: i/o error: {err}"),
        }
    }
}

impl std::error::Error for RoomError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RoomError::Usage(_) => None,
            RoomError::Bind { source, .. } => Some(source),
            RoomError::Io(err) => Some(err),
        }
    }
}

impl From<io::Error> for RoomError {
    fn from(err: io::Error) -> Self {
        RoomError::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::RoomError;
    use std::error::Error;
    use std::io;

    #[test]
    fn exit_codes() {
        assert_eq!(RoomError::Usage("x".into()).exit_code(), 2);
        let bind = RoomError::Bind {
            addr: "127.0.0.1:1".into(),
            source: io::Error::from(io::ErrorKind::AddrInUse),
        };
        assert_eq!(bind.exit_code(), 1);
        assert_eq!(RoomError::from(io::Error::other("x")).exit_code(), 1);
    }

    #[test]
    fn display_and_source() {
        let usage = RoomError::Usage("unknown flag --x".into()).to_string();
        assert!(usage.starts_with("unknown flag --x\nusage: room"));
        let bind = RoomError::Bind {
            addr: "127.0.0.1:19080".into(),
            source: io::Error::from(io::ErrorKind::AddrInUse),
        };
        assert!(bind.to_string().contains("cannot bind 127.0.0.1:19080"));
        assert!(bind.source().is_some());
        assert!(RoomError::Usage(String::new()).source().is_none());
        let io_err = RoomError::Io(io::Error::other("disk"));
        assert!(io_err.to_string().contains("disk"));
        assert!(io_err.source().is_some());
    }
}
