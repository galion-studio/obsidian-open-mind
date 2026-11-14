// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `openmind-desktop` binary. See the library docs for the modes.

use openmind_desktop::{DesktopError, Mode, USAGE};
use std::process::ExitCode;

fn run() -> Result<(), DesktopError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = openmind_desktop::parse(&args)?;
    match mode {
        Mode::Help => println!("{USAGE}"),
        Mode::Version => {
            println!(
                "{}",
                galion_common::build_info::version_line(
                    "openmind-desktop",
                    env!("CARGO_PKG_VERSION")
                )
            );
        }
        Mode::Page => print!("{}", openmind_desktop::document()),
        Mode::Listen(addr) => {
            let addr = addr.as_deref().unwrap_or(openmind_desktop::DEFAULT_BIND);
            openmind_desktop::listen(addr)?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    galion_common::build_info::exit_if_version("openmind-desktop", env!("CARGO_PKG_VERSION"));
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            let code = u8::try_from(err.exit_code()).unwrap_or(1);
            ExitCode::from(code)
        }
    }
}
