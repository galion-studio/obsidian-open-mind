// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
use galion_box::policy::{parse_args, plan_steps, Cmd, USAGE};
use galion_box::{inside, launch, probe};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match parse_args(&args) {
        Err(e) => {
            eprintln!("galion-box: {e}");
            2
        }
        Ok(Cmd::Help) => {
            print!("{USAGE}");
            0
        }
        Ok(Cmd::Version) => {
            println!(
                "galion-box {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH
            );
            0
        }
        Ok(Cmd::Probe) => {
            print!("{}", probe::Probe::collect().render());
            0
        }
        Ok(Cmd::Plan(p)) => {
            // SAFETY: trivial getters.
            let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
            for (i, s) in plan_steps(&p, uid, gid).iter().enumerate() {
                println!("{:2}. {s}", i + 1);
            }
            0
        }
        Ok(Cmd::Run(p)) => match launch::run(&p) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("galion-box: {e}");
                launch::EXIT_SETUP_FAILED
            }
        },
        Ok(Cmd::Inside(a)) => inside::dispatch(&a),
    };
    std::process::exit(code);
}
