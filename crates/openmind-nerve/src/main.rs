// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! openmind-nerve binary (deployed as galion-nerve.service).
//!
//!   openmind-nerve [serve] [--listen 127.0.0.1:PORT] [--log PATH] [--max-bytes N] [--keep K]
//!       loop + loopback API (default 127.0.0.1:8670 / NERVE_ADDR). `--log`
//!       (or NERVE_EVENT_LOG) enables the JSONL event log behind POST /event
//!       and GET /events (swarm/gaia-live's `openmind-nerve serve` API).
//!   openmind-nerve once [--md]        one observe-only cycle; prints the self-eval
//!   openmind-nerve replay DIR [--md | --state]
//!       run a read-only capture of a live host through the same pipeline
//!   openmind-nerve replay DIR --reflex [--stretch N] [--worst-case] [--log]
//!       the same replay with reflexes on, forced dry-run (nothing executed);
//!       prints the T3.2 verdict (exit 1 on any cloudflared restart or rollback)
//!   openmind-nerve reflex-audit
//!       allowlist / never-touch audit of this config's reflex targets (exit 1 on a violation)
//!   openmind-nerve --version
//!
//! Env: NERVE_ADDR, NERVE_INTERVAL (30), NERVE_DATA_DIR (/var/lib/galion-nerve),
//! NERVE_DRY_RUN (1), NERVE_REFLEX (1), NERVE_FAIL_CYCLES (3), NERVE_REPORT_EVERY (20),
//! NERVE_GALION_HEALTH_PORT (8660, 0 = off), NERVE_ROLLBACK (1), NERVE_GAIA_BIN,
//! NERVE_VOICE_UNIT, NERVE_VOICE_PORT, NERVE_TELEGRAM_ENV, NERVE_EXT_ALERTS_PER_HOUR (4),
//! NERVE_COMPONENTS (JSON table), NERVE_SYSTEMD (1), NERVE_JOURNAL (1),
//! NERVE_ACCESS_LOGS (name=path[@component],...), NERVE_KNOWN_PORTS,
//! NERVE_PUBLIC_OK_PORTS (22), NERVE_LISTEN_SCOPE (lo-hi), NERVE_DDOS_MIN_RPS (10),
//! NERVE_DDOS_MIN_REQUESTS (300), NERVE_LOADING_GRACE (900), NERVE_EVENT_LOG,
//! NERVE_SENSORS_OFF (cortex sensor names, comma-separated).
//! Runtime dry-run switch: `echo live > $NERVE_DATA_DIR/mode` or `echo dry-run > ...`.

use openmind_nerve::config::Config;
use openmind_nerve::engine::Engine;
use openmind_nerve::server::EventLog;
use openmind_nerve::Writer;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const USAGE: &str = "usage: openmind-nerve [serve [--listen 127.0.0.1:PORT] [--log PATH] [--max-bytes N] [--keep K] | once [--md] | replay DIR [--md|--state|--reflex [--stretch N] [--worst-case] [--log]] | reflex-audit | --version]";

