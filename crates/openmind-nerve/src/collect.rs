// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! One collection cycle: run every pull probe once and turn the results
//! (plus pushed signals) into one [`Sample`] per component.

use crate::config::{Config, Kind, Spec};
use crate::hub::Hub;
use crate::probe;
use crate::state::Sample;
use crate::util::run;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// Per deep route probe (console admin/board, N-307-1); kept short so a
/// slow board cannot stretch the cycle.
const DEEP_TIMEOUT: Duration = Duration::from_secs(2);

/// Map galion-health check names onto nerve component names.
fn health_name(n: &str) -> &str {
    match n {
        "one" => "galion-one",
        other => other,
    }
}

pub fn find_gaia(cfg: &Config) -> Option<PathBuf> {
    if let Some(b) = &cfg.gaia_bin {
        let p = PathBuf::from(b);
        return p.exists().then_some(p);
    }
    let mut dirs: Vec<PathBuf> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(PathBuf::from)
        .collect();
    dirs.push(PathBuf::from("/opt/galion/rust/bin"));
    dirs.into_iter()
        .map(|d| d.join("gaia"))
        .find(|p| p.is_file())
}

/// Summarise `gaia status` JSON without assuming its exact schema.
pub fn gaia_summary(v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(o) = v.as_object() {
        for (k, x) in o.iter().take(12) {
            let short = match x {
                Value::Array(a) => json!(format!("[{} items]", a.len())),
                Value::Object(m) => json!(format!("{{{} keys}}", m.len())),
                other => other.clone(),
            };
            out.insert(k.clone(), short);
        }
    }
    Value::Object(out)
}

/// Own CPU time in ns from /proc/self/stat (utime+stime at 100 Hz ticks).
pub fn self_cpu_nsec() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = s.rsplit_once(") ")?.1;
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ut: u64 = f.get(11)?.parse().ok()?;
    let st: u64 = f.get(12)?.parse().ok()?;
    Some((ut + st) * 10_000_000)
}

/// Everything one collection pass produced (live probes or a replay).
#[derive(Debug, Clone, Default)]
pub struct Collected {
    pub samples: HashMap<String, Sample>,
    pub probe_errors: Vec<String>,
    /// Raw journal lines of this window (for the cortex).
    pub lines: Vec<probe::JLine>,
    /// Listening sockets (None: not read).
    pub listeners: Option<Vec<probe::Listener>>,
    /// Holders (from /proc or `ss -ltnp`) of listeners no component owns.
    pub port_procs: std::collections::BTreeMap<u16, crate::owners::ProcInfo>,
    /// Probes intentionally skipped on this host (info, not errors).
    pub skipped: Vec<String>,
}

/// Summary of a non-2xx JSON health body (`status`, `model`, `loaded`,
/// `ok`, `error`, `provider`): what a 503 actually says. Other keys are
/// dropped so no payload is echoed.
pub fn health_body_summary(body: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(body.trim()).ok()?;
    let o = v.as_object()?;
    let mut m = serde_json::Map::new();
    for k in ["status", "model", "loaded", "ok", "error", "provider"] {
        if let Some(x) = o.get(k) {
            let x = match x {
                Value::String(s) => Value::from(s.chars().take(80).collect::<String>()),
                Value::Bool(_) | Value::Number(_) | Value::Null => x.clone(),
                _ => continue,
            };
            m.insert(k.to_string(), x);
        }
    }
    (!m.is_empty()).then_some(Value::Object(m))
}

