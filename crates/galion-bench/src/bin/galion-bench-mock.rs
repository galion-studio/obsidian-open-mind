// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-bench-mock`: serve JSONL fixtures over local HTTP until killed.
//!
//! ```text
//! galion-bench-mock --fixtures a.jsonl [--fixtures b.jsonl] [--bind 127.0.0.1:19500] [--allow-non-loopback]
//! galion-bench-mock --check a.jsonl      # validate only, exit 0/1
//! ```
//! Prints one `listening http://ADDR fixtures=N at=<Zagreb>` line on stdout
//! once it accepts connections.

use galion_bench::{load_jsonl, FixtureSet, MockServer};
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "usage: galion-bench-mock --fixtures FILE.jsonl [--fixtures FILE2.jsonl] [--bind 127.0.0.1:19500] [--allow-non-loopback]\n       galion-bench-mock --check FILE.jsonl [FILE2.jsonl ...]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut bind = "127.0.0.1:19500".to_string();
    let mut files = Vec::new();
    let mut any = false;
    let mut check = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--bind" if i + 1 < args.len() => {
                bind = args[i + 1].clone();
                i += 1;
            }
            "--fixtures" if i + 1 < args.len() => {
                files.push(args[i + 1].clone());
                i += 1;
            }
            "--allow-non-loopback" => any = true,
            "--check" => check = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other if check && !other.starts_with('-') => files.push(other.to_string()),
            other => {
                eprintln!("galion-bench-mock: unknown argument `{other}`\n{USAGE}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    if files.is_empty() {
        eprintln!("galion-bench-mock: no fixture files\n{USAGE}");
        return ExitCode::from(2);
    }
    let mut set = FixtureSet::default();
    for f in &files {
        match load_jsonl(f).and_then(|s| set.extend(s)) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("galion-bench-mock: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let n = set.fixtures.len();
    if check {
        println!("ok fixtures={n} at={}", galion_bench::now_zagreb());
        return ExitCode::SUCCESS;
    }
    let started = if any {
        MockServer::start_any(bind.as_str(), set)
    } else {
        MockServer::start(bind.as_str(), set)
    };
    let server = match started {
        Ok(s) => s,
        Err(e) => {
            eprintln!("galion-bench-mock: cannot start on {bind}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "listening {} fixtures={n} at={}",
        server.base_url(),
        galion_bench::now_zagreb()
    );
    let _ = std::io::stdout().flush();
    server.wait();
    ExitCode::SUCCESS
}
