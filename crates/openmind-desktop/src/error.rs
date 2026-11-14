// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Errors that stop the `openmind-desktop` process.

use std::fmt;
use std::io;

/// Why the desktop binary stopped before or during a listen.
#[derive(Debug)]
pub enum DesktopError {
    /// The command line was not one of the documented modes.
    Usage(String),
    /// The address is not loopback, so the process refused to bind it.
    Refused(String),
    /// Loopback bind failed (port in use, or the OS rejected it).
    Bind {
        /// Address that was passed to the OS.
        addr: String,
        /// The OS error.
        source: io::Error,
    },
    /// A probe report was printed, and at least one enabled service was not up.
    Probes(String),
    /// A grant, the inbox, or the stack address was rejected before listen.
    Files(String),
}

impl DesktopError {
    /// Process exit code: 2 for a bad command or a refused address, 1 for a
    /// bind that the OS rejected or a probe report that was not all up.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            DesktopError::Usage(_) | DesktopError::Refused(_) | DesktopError::Files(_) => 2,
            DesktopError::Bind { .. } | DesktopError::Probes(_) => 1,
        }
    }
}

impl fmt::Display for DesktopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesktopError::Usage(msg)
            | DesktopError::Refused(msg)
            | DesktopError::Probes(msg)
            | DesktopError::Files(msg) => f.write_str(msg),
            DesktopError::Bind { addr, source } => {
                write!(f, "openmind-desktop: cannot bind {addr}: {source}")
            }
        }
    }
}

impl std::error::Error for DesktopError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DesktopError::Bind { source, .. } => Some(source),
            DesktopError::Usage(_)
            | DesktopError::Refused(_)
            | DesktopError::Probes(_)
            | DesktopError::Files(_) => None,
        }
    }
}
