// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `galion-vending` CLI.
//!
//! ```text
//! galion-vending run    [--seed N] [--days N] [--level standard|hard] [--policy P] [--log PATH] [--stop-after DAY]
//! galion-vending resume --log PATH          # finish a journaled run with the scripted operator
//! galion-vending replay --log PATH          # verify a journal, print its score
//! galion-vending serve  [--port P] [--bind ADDR] [--seed N] [--days N] [--level L] [--log PATH]
//! galion-vending tools                      # tool list (JSON)
//! galion-vending schedule [--seed N] [--days N] [--level L]
//! galion-vending bench  [--seed N] [--days N] [--level L] [--policy scripted|standard]
//! ```
//! Output is JSON on stdout. Exit 0 on success, 2 on usage errors, 1 otherwise.

use galion_vending::operator::StandardPolicyOperator;
use galion_vending::sim::Operator;
use galion_vending::world::{Level, Schedule, BENCH_SEED, DEFAULT_DAYS};
use galion_vending::{bench_level, bench_standard_policy, http, journal, ScriptedOperator, Sim};
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    seed: u64,
    days: u32,
    log: Option<PathBuf>,
    stop_after: Option<u32>,
    port: u16,
    bind: String,
    level: Level,
    /// `scripted` (adapts to the level) or `standard` (published standard
    /// policy, hard rules off).
    standard_policy: bool,
}

fn parse(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args {
        seed: BENCH_SEED,
        days: DEFAULT_DAYS,
        log: None,
        stop_after: None,
        port: 19530,
        bind: "127.0.0.1".into(),
        level: Level::Standard,
        standard_policy: false,
    };
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--seed" => a.seed = v()?.parse().map_err(|_| "bad --seed")?,
            "--days" => a.days = v()?.parse().map_err(|_| "bad --days")?,
            "--log" => a.log = Some(PathBuf::from(v()?)),
            "--stop-after" => a.stop_after = Some(v()?.parse().map_err(|_| "bad --stop-after")?),
            "--port" => a.port = v()?.parse().map_err(|_| "bad --port")?,
            "--bind" => a.bind = v()?,
            "--policy" => {
                a.standard_policy = match v()?.as_str() {
                    "scripted" => false,
                    "standard" => true,
                    s => return Err(format!("bad --policy '{s}' (scripted|standard)")),
                }
            }
            "--level" => {
                let s = v()?;
                a.level = Level::parse(&s).ok_or(format!("bad --level '{s}' (standard|hard)"))?
            }
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    Ok(a)
}

fn print(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1);
    let cmd = argv.next().unwrap_or_default();
    let a = match parse(argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("galion-vending: {e}");
            return ExitCode::from(2);
        }
    };
    let r: Result<(), String> = (|| match cmd.as_str() {
        "run" => {
            let mut sim = match &a.log {
                Some(p) => {
                    journal::create_level(p, a.seed, a.days, a.level).map_err(|e| e.to_string())?
                }
                None => Sim::with_level(a.seed, a.days, a.level),
            };
            let mut op: Box<dyn Operator> = if a.standard_policy {
                Box::new(StandardPolicyOperator)
            } else {
                Box::new(ScriptedOperator)
            };
            sim.run_until(op.as_mut(), a.stop_after.unwrap_or(u32::MAX));
            print(&sim.score());
            Ok(())
        }
        "resume" => {
            let p = a.log.as_ref().ok_or("--log required")?;
            let mut sim = journal::resume(p).map_err(|e| e.to_string())?;
            print(&sim.run(&mut ScriptedOperator));
            Ok(())
        }
        "replay" => {
            let p = a.log.as_ref().ok_or("--log required")?;
            let (sim, _) = journal::replay(p).map_err(|e| e.to_string())?;
            print(&sim.score());
            Ok(())
        }
        "serve" => {
            let l = std::net::TcpListener::bind((a.bind.as_str(), a.port))
                .map_err(|e| e.to_string())?;
            println!(
                "listening http://{}:{} seed={} days={}{}",
                a.bind,
                a.port,
                a.seed,
                a.days,
                if a.level.is_standard() {
                    ""
                } else {
                    " level=hard"
                }
            );
            http::Server::with_level(a.seed, a.days, a.level, a.log.clone())
                .map_err(|e| e.to_string())?
                .serve(l)
                .map_err(|e| e.to_string())
        }
        "tools" => {
            print(&galion_vending::tools::tool_specs());
            Ok(())
        }
        "schedule" => {
            print(&Schedule::with_level(a.seed, a.days, a.level));
            Ok(())
        }
        "bench" => {
            if a.standard_policy {
                print(&bench_standard_policy(a.seed, a.days, a.level));
            } else {
                print(&bench_level(a.seed, a.days, a.level));
            }
            Ok(())
        }
        _ => Err(format!(
            "unknown command '{cmd}' (run|resume|replay|serve|tools|schedule|bench)"
        )),
    })();
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("galion-vending: {e}");
            ExitCode::FAILURE
        }
    }
}
