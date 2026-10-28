// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `cargo xtask <command>` (alias in .cargo/config.toml). See lib.rs.

use std::process::{Command, ExitCode};
use xtask::{cargo_args, child_env, facts_line, host_root, parse_test, plan, probe};

const USAGE: &str = "usage: cargo xtask test [--sandbox | --sandbox=auto|require | --no-sandbox] [--dry-run] [-- <cargo test args>]\n       cargo xtask probe\nenv: GALION_SANDBOX_TESTS=auto|1|require|0, GALION_SANDBOX_VPS_ONLY=1, HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (root, simulated) = host_root(std::env::var("GALION_XTASK_HOST_ROOT").ok());
    let env_vps_only = std::env::var("GALION_SANDBOX_VPS_ONLY").as_deref() == Ok("1");
    let allow_no_ll = std::env::var("HERMES_RS_CODING_ALLOW_NO_LANDLOCK").as_deref() == Ok("1");
    let facts = probe(&root, env_vps_only);
    match args.first().map(String::as_str) {
        Some("probe") => {
            println!("{}", facts_line(&facts));
            for k in xtask::SANDBOX_CRATES {
                let m = xtask::missing(k, &facts, allow_no_ll);
                println!(
                    "xtask: {k} {}",
                    if m.is_empty() {
                        "supported".to_string()
                    } else {
                        format!("lacks {}", m.join("+"))
                    }
                );
            }
            ExitCode::SUCCESS
        }
        Some("test") => {
            let env_mode = std::env::var("GALION_SANDBOX_TESTS").ok();
            let cli = match parse_test(&args[1..], env_mode.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("xtask: {e}\n{USAGE}");
                    return ExitCode::from(2);
                }
            };
            println!("{}", facts_line(&facts));
            let p = match plan(&facts, cli.mode, allow_no_ll) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("xtask: {e}");
                    return ExitCode::from(3);
                }
            };
            println!("xtask: sandbox mode {}", cli.mode.name());
            for k in &p.include {
                println!("xtask: include {k}");
            }
            for (k, why) in &p.exclude {
                println!("xtask: exclude {k} ({why})");
            }
            let a = cargo_args(&p, &cli.extra);
            println!("xtask: cargo {}", a.join(" "));
            let (ek, ev) = child_env(cli.mode);
            println!("xtask: test env {ek}={ev}");
            if cli.dry_run || simulated {
                if simulated {
                    println!("xtask: GALION_XTASK_HOST_ROOT set: simulation only, cargo not run");
                }
                return ExitCode::SUCCESS;
            }
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            // GALION_SANDBOX_TESTS tells the test processes which mode chose them
            // (F-335-1): hermes-tools sandboxed.rs fails, not skips, under require.
            match Command::new(cargo).args(&a).env(ek, ev).status() {
                Ok(s) if s.success() => ExitCode::SUCCESS,
                Ok(s) => {
                    eprintln!("xtask: cargo test failed ({s})");
                    ExitCode::from(1)
                }
                Err(e) => {
                    eprintln!("xtask: cannot start cargo: {e}");
                    ExitCode::from(1)
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
