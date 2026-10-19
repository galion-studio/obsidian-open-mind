// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Print discovery candidates as JSON.
//!
//! With no flags this reads nothing and opens no socket. `--fixture` parses a
//! local mDNS-SD style file. `--scan` is off by default; this stub sends no
//! packets when it is set.

use openmind_discovery::{cli, read_fixture_file, CliError, CliOut, USAGE};
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match cli(env::args().skip(1), read_fixture_file) {
        Ok(CliOut::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(CliOut::Report(value)) => {
            let mut stdout = io::stdout().lock();
            if serde_json::to_writer(&mut stdout, &value).is_err() {
                return ExitCode::from(1);
            }
            if stdout.write_all(b"\n").is_err() {
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(CliError::Usage(msg)) => {
            eprintln!("{msg}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Fail(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(1)
        }
    }
}
