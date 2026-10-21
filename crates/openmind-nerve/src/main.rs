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
//!   openmind-nerve replay DIR [--md | --state | --plans | --verdict [--hold N]]
//!       run a read-only capture of a live host through the same pipeline;
//!       `--verdict` prints the replay-corpus summary (final + held verdict
//!       after N quiet hold cycles, default 20, and any flapping keys; exit 3
//!       on a flap)
//!   openmind-nerve plans STATE.json|-
//!       HADES plan templates (T4.2) for a saved `/api/v1/state` JSON (text only,
//!       nothing is run; exit 0, or 2 on unreadable input)
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
//! NERVE_SENSORS_OFF (cortex sensor names, comma-separated), NERVE_EDGE_HOSTS (1; 0 = no
//! per-hostname edge streams), NERVE_EDGE_HOSTS_MAX (32).
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

const USAGE: &str = "usage: openmind-nerve [serve [--listen 127.0.0.1:PORT] [--log PATH] [--max-bytes N] [--keep K] | once [--md] | replay DIR [--md|--state|--plans|--verdict [--hold N]] | plans STATE.json|- | --version]";

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
    if cmd == "plans" {
        return plans_cmd(args.get(1).map(String::as_str));
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
        "replay" => {
            let Some(dir) = args.get(1).filter(|a| !a.starts_with("--")) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            if args.iter().any(|a| a == "--verdict") {
                let hold = match opt(&args, "--hold").map(|h| h.parse::<usize>()) {
                    None => openmind_nerve::corpus::HOLD_CYCLES,
                    Some(Ok(h)) if h <= 10_000 => h,
                    _ => {
                        eprintln!("openmind-nerve replay: --hold N (0..=10000)");
                        return ExitCode::from(2);
                    }
                };
                return match openmind_nerve::corpus::run(
                    &PathBuf::from(dir),
                    cfg,
                    &|k| std::env::var(k).ok(),
                    hold,
                ) {
                    Ok(v) => {
                        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                        if v["flaps"].as_array().is_some_and(|f| !f.is_empty()) {
                            eprintln!("openmind-nerve replay: verdict flaps: {}", v["flaps"]);
                            ExitCode::from(3)
                        } else {
                            ExitCode::SUCCESS
                        }
                    }
                    Err(err) => {
                        eprintln!("openmind-nerve replay: {err}");
                        ExitCode::from(2)
                    }
                };
            }
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

/// `plans FILE|-`: read a state JSON, print its plan templates. Reads one
/// file (or stdin); runs nothing.
fn plans_cmd(path: Option<&str>) -> ExitCode {
    use std::io::Read;
    let mut s = String::new();
    let read = match path {
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Some("-") => std::io::stdin().read_to_string(&mut s).map(|_| ()),
        Some(p) => std::fs::read_to_string(p).map(|t| s = t),
    };
    let state: serde_json::Value = match read
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("openmind-nerve plans: {e}");
            return ExitCode::from(2);
        }
    };
    let keep =
        openmind_nerve::protected_hosts::ProtectedHosts::from_env(&|k| std::env::var(k).ok());
    let p = openmind_nerve::plans::from_state_with(&state, &keep).to_json();
    println!("{}", serde_json::to_string_pretty(&p).unwrap_or_default());
    ExitCode::SUCCESS
}

fn print_report(e: &Engine, args: &[String]) -> ExitCode {
    let sh = e.shared.lock().unwrap_or_else(|x| x.into_inner());
    if args.iter().any(|a| a == "--plans") {
        println!(
            "{}",
            serde_json::to_string_pretty(&sh.state["cortex_plans"]).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }
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
