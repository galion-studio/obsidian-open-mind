// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Test double, dispatched on its argv[0] name (tests symlink it into place):
//!
//! * `systemctl`, `journalctl`: record every call in `$SHIM_CALLS` and fake one unit
//!   (used by tests/systemd_backend.rs). `SHIM_KNOWN=tuner` is the only unit
//!   `systemctl cat` knows; `SHIM_TARGET` is the ExecStart path `systemctl show`
//!   reports; `SHIM_STATE` present means active (restart creates it unless the
//!   target is missing or equals `SHIM_BAD`, stop removes it); `SHIM_RESTART_RC` is
//!   the exit code of every `systemctl restart` (default 0).
//!   `systemctl show -p ActiveState -p SubState -p NRestarts` (fail-fast) answers only when
//!   `SHIM_NRESTARTS` names a counter file: every read while the unit is not active
//!   counts one more automatic restart and reports one pending (a crash loop). Unset, it prints what an old
//!   systemd without NRestarts would: nothing usable.
//! * `ctl`: the same fake unit behind the direct backend's verbs
//!   (`ctl known|exec-path|restart|stop|is-active|reset-failed|journal UNIT [N]`).
//! * `data-backup`: stand-in for galion-backup.sh. Prints one line, touches
//!   `$SHIM_DATA_MARK` if set, sleeps `$SHIM_DATA_SLEEP` seconds (the real snapshot
//!   takes about 3 s; tests/real_rehearsal.rs uses 3).
//!
//! Not a product binary: it is only built as a cargo example.

use std::fs;
use std::io::Write;

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

fn record(line: &str) {
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(env("SHIM_CALLS"))
    {
        let _ = writeln!(f, "{line}");
    }
}

fn systemctl(verb: &str, unit_arg: Option<&str>) -> i32 {
    let unit = format!("{}.service", env("SHIM_KNOWN"));
    let state = env("SHIM_STATE");
    let known = unit_arg == Some(unit.as_str());
    match verb {
        "cat" => {
            if known {
                println!("# unit");
                0
            } else {
                1
            }
        }
        "show" => {
            let t = env("SHIM_TARGET");
            println!("{{ path={t} ; argv[]={t} ; ignore_errors=no ; start_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}");
            0
        }
        "restart" => {
            let rc: i32 = env("SHIM_RESTART_RC").parse().unwrap_or(0);
            if rc == 0 {
                let bad = fs::read(env("SHIM_BAD")).ok();
                let cur = fs::read(env("SHIM_TARGET")).ok();
                if cur.is_some() && cur != bad {
                    let _ = fs::write(&state, "");
                } else {
                    let _ = fs::remove_file(&state);
                }
            }
            rc
        }
        "stop" => {
            let _ = fs::remove_file(&state);
            0
        }
        "is-active" => {
            if std::path::Path::new(&state).exists() {
                0
            } else {
                3
            }
        }
        "reset-failed" => 0,
        _ => 64,
    }
}

fn unit_state() -> i32 {
    let f = env("SHIM_NRESTARTS");
    if f.is_empty() {
        return 0;
    }
    let mut n: u32 = fs::read_to_string(&f)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let active = std::path::Path::new(&env("SHIM_STATE")).exists();
    if !active {
        n += 1;
        let _ = fs::write(&f, n.to_string());
    }
    let (st, sub) = if active {
        ("active", "running")
    } else {
        ("activating", "auto-restart")
    };
    println!("ActiveState={st}\nSubState={sub}\nNRestarts={n}");
    0
}

fn journal() -> i32 {
    println!("tuner: bind 127.0.0.1:8650: boom");
    0
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let me = std::path::Path::new(&argv[0])
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    record(&format!("{me} {}", args.join(" ")));
    let code = match me.as_str() {
        "journalctl" => journal(),
        "systemctl" => match args.as_slice() {
            ["is-active", "--quiet", u] => systemctl("is-active", Some(u)),
            ["show", "-p", "ActiveState", "-p", "SubState", "-p", "NRestarts", _] => unit_state(),
            ["show", .., u] => systemctl("show", Some(u)),
            [verb, u, ..] => systemctl(verb, Some(u)),
            _ => 64,
        },
        "ctl" => match args.as_slice() {
            ["exec-path", _] => {
                println!("{}", env("SHIM_TARGET"));
                0
            }
            ["journal", ..] => journal(),
            ["show", _] => unit_state(),
            ["known", u] => systemctl("cat", Some(&format!("{u}.service"))),
            [verb, u, ..] => systemctl(verb, Some(&format!("{u}.service"))),
            _ => 64,
        },
        "data-backup" => {
            let secs: f64 = env("SHIM_DATA_SLEEP").parse().unwrap_or(0.0);
            println!("galion-backup.sh (sim stub): sleeping {secs} s like the real snapshot");
            if !env("SHIM_DATA_MARK").is_empty() {
                let _ = fs::write(env("SHIM_DATA_MARK"), "ran\n");
            }
            std::thread::sleep(std::time::Duration::from_secs_f64(secs));
            0
        }
        _ => 64,
    };
    std::process::exit(code);
}
