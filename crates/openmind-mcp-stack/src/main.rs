// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Stdio entry for the read-only MCP stub.
//!
//! DESK-ID: openmind.B57
//!
//! Crate names are the process arguments. The process reads JSON-RPC lines
//! from stdin, writes one response line to stdout, and flushes. It exits when
//! stdin reaches EOF. It does not bind a port and it does not spawn.

#![forbid(unsafe_code)]

use openmind_mcp_stack::StackServer;
use std::env;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // `--version` / `-V` first: no config, bind or I/O (release rehearsal, slot 216).
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V")) {
        println!("openmind-mcp-stack {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    // `--help` / `-h`: usage only, not read as a crate name (release rehearsal, slot 228).
    if matches!(std::env::args().nth(1).as_deref(), Some("--help" | "-h")) {
        println!(
            "usage: openmind-mcp-stack [CRATE]...  (read-only MCP stub: JSON-RPC lines on stdin, one reply line each on stdout; no port, no spawn)"
        );
        std::process::exit(0);
    }
    let server = match StackServer::new(env::args().skip(1)) {
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