fn opt(args: &[String], k: &str) -> Option<String> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    if cmd == "--version" || cmd == "-V" {
        println!("openmind-nerve {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let listen = opt(&args, "--listen");
    let env = |k: &str| {
        if k == "NERVE_ADDR" {
            if let Some(l) = &listen {
                return Some(l.clone());
            }
        }
        std::env::var(k).ok()
    };
    let cfg = match Config::from_env(&env) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("<3>openmind-nerve: config: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match cmd {
        "once" => {
            let mut e = match Engine::new(cfg) {
                Ok(e) => e,
                Err(err) => {
                    eprintln!("<3>openmind-nerve: {err}");
                    return ExitCode::from(2);
                }
            };
            e.reflexes_enabled = false;
            e.write_files = false;
            e.alerts.path = None;
            e.alerts.to_journal = false;
            e.cycle();
            print_report(&e, &args)
        }
        "reflex-audit" => match openmind_nerve::config::specs_for(&cfg) {
            Ok(specs) => {
                let a = openmind_nerve::reflex_audit::allowlist_audit(&specs);
                println!("{}", serde_json::to_string_pretty(&a).unwrap_or_default());
                if a["pass"] == true {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(err) => {
                eprintln!("openmind-nerve reflex-audit: {err}");
                ExitCode::from(2)
            }
        },
        "replay" if args.iter().any(|a| a == "--reflex") => {
            let Some(dir) = args.get(1).filter(|a| !a.starts_with("--")) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let opts = openmind_nerve::reflex_audit::DryOpts {
                stretch: opt(&args, "--stretch")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0)
                    .min(10_000),
                worst_case: args.iter().any(|a| a == "--worst-case"),
            };
            match openmind_nerve::reflex_audit::dry_replay(&PathBuf::from(dir), cfg, &opts) {
                Ok(r) => {
                    for n in &r.out.notes {
                        eprintln!("replay: {n}");
                    }
                    let mut v = r.verdict.clone();
                    if args.iter().any(|a| a == "--log") {
                        v["log"] = serde_json::Value::from(r.log.clone());
                    }
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                    if r.verdict["pass"] == true {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    }
                }
                Err(err) => {
                    eprintln!("openmind-nerve replay: {err}");
                    ExitCode::from(2)
                }
            }
        }
        "replay" => {
            let Some(dir) = args.get(1).filter(|a| !a.starts_with("--")) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            match openmind_nerve::replay::replay_dir(&PathBuf::from(dir), cfg) {
                Ok(out) => {
                    for n in &out.notes {
                        eprintln!("replay: {n}");
                    }
                    print_report(&out.engine, &args)
                }
                Err(err) => {
                    eprintln!("openmind-nerve replay: {err}");
                    ExitCode::from(2)
                }
            }
        }
        "serve" => serve(cfg, &args),
        other => {
            eprintln!("openmind-nerve: unknown command {other:?}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn print_report(e: &Engine, args: &[String]) -> ExitCode {
    let sh = e.shared.lock().unwrap_or_else(|x| x.into_inner());
    if args.iter().any(|a| a == "--md") {
        print!("{}", sh.report_md);
    } else if args.iter().any(|a| a == "--state") {
        println!(
            "{}",
            serde_json::to_string_pretty(&sh.state).unwrap_or_default()
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&sh.report).unwrap_or_default()
        );
    }
    if sh.report["pass"] == true {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn serve(cfg: Config, args: &[String]) -> ExitCode {
    let log = opt(args, "--log").or_else(|| std::env::var("NERVE_EVENT_LOG").ok());
    let ev = match log {
        Some(p) => {
            let max = opt(args, "--max-bytes")
                .and_then(|v| v.parse().ok())
                .unwrap_or(8 * 1024 * 1024);
            let keep = opt(args, "--keep")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3);
            match Writer::open(&p, max, keep) {
                Ok(w) => Some(Arc::new(Mutex::new(EventLog {
                    writer: w,
                    path: PathBuf::from(&p),
                    appended: 0,
                }))),
                Err(e) => {
                    eprintln!("<3>openmind-nerve: open {p}: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        None => None,
    };
    let listener = match TcpListener::bind(cfg.addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("<3>openmind-nerve: bind {}: {e}", cfg.addr);
            return ExitCode::from(1);
        }
    };
    let interval = Duration::from_secs(cfg.interval_s);
    let mut e = match Engine::new(cfg) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("<3>openmind-nerve: {err}");
            return ExitCode::from(2);
        }
    };
    eprintln!(
        "<6>openmind-nerve {} listening on {} interval={}s mode={} data={} event-log={} components={}",
        env!("CARGO_PKG_VERSION"),
        e.cfg.addr,
        e.cfg.interval_s,
        e.mode(),
        e.cfg.data_dir.display(),
        ev.as_ref().map(|x| x.lock().map(|l| l.path.display().to_string()).unwrap_or_default()).unwrap_or_else(|| "off".into()),
        e.comps.len()
    );
    let sh = e.shared.clone();
    if std::thread::Builder::new()
        .name("nerve-http".into())
        .spawn(move || openmind_nerve::server::serve(listener, sh, ev))
        .is_err()
    {
        eprintln!("<3>openmind-nerve: cannot start http thread");
        return ExitCode::from(1);
    }
    let mut last_mode = e.mode();
    let mut last_sys = String::new();
    loop {
        let t = Instant::now();
        e.cycle();
        let m = e.mode();
        if m != last_mode {
            eprintln!("<5>openmind-nerve reflex mode {last_mode} -> {m}");
            last_mode = m;
        }
        let sys = e.system_status().to_string();
        if e.cycle % 10 == 1 || sys != last_sys {
            eprintln!(
                "<6>openmind-nerve cycle {} system={sys} score={} incidents={} ddos={} cycle_ms={}",
                e.cycle,
                e.score.as_ref().map(|s| s.score).unwrap_or(0),
                e.incidents.open.len(),
                e.cortex.ddos_firing,
                t.elapsed().as_millis()
            );
            last_sys = sys;
        }
        std::thread::sleep(interval.saturating_sub(t.elapsed()));
    }
}
