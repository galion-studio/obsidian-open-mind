// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! `gaia` binary. Dry-run only. One shot: no daemon, no loop.

#![deny(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let override_env = env::var("GAIA_OVERRIDE").ok();
    let now = gaia_core::now_zagreb();
    let out = gaia_core::run(&refs, override_env.as_deref(), &now);
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    if stdout.write_all(out.stdout.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    if stderr.write_all(out.stderr.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(out.code)
}