pub fn collect(specs: &[Spec], cfg: &Config, hub: &Hub, since: u64, now: u64) -> Collected {
    let mut errs = vec![];
    let mut skipped = vec![];
    let units: Vec<&str> = specs.iter().filter_map(|s| s.unit).collect();
    let infos = if !cfg.systemd {
        skipped.push("systemd (NERVE_SYSTEMD=0)".to_string());
        HashMap::new()
    } else {
        match probe::systemd_units(&units) {
            Ok(m) => m,
            Err(e) => {
                errs.push(format!("systemd: {e}"));
                HashMap::new()
            }
        }
    };
    let lines = if !cfg.journal || units.is_empty() {
        skipped.push("journald (NERVE_JOURNAL=0 or no units)".to_string());
        vec![]
    } else {
        match probe::journal_lines(&units, since) {
            Ok(v) => v,
            Err(e) => {
                errs.push(format!("journald: {e}"));
                vec![]
            }
        }
    };
    let journal = probe::journal_counts(&lines, &units);
    let health: HashMap<String, bool> = match cfg.health_url_port {
        Some(p) => {
            // Full body (http_probe keeps only 512 bytes).
            let body = crate::util::http(
                crate::util::lo(p),
                "/status.json",
                None,
                Duration::from_secs(8),
            )
            .map(|r| r.body)
            .unwrap_or_default();
            probe::parse_galion_health(&body)
                .map(|m| {
                    m.into_iter()
                        .map(|(k, v)| (health_name(&k).to_string(), v))
                        .collect()
                })
                .unwrap_or_default()
        }
        None => HashMap::new(),
    };
    let mut http_jobs: Vec<HttpJob> = vec![];
    let mut out = HashMap::new();
    for sp in specs {
        let mut s = Sample {
            t: now,
            extra: json!({}),
            ..Default::default()
        };
        if let Some(age) = hub.age(sp.name, now) {
            s.extra["push_age_s"] = json!(age);
        }
        s.pushed_errors = hub.errors_since(sp.name, since);
        s.health_says = health.get(sp.name).copied();
        if let Some(u) = sp.unit {
            if infos.is_empty() && !cfg.systemd {
                // No systemd on this host: the HTTP probe decides.
                s.present = sp.expected;
            } else if infos.is_empty() {
                // systemd unreadable: let the HTTP probe decide, never act on it.
                s.present = sp.expected;
                s.extra["degraded"] = json!("systemd state unavailable");
            } else if let Some(i) = infos.get(u) {
                s.present = i.found();
                s.unit_active = Some(i.active.clone());
                s.unit_sub = Some(i.sub.clone());
                s.unit_result = Some(i.result.clone());
                s.file_state = Some(i.file_state.clone());
                s.pid = i.main_pid;
                s.n_restarts = i.n_restarts;
                s.active_enter = i.active_enter_mono;
                s.cpu_nsec = i.cpu_nsec;
                s.mem_max_kb = i.memory_max.map(|b| b / 1024);
                s.rss_kb = (i.main_pid > 0)
                    .then(|| probe::proc_rss_kb(i.main_pid))
                    .flatten();
                s.mem_kb = i.memory_current.map(|b| b / 1024).or(s.rss_kb);
            }
            // P-233-1: "N errors in 5 min" must use the real last 5 minutes,
            // not every line since the previous cycle (event-mode gaps ≈ 900 s).
            // Full `lines` still go to the cortex via Collected.
            let j5 = probe::journal_counts_since(&lines, &[u], now.saturating_sub(300));
            if let Some(j) = j5.get(u) {
                s.journal_errors = j.errors;
                s.journal_warnings = j.warnings;
                s.last_error = j.last_error.clone();
            } else if let Some(j) = journal.get(u) {
                // Fallback: unit missing from filtered map (should not happen).
                s.journal_errors = j.errors;
                s.journal_warnings = j.warnings;
                s.last_error = j.last_error.clone();
            }
        }
        match sp.kind {
            Kind::Service => {
                if sp.unit.is_none() {
                    s.present = sp.port.is_some();
                }
                if let Some(p) = sp.port.filter(|_| s.present) {
                    // Probed after this loop on up to `probe_parallel`
                    // threads (slot 344, N-332-1).
                    http_jobs.push(HttpJob {
                        name: sp.name,
                        port: p,
                        path: sp.path,
                        deep: sp.deep,
                    });
                }
            }
            Kind::Tunnel => {
                if let Some(mp) = sp.metrics_port.filter(|_| s.present) {
                    if let Some(m) = probe::cloudflared_metrics(mp) {
                        s.tunnel_conns = m.ha_connections;
                        s.tunnel_requests = m.total_requests;
                        s.tunnel_req_errors = m.request_errors;
                    }
                }
            }
            Kind::Host => {
                s.present = true;
                match probe::disk(&cfg.disk_path) {
                    Some((t, a)) => {
                        s.disk_total_kb = Some(t);
                        s.disk_avail_kb = Some(a);
                    }
                    None => errs.push("disk: df failed".into()),
                }
                match probe::meminfo() {
                    Some((t, a)) => {
                        s.ram_total_kb = Some(t);
                        s.ram_avail_kb = Some(a);
                    }
                    None => errs.push("ram: /proc/meminfo unreadable".into()),
                }
                if let Some((st, sf)) = probe::swapinfo() {
                    s.extra["swap_total_kb"] = json!(st);
                    s.extra["swap_free_kb"] = json!(sf);
                }
                s.load1 = probe::loadavg();
                s.extra["cpus"] = json!(probe::cpus());
                s.extra["scaling"] = json!(
                    "single LXC: no horizontal scaling; remediation is restart/rollback only"
                );
            }
            Kind::Gaia => {
                if let Some(bin) = find_gaia(cfg) {
                    s.present = true;
                    match run(
                        &bin.display().to_string(),
                        &["status"],
                        Duration::from_secs(10),
                    ) {
                        Some(o) if o.code == 0 => match serde_json::from_str::<Value>(&o.stdout) {
                            Ok(v) => s.extra["gaia_status"] = gaia_summary(&v),
                            Err(_) => s.extra["degraded"] = json!("gaia status printed non-JSON"),
                        },
                        _ => s.extra["degraded"] = json!("gaia status failed"),
                    }
                } else {
                    s.present = false;
                }
            }
            Kind::SelfNerve => {
                s.present = true;
                s.rss_kb = probe::proc_rss_kb(std::process::id());
                s.mem_kb = s.rss_kb;
                s.cpu_nsec = self_cpu_nsec();
                s.pid = std::process::id();
            }
        }
        out.insert(sp.name.to_string(), s);
    }
    for (job, done) in http_jobs
        .iter()
        .zip(run_http_jobs(&http_jobs, cfg.probe_parallel))
    {
        if let Some(s) = out.get_mut(job.name) {
            apply_http(s, job, done);
        }
    }
    let listeners = probe::listeners();
    // Holders of listeners nobody owns (in scope), for "unknown owner".
    let port_procs = match &listeners {
        Some(ls) => {
            let om = crate::owners::OwnerMap::for_config(specs, cfg);
            let in_scope = |p: u16| cfg.listen_scope.is_none_or(|(a, b)| p >= a && p <= b);
            let ports: std::collections::BTreeSet<u16> = ls
                .iter()
                .map(|l| l.port)
                .filter(|p| in_scope(*p) && !om.is_owned(*p))
                .collect();
            crate::owners::socket_procs(&ports)
        }
        None => Default::default(),
    };
    Collected {
        samples: out,
        probe_errors: errs,
        lines,
        listeners,
        port_procs,
        skipped,
    }
}

