// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `bench-scoreboard [--score <KEY>=<json>@<sha>]... [--notes FILE] [--out FILE]`
//! KEY: B1, B2, B3 (standard) or B1-ADV, B2-ADV, B3-HARD, B3-HARD-H (adversarial / hard).
//! Prints (or writes) SCOREBOARD.md. Exit 0 ok, 1 bad score file, 2 usage.

use bench_scoreboard::{parse_score, render, render_adv, Entry, ADV_SETS, BENCHES};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut entries: [Option<Entry>; 3] = [None, None, None];
    let mut adv: Vec<Option<Entry>> = vec![None; ADV_SETS.len()];
    let mut notes: Option<String> = None;
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
                let slot = if let Some(i) = BENCHES.iter().position(|(k, _, _)| *k == key) {
                    &mut entries[i]
                } else if let Some(i) = ADV_SETS.iter().position(|s| s.key == key) {
                    &mut adv[i]
                } else {
                    eprintln!(
                        "unknown benchmark {key} (B1, B2, B3, B1-ADV, B2-ADV, B3-HARD, B3-HARD-H)"
                    );
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
                        *slot = Some(Entry {
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
            "--notes" => notes = args.next(),
            _ => {
                eprintln!("usage: bench-scoreboard [--score <KEY>=<json>@<sha>]... [--notes FILE] [--out FILE]");
                return ExitCode::from(2);
            }
        }
    }
    let now = galion_bench::now_zagreb();
    let mut md = render(&entries, &now);
    if adv.iter().any(Option::is_some) {
        md.push_str(&render_adv(&adv, &now));
    }
    if let Some(n) = notes {
        match std::fs::read_to_string(&n) {
            Ok(t) => {
                md.push('\n');
                md.push_str(&t);
            }
            Err(e) => {
                eprintln!("{n}: {e}");
                return ExitCode::from(1);
            }
        }
    }
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
