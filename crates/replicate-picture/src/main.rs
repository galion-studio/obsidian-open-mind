// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! replicate-picture --prompt P --out FILE [--aspect 1:1]
//! Prints the model and the file path. The token stays in the key file; never printed.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match replicate_picture::parse_args(&args) {
        Ok(o) => o,
        Err(e) if e == replicate_picture::USAGE => {
            println!("{e}");
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    match replicate_picture::run(&o) {
        Ok((model, dest)) => {
            println!("{model}");
            println!("{}", dest.display());
        }
        Err(e) => {
            // SystemExit(msg): message on stderr, exit 1.
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