/// One component's HTTP health probe (+ deep routes), slot 344.
#[derive(Debug, Clone)]
pub struct HttpJob {
    pub name: &'static str,
    pub port: u16,
    pub path: &'static str,
    pub deep: &'static [&'static str],
}

/// Result of an [`HttpJob`]: the health probe and, when it passed, the deep
/// route records.
#[derive(Debug, Clone)]
pub struct HttpDone {
    pub probe: probe::HttpProbe,
    pub deep: Option<Vec<Value>>,
}

fn run_http_job(j: &HttpJob) -> HttpDone {
    let r = probe::http_probe(j.port, j.path, HTTP_TIMEOUT);
    let deep = (r.ok() && !j.deep.is_empty()).then(|| {
        j.deep
            .iter()
            .map(|path| {
                let d = probe::http_probe(j.port, path, DEEP_TIMEOUT);
                json!({"path": path, "http": d.code, "ok": probe::deep_route_ok(d.code),
                       "ms": d.code.map(|_| d.ms), "error": d.error})
            })
            .collect()
    });
    HttpDone { probe: r, deep }
}

/// Run the jobs on at most `parallel` threads (the caller is one of them),
/// results in job order. N-332-1 / slot 344: the old walk was sequential, so
/// k hung components cost k x 3 s (+2 s per deep route) per cycle; now the
/// cycle pays about ceil(slow / parallel) x the per-job budget. A failed
/// thread spawn only lowers the parallelism (the caller drains the queue).
pub fn run_http_jobs(jobs: &[HttpJob], parallel: usize) -> Vec<HttpDone> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let workers = parallel.clamp(1, 16).min(jobs.len());
    if workers <= 1 {
        return jobs.iter().map(run_http_job).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<HttpDone>>> = Mutex::new(vec![None; jobs.len()]);
    let work = || loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        let Some(j) = jobs.get(i) else { break };
        let d = run_http_job(j);
        slots.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(d);
    };
    std::thread::scope(|sc| {
        for _ in 1..workers {
            let spawned = std::thread::Builder::new()
                .name("nerve-probe".into())
                .stack_size(256 * 1024)
                .spawn_scoped(sc, work);
            if spawned.is_err() {
                break;
            }
        }
        work();
    });
    slots
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .into_iter()
        .zip(jobs)
        .map(|(d, j)| d.unwrap_or_else(|| run_http_job(j)))
        .collect()
}

