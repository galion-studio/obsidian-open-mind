// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-deploy binary: see the crate docs (src/lib.rs) and README.md.

fn main() {
    galion_deploy::sys::set_umask(0o077);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("backup") => galion_deploy::backup::main_backup(&args[1..]),
        Some("backup-verify") => galion_deploy::backup::main_verify(&args[1..]),
        Some(c @ ("dist-install" | "dist-check" | "dist-rollback")) => {
            galion_deploy::dist::main(c, &args[1..])
        }
        _ => {
            galion_deploy::sys::install_handlers();
            galion_deploy::deploy::main(&args)
        }
    };
    std::process::exit(code);
}
