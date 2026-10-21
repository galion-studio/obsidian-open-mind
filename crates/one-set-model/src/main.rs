// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Print the modes JSON object `set-model.py` would write.
//! Reads the given file. Writes nothing.

use one_set_model::{apply_file, render};
use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    galion_common::flags::early("one-set-model", env!("CARGO_PKG_VERSION"), "usage: one-set-model <plan|action> <vendor/model> <modes-file>\nprints the JSON object that would be written; does not write the file\n", || Ok("config none (arguments only; never writes)".into()));
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("usage: one-set-model <plan|action> <vendor/model> <modes-file>");
        eprintln!("prints the JSON object that would be written; does not write the file");
        return ExitCode::from(2);
    }
    let doc = match apply_file(Path::new(&args[2]), &args[0], &args[1]) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("one-set-model: {err}");
            return ExitCode::FAILURE;
        }
    };
    let text = match render(&doc) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("one-set-model: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut stdout = io::stdout().lock();
    if stdout.write_all(text.as_bytes()).is_err() {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
