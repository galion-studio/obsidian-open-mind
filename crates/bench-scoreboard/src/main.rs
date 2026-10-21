// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `bench-scoreboard [--score B<n>=<json>@<sha>]... [--out FILE]`
//! Prints (or writes) SCOREBOARD.md. Exit 0 ok, 1 bad score file, 2 usage.

use bench_scoreboard::{parse_score, render, Entry, BENCHES};
use std::process::ExitCode;

fn main() -> ExitCode {
    // `--version` / `-V` first: no config, bind or I/O (release rehearsal, slot 216).
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V")) {
        println!("bench-scoreboard {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    let mut entries: [Option<Entry>; 3] = [None, None, None];
    let mut out: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--score" => {
                let Some(spec) = args.next() else {
                    eprintln!("--score needs B<n>=<json>@<sha>");
                    return ExitCode::from(2);
                };
                let Some((key, rest)) = spec.split_once('=') else {
                    eprintln!("bad --score {spec}");
                    return ExitCode::from(2);
                };
                let Some((path, sha)) = rest.rsplit_once('@') else {
                    eprintln!("bad --score {spec}: missing @<sha>");
                    return ExitCode::from(2);
                };
                let Some(i) = BENCHES.iter().position(|(k, _, _)| *k == key) else {
                    eprintln!("unknown benchmark {key} (B1, B2, B3)");
                    return ExitCode::from(2);
                };
                let text = match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("{path}: {e}");
                        return ExitCode::from(1);
                    }
                };
                match parse_score(&text) {
                    Ok(card) => {
                        entries[i] = Some(Entry {
                            card,
                            sha: sha.to_string(),
                            json_path: path.to_string(),
                            stamp: galion_bench::now_zagreb(),
                        })
                    }
                    Err(e) => {
                        eprintln!("{path}: {e}");
                        return ExitCode::from(1);
                    }
                }
            }
            "--out" => out = args.next(),
            _ => {
                eprintln!("usage: bench-scoreboard [--score B<n>=<json>@<sha>]... [--out FILE]");
                return ExitCode::from(2);
            }
        }
    }
    let md = render(&entries, &galion_bench::now_zagreb());
    match out {
        Some(f) => {
            if let Err(e) = std::fs::write(&f, md) {
                eprintln!("{f}: {e}");
                return ExitCode::from(1);
            }
        }
        None => print!("{md}"),
    }
    ExitCode::SUCCESS
}
