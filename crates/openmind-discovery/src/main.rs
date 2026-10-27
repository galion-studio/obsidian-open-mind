// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! LAN discovery CLI. With no arguments this prints the seed candidates and
//! opens no socket. `find` browses mDNS (and probes only with `--probe`),
//! `approve` / `deny` / `join` / `list` manage provider rows, `announce`
//! answers mDNS for given services. See `--help`.

use openmind_discovery::{run, CliError, CliOut, USAGE};
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // NERVE push (no-op unless NERVE_URL is set): started.
    nerve_client::event("openmind-discovery", "started", "cli");
    match run(env::args().skip(1).collect()) {
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
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            eprint!("{USAGE}");
            if msg.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(CliError::Fail(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(1)
        }
    }
}
