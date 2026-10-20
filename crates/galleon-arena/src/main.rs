// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `galleon-arena list` prints the seed gladiators.
//!
//! The timestamp is Europe/Zagreb RFC 3339 from `TZ=Europe/Zagreb date -Iseconds`.
//! It never ends in `Z`. The command reads seed files only. It does not open a
//! socket. Scoring in this crate is paper integers. This binary does not score
//! and does not transfer a value.

#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use galleon_arena::{dispatch, read_zagreb_now, CliError, CliOut, USAGE};

fn wants_stamp(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("list") => !args
            .iter()
            .skip(1)
            .any(|arg| arg == "-h" || arg == "--help"),
        _ => false,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let at = if wants_stamp(&args) {
        match read_zagreb_now() {
            Ok(stamp) => stamp,
            Err(CliError::Failed(msg)) => {
                eprintln!("{msg}");
                return ExitCode::from(1);
            }
            Err(CliError::Usage(msg)) => {
                eprintln!("{msg}");
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    } else {
        String::new()
    };
    match dispatch(&args, &at) {
        Ok(CliOut::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(CliOut::List(text)) => {
            let mut stdout = io::stdout().lock();
            if stdout.write_all(text.as_bytes()).is_err() {
                return ExitCode::from(1);
            }
            if stdout.flush().is_err() {
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Err(CliError::Usage(msg)) => {
            eprintln!("{msg}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Failed(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(1)
        }
    }
}
