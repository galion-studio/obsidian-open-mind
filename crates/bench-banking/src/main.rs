// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `bench-banking mock --port N`             serve the local mock bank (loopback only)
//! `bench-banking axis <id> --mock URL`      run one axis, print JSON, exit 1 if < 100
//! `bench-banking score --mock URL --out DIR [--sha SHA]`
//!                                           run all ten axes, write banking.md + banking.score.json

use bench_banking::{axes, mock::Mock};
use std::net::TcpListener;
use std::process::ExitCode;

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

const USAGE: &str = "usage: bench-banking mock --port N | axis <id> --mock URL | score --mock URL --out DIR [--sha SHA]\n";

fn usage() -> ExitCode {
    eprint!("{USAGE}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    galion_common::flags::early("bench-banking", env!("CARGO_PKG_VERSION"), USAGE, || {
        Ok("config none (arguments only; mock binds only with `mock --port`)".into())
    });
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("mock") => {
            let Some(port) = flag(&args, "--port").and_then(|p| p.parse::<u16>().ok()) else {
                return usage();
            };
            let l = match TcpListener::bind(("127.0.0.1", port)) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("bench-banking mock: bind 127.0.0.1:{port}: {e}");
                    return ExitCode::from(1);
                }
            };
            println!("bench-banking mock listening on http://127.0.0.1:{port} (loopback only, Revolut-shaped fixtures)");
            Mock::new().serve(l);
            ExitCode::SUCCESS
        }
        Some("axis") => {
            let (Some(id), Some(url)) = (args.get(1), flag(&args, "--mock")) else {
                return usage();
            };
            let Some(a) = axes::run(id, &url) else {
                eprintln!("unknown axis {id}");
                return usage();
            };
            let v = axes::score_json(std::slice::from_ref(&a), &zagreb_time::now_rfc3339(), "-");
            println!("{}", v["axes"][0]);
            if a.score() >= 100.0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Some("score") => {
            let (Some(url), Some(out)) = (flag(&args, "--mock"), flag(&args, "--out")) else {
                return usage();
            };
            let sha = flag(&args, "--sha").unwrap_or_else(|| "unknown".into());
            let at = zagreb_time::now_rfc3339();
            let all = axes::run_all(&url);
            let dir = std::path::Path::new(&out);
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("create {out}: {e}");
                return ExitCode::from(1);
            }
            let json = axes::score_json(&all, &at, &sha);
            let pretty = serde_json::to_string_pretty(&json).unwrap_or_default() + "\n";
            let wrote = std::fs::write(dir.join("banking.score.json"), pretty).and_then(|_| {
                std::fs::write(dir.join("banking.md"), axes::score_md(&all, &at, &sha))
            });
            if let Err(e) = wrote {
                eprintln!("write scoreboard: {e}");
                return ExitCode::from(1);
            }
            println!(
                "B1 banking: {}% ({} axes) at {at}",
                axes::pct(&all),
                all.len()
            );
            for a in &all {
                println!(
                    "  {:<18} {:>5}%  {}/{}",
                    a.id,
                    a.score(),
                    a.passed(),
                    a.checks.len()
                );
                for c in a.failing() {
                    println!("      FAIL {} :: {}", c.name, c.detail);
                }
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
