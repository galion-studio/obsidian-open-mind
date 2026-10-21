// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! simsup: box-only cutover rehearsal supervisor (no systemd on the box).
//!
//!   simsup --root DIR daemon              run every unit in DIR/units
//!   simsup --root DIR ctl VERB [UNIT] [N] known | exec-path | cat | show | is-active |
//!                                         start | stop | restart | reset-failed |
//!                                         journal UNIT N | status | daemon-reload | shutdown
//!   simsup probe ADDR PATH INTERVAL_MS STOPFILE MAX_S
//!   simsup --version
//!
//! `GALION_DEPLOY_SM=direct GALION_DEPLOY_CTL="simsup --root DIR ctl" galion-deploy ...`
//! drives a deploy against this supervisor instead of systemd.

mod probe;
mod sup;
mod unit;
mod zagreb;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage: simsup --root DIR (daemon | ctl VERB [UNIT] [N]) | simsup probe ADDR PATH INTERVAL_MS STOPFILE MAX_S | simsup --version";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut root: Option<PathBuf> = None;
    if args.first().map(String::as_str) == Some("--root") && args.len() >= 2 {
        root = Some(PathBuf::from(&args[1]));
        args.drain(..2);
    }
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("simsup {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("daemon") => {
            let Some(root) = root else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            match sup::Sup::new(&root).and_then(|mut s| s.run()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("simsup: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("ctl") if root.is_some() && args.len() >= 2 => {
            let code = sup::ctl(root.as_deref().expect("checked"), &args[1..]);
            ExitCode::from(code.clamp(0, 255) as u8)
        }
        Some("probe") if args.len() == 6 => {
            let (Ok(addr), Ok(ms), Ok(max)) = (
                args[1].parse(),
                args[3].parse::<u64>(),
                args[5].parse::<u64>(),
            ) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let iv = Duration::from_millis(ms.max(1));
            let s = probe::run(
                addr,
                &args[2],
                iv,
                &PathBuf::from(&args[4]),
                Duration::from_secs(max),
            );
            print!("{}", probe::report(addr, &args[2], iv, &s));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::path::Path;
    use std::time::Instant;

    fn root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("simsup-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn wait_for(f: impl Fn() -> bool, ms: u64) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn spawn(root: &Path) -> std::thread::JoinHandle<()> {
        let r = root.to_path_buf();
        let h = std::thread::spawn(move || {
            sup::Sup::new(&r).unwrap().run().unwrap();
        });
        assert!(wait_for(|| root.join("ctl.sock").exists(), 3000));
        h
    }

    fn ctl(root: &Path, a: &[&str]) -> i32 {
        sup::ctl(root, &a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn crash_loop_hits_start_limit_and_manual_restart_is_refused() {
        let r = root("limit");
        sup::write_unit(
            &r,
            "crashy",
            "[Unit]\nStartLimitIntervalSec=30\nStartLimitBurst=3\n[Service]\nExecStart=/bin/false\nRestart=always\nRestartSec=50ms\n",
        )
        .unwrap();
        let h = spawn(&r);
        assert!(wait_for(|| ctl(&r, &["is-active", "crashy"]) == 3, 2000));
        // 3 starts allowed in the window, the 4th is refused -> failed.
        std::thread::sleep(Duration::from_millis(600));
        let j = std::fs::read_to_string(r.join("journal/crashy.log")).unwrap();
        assert!(j.contains("start-limit-hit"), "{j}");
        assert_eq!(ctl(&r, &["restart", "crashy"]), 1);
        assert_eq!(ctl(&r, &["reset-failed", "crashy"]), 0);
        assert_eq!(ctl(&r, &["restart", "crashy"]), 0);
        assert_eq!(ctl(&r, &["shutdown"]), 0);
        h.join().unwrap();
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn restart_swaps_process_and_unknown_unit_fails() {
        let r = root("swap");
        sup::write_unit(
            &r,
            "sleeper",
            "[Service]\nExecStart=/bin/sleep 600\nRestart=always\nRestartSec=2\nTimeoutStopSec=5\n",
        )
        .unwrap();
        let h = spawn(&r);
        assert!(wait_for(|| ctl(&r, &["is-active", "sleeper"]) == 0, 2000));
        let pid1 = std::fs::read_to_string(r.join("run/sleeper.pid")).unwrap();
        assert_eq!(ctl(&r, &["restart", "sleeper"]), 0);
        let pid2 = std::fs::read_to_string(r.join("run/sleeper.pid")).unwrap();
        assert_ne!(pid1, pid2);
        assert_eq!(ctl(&r, &["is-active", "sleeper"]), 0);
        assert_eq!(ctl(&r, &["known", "nope"]), 1);
        assert_eq!(ctl(&r, &["exec-path", "sleeper"]), 0);
        assert_eq!(ctl(&r, &["shutdown"]), 0);
        h.join().unwrap();
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn exec_failure_is_auto_restarted_not_a_job_error() {
        let r = root("exec");
        sup::write_unit(
            &r,
            "ghost",
            "[Service]\nExecStart=/nonexistent/ghost\nRestart=always\nRestartSec=5\n",
        )
        .unwrap();
        let h = spawn(&r);
        assert_eq!(ctl(&r, &["restart", "ghost"]), 0);
        assert_eq!(ctl(&r, &["is-active", "ghost"]), 3);
        let j = std::fs::read_to_string(r.join("journal/ghost.log")).unwrap();
        assert!(j.contains("status=203/EXEC"), "{j}");
        assert_eq!(ctl(&r, &["shutdown"]), 0);
        h.join().unwrap();
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn probe_sees_refused_then_ok() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        assert_eq!(
            probe::try_once(addr, "/health", Duration::from_millis(200)),
            probe::Outcome::Refused
        );
        let l = TcpListener::bind(addr).unwrap();
        let t = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut c, _) = l.accept().unwrap();
            let mut b = [0u8; 256];
            let _ = c.read(&mut b);
            c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        assert_eq!(
            probe::try_once(addr, "/health", Duration::from_millis(500)),
            probe::Outcome::Ok
        );
        t.join().unwrap();
    }
}
