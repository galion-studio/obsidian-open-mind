// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! A valid ELF that is a broken service: `--version` answers, but serving exits 1
//! at once (the classic "new binary crashes on start"). galion-deploy must roll back.

use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("sim-broken 0.1.0");
        return ExitCode::SUCCESS;
    }
    eprintln!("sim-broken: refusing to serve (deliberately broken build)");
    ExitCode::from(1)
}
