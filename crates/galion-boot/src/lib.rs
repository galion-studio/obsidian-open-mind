// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! galion-boot: replaces galion-light keep.sh, stack-boot.sh,
//! persist-boot.sh, vps-boot.sh, start.sh, hardware.sh, loop-daemon.sh and
//! the lock-secrets.sh step. Services run as real systemd units (rendered
//! here) or, where there is no systemd, under the built-in supervisor.

pub mod config;
pub mod hardware;
pub mod http;
pub mod secrets;
pub mod supervise;
pub mod units;

use openmind_survival::{http_get, verdict, Runner};
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::Duration;

/// True when systemd is init on this host.
pub fn systemd_present() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

/// `status`: one line per service, read-only. Returns (lines, all_ok).
pub fn status(f: &config::File, runner: &dyn Runner, systemd: bool) -> (Vec<String>, bool) {
    let mut all = true;
    let mut out = Vec::new();
    for s in &f.service {
        let backend = if systemd {
            runner
                .run("systemctl", &["is-active", &format!("{}.service", s.name)])
                .map(|o| o.trim().to_string())
                .unwrap_or_else(|e| e)
        } else {
            let pid = std::fs::read_to_string(f.boot.supervisor_dir.join("pids").join(&s.name))
                .ok()
                .map(|p| p.trim().to_string());
            match pid {
                Some(p) if Path::new("/proc").join(&p).exists() => format!("supervised pid {p}"),
                _ => "not running".into(),
            }
        };
        let health = match &s.health {
            None => "-".to_string(),
            Some(h) => match (h.host.as_str(), h.port)
                .to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
            {
                None => "resolve failed".into(),
                Some(addr) => {
                    let o = http_get(addr, &h.path, Duration::from_millis(2500));
                    let codes: &[u16] = if h.ok_codes.is_empty() {
                        &[200]
                    } else {
                        &h.ok_codes
                    };
                    let (ok, d) = verdict(&o.result, codes, true);
                    if !ok {
                        all = false;
                    }
                    d
                }
            },
        };
        let running = backend == "active" || backend.starts_with("supervised");
        if !running && s.timer_every_s.is_none() && s.kind == "simple" {
            all = false;
        }
        out.push(format!("{:<20} {:<22} health {}", s.name, backend, health));
    }
    (out, all)
}
