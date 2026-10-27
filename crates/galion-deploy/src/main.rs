// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-deploy binary: see the crate docs (src/lib.rs) and README.md.

fn main() {
    galion_deploy::sys::set_umask(0o077);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("backup") => {
            nerve_client::event("galion-deploy", "started", "backup");
            galion_deploy::backup::main_backup(&args[1..])
        }
        Some("backup-verify") => {
            nerve_client::event("galion-deploy", "started", "backup-verify");
            galion_deploy::backup::main_verify(&args[1..])
        }
        _ => {
            // NERVE push (no-op unless NERVE_URL is set): started + heartbeat for the
            // long-running deploy path (backup -> install -> restart -> health).
            nerve_client::event(
                "galion-deploy",
                "started",
                &format!("deploy args={}", args.len()),
            );
            nerve_client::start_heartbeat("galion-deploy", 60);
            galion_deploy::sys::install_handlers();
            galion_deploy::deploy::main(&args)
        }
    };
    std::process::exit(code);
}
