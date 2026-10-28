// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `room` binary. See the library docs (`cargo doc -p room`) for the modes.

use room::{Config, Mode, RoomError, USAGE};
use std::process::ExitCode;

fn run() -> Result<(), RoomError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = room::parse(&args)?;
    let cfg = Config::from_env();
    match mode {
        Mode::Help => println!("{USAGE}"),
        Mode::Version => println!("room {}", env!("CARGO_PKG_VERSION")),
        Mode::Listen => room::listen(cfg)?,
        Mode::Page | Mode::Fixture(_) => {
            if let Some(json) = room::page(&mode, &cfg) {
                println!("{json}");
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    galion_common::build_info::exit_if_version("room", env!("CARGO_PKG_VERSION"));
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(1))
        }
    }
}
