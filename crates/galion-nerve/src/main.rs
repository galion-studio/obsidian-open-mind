// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-nerve binary.
//!
//!   galion-nerve [serve]          loop + loopback API (default 127.0.0.1:8670)
//!   galion-nerve once [--md]      one observe-only cycle; prints the self-eval report
//!   galion-nerve --version
//!
//! Env: NERVE_ADDR, NERVE_INTERVAL (30), NERVE_DATA_DIR (/var/lib/galion-nerve),
//! NERVE_DRY_RUN (1), NERVE_FAIL_CYCLES (3), NERVE_REPORT_EVERY (20),
//! NERVE_GALION_HEALTH_PORT (8660, 0 = off), NERVE_ROLLBACK (1), NERVE_GAIA_BIN,
//! NERVE_VOICE_UNIT, NERVE_VOICE_PORT, NERVE_TELEGRAM_ENV, NERVE_EXT_ALERTS_PER_HOUR (4).
//! Runtime dry-run switch: `echo live > $NERVE_DATA_DIR/mode` or `echo dry-run > ...`.

use galion_nerve::config::Config;
use galion_nerve::engine::Engine;
use std::net::TcpListener;
use std::process::ExitCode;
use std::time::{Duration, Instant};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    if cmd == "--version" || cmd == "-V" {
        println!("galion-nerve {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if cmd == "-h" || cmd == "--help" {
        eprintln!("usage: galion-nerve [serve | once [--md] | --version]");
        return ExitCode::SUCCESS;
    }
    let cfg = match Config::from_env(&|k| std::env::var(k).ok()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("<3>galion-nerve: config: {e}");
            return ExitCode::from(2);
        }
    };
    match cmd {
        "once" => {
            let mut e = Engine::new(cfg);
            e.reflexes_enabled = false;
            e.write_files = false;
            e.alerts.path = None;
            e.alerts.to_journal = false;
            e.cycle();
            let sh = e.shared.lock().unwrap_or_else(|x| x.into_inner());
            if args.iter().any(|a| a == "--md") {
                print!("{}", sh.report_md);
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
        "serve" => serve(cfg),
        other => {
            eprintln!("galion-nerve: unknown command {other:?}");
            ExitCode::from(2)
        }
    }
}

fn serve(cfg: Config) -> ExitCode {
    let listener = match TcpListener::bind(cfg.addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("<3>galion-nerve: bind {}: {e}", cfg.addr);
            return ExitCode::from(1);
        }
    };
    let interval = Duration::from_secs(cfg.interval_s);
    let mut e = Engine::new(cfg);
    eprintln!(
        "<6>galion-nerve {} listening on {} interval={}s mode={} data={}",
        env!("CARGO_PKG_VERSION"),
        e.cfg.addr,
        e.cfg.interval_s,
        e.mode(),
        e.cfg.data_dir.display()
    );
    let sh = e.shared.clone();
    if std::thread::Builder::new()
        .name("nerve-http".into())
        .spawn(move || galion_nerve::server::serve(listener, sh))
        .is_err()
    {
        eprintln!("<3>galion-nerve: cannot start http thread");
        return ExitCode::from(1);
    }
    let mut last_mode = e.mode();
    loop {
        let t = Instant::now();
        e.cycle();
        let m = e.mode();
        if m != last_mode {
            eprintln!("<5>galion-nerve reflex mode {last_mode} -> {m}");
            last_mode = m;
        }
        let st = e
            .shared
            .lock()
            .map(|s| {
                s.state["system"]["status"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        if e.cycle % 10 == 1 {
            eprintln!(
                "<6>galion-nerve cycle {} system={st} cycle_ms={}",
                e.cycle,
                t.elapsed().as_millis()
            );
        }
        std::thread::sleep(interval.saturating_sub(t.elapsed()));
    }
}
