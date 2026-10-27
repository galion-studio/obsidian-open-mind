// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! pw-check [/path ...]: headless check of console pages (port of pw.py).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match pw_check::parse_args(&args) {
        Ok(o) => o,
        Err(e) if e == pw_check::USAGE => {
            println!("{e}");
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    match pw_check::run(&o) {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("pw-check: {e}");
            std::process::exit(1);
        }
    }
}
