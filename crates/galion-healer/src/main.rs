// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-healer
//!   run [--config F] [--once] [--dry-run]   desk healer loop (30 s)
//!   plan "<error text>"                      heal-dry.py: print the dry-run plan
//!   repair-json --root R --file F --error E [--apply] [--config F]
//!                                             heal-apply.py, fixture root only

use galion_healer::{actions, config, probe_one, run_cycle, Deps};
use openmind_survival::SystemRunner;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn load_cfg(path: Option<String>) -> Result<config::File, (i32, String)> {
    let p = path
        .or_else(|| std::env::var("HEALER_CONFIG").ok())
        .ok_or((2, "no config (--config or HEALER_CONFIG)".to_string()))?;
    let t = std::fs::read_to_string(&p).map_err(|e| (2, format!("{p}: {e}")))?;
    config::parse(&t).map_err(|e| (2, format!("{p}: {e}")))
}

fn real_main() -> Result<i32, (i32, String)> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str).unwrap_or("run");
    let implicit = first.starts_with("--") && first != "--version";
    let sub = if implicit { "run" } else { first };
    let mut cfg = None;
    let (mut once, mut dry, mut apply) = (false, false, false);
    let (mut root, mut file, mut error) = (None, None, None);
    let mut rest = Vec::new();
    let mut it = args
        .iter()
        .skip(if args.is_empty() || implicit { 0 } else { 1 });
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or((2, format!("{a} needs a value")));
        match a.as_str() {
            "--config" => cfg = Some(val()?),
            "--once" => once = true,
            "--dry-run" => dry = true,
            "--apply" => apply = true,
            "--root" => root = Some(val()?),
            "--file" => file = Some(val()?),
            "--error" => error = Some(val()?),
            o if o.starts_with("--") => return Err((2, format!("unknown argument {o}"))),
            o => rest.push(o.to_string()),
        }
    }
    match sub {
        "--version" => {
            println!("galion-healer {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        "plan" => {
            println!(
                "{}",
                openmind_survival::patterns::dry_plan(&rest.join(" ")).to_json()
            );
            Ok(0)
        }
        "repair-json" => {
            let fixture = match cfg {
                Some(_) => load_cfg(cfg)?.healer.fixture_root,
                None => config::Settings::default().fixture_root,
            };
            let (root, file, error) = match (root, file, error) {
                (Some(r), Some(f), Some(e)) => (r, f, e),
                _ => return Err((2, "repair-json needs --root, --file and --error".into())),
            };
            let (v, code) =
                actions::json_brace_repair(&fixture, Path::new(&root), &file, &error, apply);
            println!("{v}");
            eprintln!(
                "{}",
                actions::log_row("json-brace-repair", &file, apply, &v.to_string())
            );
            Ok(code)
        }
        "run" => {
            let mut f = load_cfg(cfg)?;
            if dry {
                f.healer.apply = false;
            }
            let mut prober = probe_one;
            loop {
                let t0 = Instant::now();
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                let mut deps = Deps {
                    runner: &SystemRunner,
                    proc_root: Path::new("/proc"),
                    prober: &mut prober,
                    now_unix: now,
                    stamp: zagreb_time::now_rfc3339(),
                };
                match run_cycle(&f, &mut deps) {
                    Ok(p) => {
                        let down = p["down"].as_array().map_or(0, Vec::len);
                        println!(
                            "{} {} api {} mode {}",
                            p["at"].as_str().unwrap_or(""),
                            if down > 0 { "down" } else { "clear" },
                            p["api_calls"],
                            p["mode"].as_str().unwrap_or("")
                        );
                    }
                    Err(e) => println!("heal error {}", openmind_survival::truncate(&e, 200)),
                }
                if once {
                    return Ok(0);
                }
                std::thread::sleep(
                    Duration::from_secs(f.healer.interval_s).saturating_sub(t0.elapsed()),
                );
            }
        }
        o => Err((2, format!("unknown subcommand {o}"))),
    }
}

fn main() {
    match real_main() {
        Ok(code) => std::process::exit(code),
        Err((code, msg)) => {
            eprintln!("galion-healer: {msg}");
            std::process::exit(code);
        }
    }
}
