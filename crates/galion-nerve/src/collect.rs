// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

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

pub struct Collected {
    pub samples: HashMap<String, Sample>,
    pub probe_errors: Vec<String>,
}

pub fn collect(specs: &[Spec], cfg: &Config, hub: &Hub, since: u64, now: u64) -> Collected {
    let mut errs = vec![];
    let units: Vec<&str> = specs.iter().filter_map(|s| s.unit).collect();
    let infos = match probe::systemd_units(&units) {
        Ok(m) => m,
        Err(e) => {
            errs.push(format!("systemd: {e}"));
            HashMap::new()
        }
    };
    let journal = match probe::journal_scan(&units, since) {
        Ok(m) => m,
        Err(e) => {
            errs.push(format!("journald: {e}"));
            HashMap::new()
        }
    };
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
            if infos.is_empty() {
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
            if let Some(j) = journal.get(u) {
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
                    let r = probe::http_probe(p, sp.path, HTTP_TIMEOUT);
                    s.http_code = r.code;
                    s.latency_ms = r.code.map(|_| r.ms);
                    s.probe_error = r.error.clone();
                    if sp.name == "galion-one-mouth" && r.body.contains("\"loading\"") {
                        s.extra["mouth"] = json!("model loading");
                    }
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
    Collected {
        samples: out,
        probe_errors: errs,
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