fn apply_http(s: &mut Sample, job: &HttpJob, done: HttpDone) {
    let r = done.probe;
    s.http_code = r.code;
    s.latency_ms = r.code.map(|_| r.ms);
    s.probe_error = r.error.clone();
    if job.name == "galion-one-mouth" && r.body.contains("\"loading\"") {
        s.extra["mouth"] = json!("model loading");
    }
    if !r.ok() {
        if let Some(b) = health_body_summary(&r.body) {
            s.extra["health_body"] = b;
        }
    }
    if let Some(deep) = done.deep {
        s.extra["deep"] = Value::Array(deep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaia_summary_is_shallow() {
        let v = json!({"functions": [1, 2, 3], "tick": 4, "bus": {"a": 1}});
        let s = gaia_summary(&v);
        assert_eq!(s["functions"], "[3 items]");
        assert_eq!(s["tick"], 4);
        assert_eq!(s["bus"], "{1 keys}");
        assert_eq!(health_name("one"), "galion-one");
    }

    #[test]
    fn self_cpu_reads() {
        assert!(self_cpu_nsec().is_some());
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;

    #[test]
    fn health_body_keeps_only_status_keys() {
        let b = health_body_summary(r#"{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false,"secret":"x","nested":{"a":1}}"#).unwrap();
        assert_eq!(
            b,
            json!({"ok": false, "status": "loading", "provider": "local", "model": "chatterbox", "loaded": false})
        );
        assert!(health_body_summary("busy").is_none());
        assert!(health_body_summary("{}").is_none());
    }

    #[test]
    fn http_jobs_run_bounded_parallel_in_order() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        // One listener that accepts and never answers (holds sockets), one
        // that answers 200 at once.
        let hang = TcpListener::bind("127.0.0.1:0").unwrap();
        let hp = hang.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut held = vec![];
            for s in hang.incoming().flatten().take(16) {
                held.push(s);
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        });
        let ok = TcpListener::bind("127.0.0.1:0").unwrap();
        let op = ok.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut s in ok.incoming().flatten().take(16) {
                let mut b = [0u8; 512];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\nok");
            }
        });
        let mut jobs = vec![];
        for i in 0..7 {
            let port = if i == 3 { op } else { hp };
            jobs.push(HttpJob {
                name: "x",
                port,
                path: "/health",
                deep: &[],
            });
        }
        let t = std::time::Instant::now();
        let out = run_http_jobs(&jobs, 8);
        let el = t.elapsed();
        assert_eq!(out.len(), 7);
        assert_eq!(out[3].probe.code, Some(200), "{:?}", out[3]);
        for (i, d) in out.iter().enumerate().filter(|(i, _)| *i != 3) {
            assert_eq!(d.probe.code, None, "job {i}: {d:?}");
            assert!(
                d.probe.error.as_deref().unwrap_or("").contains("timeout"),
                "{d:?}"
            );
        }
        // 6 hung jobs: sequential = 6 x 3 s = 18 s; parallel ~ one budget.
        assert!(el < HTTP_TIMEOUT * 2, "took {el:?}");
        // parallel 1 keeps the sequential path (and the same order)
        let one = run_http_jobs(&jobs[3..4], 1);
        assert_eq!(one[0].probe.code, Some(200));
    }
}
