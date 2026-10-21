// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! galion-wallstreet CLI. PAPER ONLY: no live broker, no real orders, no money.
//!
//!   galion-wallstreet serve [--bind 127.0.0.1:19520]   (or WALLSTREET_BIND)
//!   galion-wallstreet ticket <file.json> [--md]
//!   galion-wallstreet backtest
//!   galion-wallstreet score [--md]
//!   galion-wallstreet screen "<text>"

use galion_wallstreet::{backtest, compliance, http, report, score, ticket};
use std::process::ExitCode;

const USAGE: &str = "usage: galion-wallstreet serve [--bind ADDR] | ticket FILE [--md] | backtest | score [--md] | screen TEXT";

fn score_md(s: &score::Scoreboard) -> String {
    let mut o = format!(
        "B2 wallstreet: {}% ({}/{} checks), mode {}, live broker {}, generated {}\n",
        s.percent, s.passed, s.total, s.mode, s.live_broker, s.generated_at
    );
    for a in &s.axes {
        o.push_str(&format!(
            "  {:<22} {:>3}% ({}/{})\n",
            a.axis, a.percent, a.passed, a.total
        ));
        for c in a.checks.iter().filter(|c| !c.pass) {
            o.push_str(&format!("    FAIL {} {}\n", c.name, c.detail));
        }
    }
    o
}

fn main() -> ExitCode {
    galion_common::flags::early(
        "galion-wallstreet",
        env!("CARGO_PKG_VERSION"),
        USAGE,
        || {
            let mut r = String::new();
            let bind =
                std::env::var("WALLSTREET_BIND").unwrap_or_else(|_| "127.0.0.1:19520".into());
            galion_common::flags::bind_addr(&mut r, "serve_bind", &bind)?;
            Ok(r)
        },
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    match args.first().map(String::as_str) {
        Some("serve") => {
            let bind = args
                .iter()
                .position(|a| a == "--bind")
                .and_then(|i| args.get(i + 1).cloned())
                .or_else(|| std::env::var("WALLSTREET_BIND").ok())
                .unwrap_or_else(|| "127.0.0.1:19520".into());
            match http::serve(&bind) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("galion-wallstreet: cannot serve on {bind}: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Some("ticket") => {
            let Some(path) = args.get(1) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let txt = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("galion-wallstreet: {path}: {e}");
                    return ExitCode::from(2);
                }
            };
            match ticket::parse(&txt) {
                Ok(t) => {
                    let r = ticket::run_fresh(&t);
                    if flag("--md") {
                        print!("{}", report::markdown(&r.report));
                        if let Some(v) = &r.refusal {
                            println!(
                                "\nTicket refused: {}",
                                serde_json::to_string(v).unwrap_or_default()
                            );
                        }
                    } else {
                        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
                    }
                    if r.status == "refused" {
                        ExitCode::from(3)
                    } else {
                        ExitCode::SUCCESS
                    }
                }
                Err(e) => {
                    eprintln!("galion-wallstreet: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Some("backtest") => {
            let r = backtest::reference();
            println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            ExitCode::SUCCESS
        }
        Some("score") => {
            let s = score::run();
            if flag("--md") {
                print!("{}", score_md(&s));
            } else {
                println!("{}", serde_json::to_string_pretty(&s).unwrap_or_default());
            }
            if s.percent == 100 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Some("screen") => {
            let text = args[1..].join(" ");
            let v = compliance::screen(&text);
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            if v.is_refusal() {
                ExitCode::from(3)
            } else {
                ExitCode::SUCCESS
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
