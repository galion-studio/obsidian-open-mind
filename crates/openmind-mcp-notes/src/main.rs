// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Stdio entry for the research-vault MCP server.
//!
//! DESK-ID: openmind.MCP-NOTES
//!
//! The vault is the first argument, or `OPENMIND_NOTES_VAULT` when there is
//! no argument. The process reads JSON-RPC lines from stdin and writes one
//! response line to stdout. It exits when stdin reaches EOF. It does not
//! bind a port and it does not spawn.

#![forbid(unsafe_code)]

use openmind_mcp_notes::NotesServer;
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = vault_arg() else {
        let _ = writeln!(
            io::stderr(),
            "usage: openmind-mcp-notes <vault>  (or set OPENMIND_NOTES_VAULT)"
        );
        return ExitCode::from(2);
    };
    let server = match NotesServer::open(&path) {
        Ok(server) => server,
        Err(err) => {
            let _ = writeln!(io::stderr(), "{err}");
            return ExitCode::from(2);
        }
    };
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                let _ = writeln!(io::stderr(), "{err}");
                return ExitCode::from(1);
            }
        };
        let mut framed = line;
        framed.push('\n');
        let out = server.handle_bytes(framed.as_bytes());
        if out.is_empty() {
            continue;
        }
        if stdout.write_all(&out).is_err() || stdout.flush().is_err() {
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

fn vault_arg() -> Option<PathBuf> {
    let mut args = env::args().skip(1);
    if let Some(path) = args.next() {
        if args.next().is_some() || path.is_empty() {
            return None;
        }
        return Some(PathBuf::from(path));
    }
    env::var("OPENMIND_NOTES_VAULT")
        .ok()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}
