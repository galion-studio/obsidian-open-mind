// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-deploy binary: see the crate docs (src/lib.rs) and README.md.

fn main() {
    galion_deploy::sys::set_umask(0o077);
    galion_deploy::sys::install_handlers();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = galion_deploy::deploy::main(&args);
    std::process::exit(code);
}
