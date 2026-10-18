// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Run one probe spec into a [`Row`] with guardian-tick.py statuses:
//! `healthy`, `unhealthy` (wrong code, warn level, stale) or `critical`
//! (no answer, crit level, process missing).

use crate::config::Spec;
use openmind_survival::{
    check_freshness, check_loadavg, check_meminfo, http_get, parse_df, truncate, verdict, Runner,
};
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub status: &'static str,
    pub ms: f64,
    pub error: Option<String>,
    pub detail: String,
}

impl Row {
    pub fn ok(&self) -> bool {
        self.status == "healthy"
    }
}

fn row(name: &str, status: &'static str, ms: f64, detail: String) -> Row {
    Row {
        name: name.to_string(),
        status,
        ms: (ms * 10.0).round() / 10.0,
        error: (status != "healthy").then(|| detail.clone()),
        detail,
    }
}

fn level_status(detail: &str, ok: bool) -> &'static str {
    if ok {
        "healthy"
    } else if detail.contains("crit") {
        "critical"
    } else {
        "unhealthy"
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve: {e}"))?
        .next()
        .ok_or_else(|| "resolve: no address".to_string())
}

/// Scan `<proc>/*/comm` for an exact process name (read-only).
pub fn find_process(proc_root: &Path, comm: &str) -> Option<String> {
    for ent in std::fs::read_dir(proc_root).ok()?.flatten() {
        let n = ent.file_name();
        let n = n.to_string_lossy();
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(c) = std::fs::read_to_string(ent.path().join("comm")) {
            if c.trim() == comm {
                return Some(n.into_owned());
            }
        }
    }
    None
}

pub struct Env<'a> {
    pub runner: &'a dyn Runner,
    pub proc_root: &'a Path,
    pub has_systemd: bool,
}

/// True when this host runs systemd as init (`/run/systemd/system` exists).
pub fn systemd_present() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

pub fn run(spec: &Spec, env: &Env) -> Row {
    let t0 = Instant::now();
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
    match spec {
        Spec::Http {
            name,
            host,
            port,
            path,
            ok_codes,
            loading_ok,
            timeout_ms,
        } => {
            let addr = match resolve(host, *port) {
                Ok(a) => a,
                Err(e) => return row(name, "critical", ms(t0), e),
            };
            let out = http_get(addr, path, Duration::from_millis(*timeout_ms));
            let codes: &[u16] = if ok_codes.is_empty() {
                &[200]
            } else {
                ok_codes
            };
            let (ok, detail) = verdict(&out.result, codes, *loading_ok);
            let status = match (&out.result, ok) {
                (_, true) => "healthy",
                (Err(_), false) => "critical",
                (Ok(_), false) => "unhealthy",
            };
            row(name, status, out.ms, detail)
        }
        Spec::Unit { name, unit } => {
            let label = name.clone().unwrap_or_else(|| format!("unit:{unit}"));
            if !env.has_systemd {
                return row(&label, "healthy", 0.0, "skipped: no systemd".into());
            }
            match env.runner.run("systemctl", &["is-active", unit]) {
                Ok(o) => {
                    let s = o.trim().to_string();
                    let st = if s == "active" { "healthy" } else { "critical" };
                    row(&label, st, ms(t0), s)
                }
                Err(e) => row(&label, "critical", ms(t0), e),
            }
        }
        Spec::Disk { path, warn, crit } => {
            let name = format!("disk:{path}");
            match env.runner.run("df", &["-P", path]) {
                Ok(o) => match parse_df(&o) {
                    Some(p) => {
                        let st = if p >= *crit {
                            "critical"
                        } else if p >= *warn {
                            "unhealthy"
                        } else {
                            "healthy"
                        };
                        row(&name, st, ms(t0), format!("used {p:.1}%"))
                    }
                    None => row(&name, "unhealthy", ms(t0), "unparseable df output".into()),
                },
                Err(e) => row(&name, "unhealthy", ms(t0), e),
            }
        }
        Spec::Mem => {
            let r = match std::fs::read_to_string(env.proc_root.join("meminfo")) {
                Ok(t) => check_meminfo(&t),
                Err(e) => return row("mem", "unhealthy", ms(t0), e.to_string()),
            };
            row("mem", level_status(&r.detail, r.ok), ms(t0), r.detail)
        }
        Spec::Load => {
            let r = match std::fs::read_to_string(env.proc_root.join("loadavg")) {
                Ok(t) => {
                    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
                    check_loadavg(&t, cores)
                }
                Err(e) => return row("load", "unhealthy", ms(t0), e.to_string()),
            };
            row("load", level_status(&r.detail, r.ok), ms(t0), r.detail)
        }
        Spec::Fresh { path, max_age_s } => {
            let r = check_freshness(Path::new(path), *max_age_s);
            let st = if r.ok { "healthy" } else { "unhealthy" };
            row(&r.name, st, ms(t0), r.detail)
        }
        Spec::Process { name, comm } => {
            let label = name.clone().unwrap_or_else(|| "tunnel".into());
            match find_process(env.proc_root, comm) {
                Some(pid) => row(&label, "healthy", ms(t0), format!("pid {pid}")),
                None => row(
                    &label,
                    "critical",
                    ms(t0),
                    truncate(&format!("{comm} not running"), 160).to_string(),
                ),
            }
        }
    }
}
