// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Replay: run data captured read-only from a live host through the same
//! pipeline the live loop uses (`Engine::cycle_with`).
//!
//! A capture directory holds (all optional except at least one state file):
//!
//! | file | what | captured with (read-only) |
//! |---|---|---|
//! | `state-*.json` | galion-nerve `GET /state` snapshots, one cycle each, any number | `curl -s 127.0.0.1:8670/state` |
//! | `journal.jsonl` | journald records of the stack units | `journalctl -o json --output-fields=... -u ...` |
//! | `ss.txt` | listening sockets | `ss -ltn` |
//! | `health_status.json` | galion-health report | `curl -s 127.0.0.1:8660/status.json` |
//! | `body-<port>.json` | body of a failing health endpoint | `curl -s 127.0.0.1:8002/health` |
//! | `access-<name>.log` | an access log (CLF or JSON lines) | `cat` / `tail` |
//!
//! Each state snapshot becomes one cycle at its own timestamp; journal lines
//! are split into the windows between snapshots. Nothing in a capture is
//! written back anywhere; reflexes and alerts are off.

use crate::access::{parse_line, Hit};
use crate::collect::{health_body_summary, Collected};
use crate::config::Config;
use crate::engine::Engine;
use crate::probe::{journal_counts, parse_galion_health, parse_journal_lines, parse_ss, JLine};
use crate::state::{Sample, Status};
use crate::util::parse_rfc3339;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

fn status_of(s: &str) -> Option<Status> {
    Some(match s {
        "healthy" => Status::Healthy,
        "degrading" => Status::Degrading,
        "failing-soon" => Status::FailingSoon,
        "failing" => Status::Failing,
        "absent" => Status::Absent,
        _ => return None,
    })
}

/// One captured component (old galion-nerve `/state` entry) as a sample.
pub fn sample_from_state(c: &Value, t: u64) -> Sample {
    let u = |k: &str| c.get(k).and_then(Value::as_u64);
    let st = |k: &str| c.get(k).and_then(Value::as_str).map(str::to_string);
    let tun = &c["tunnel"];
    let host = &c["host"];
    let mut extra = c
        .get("extra")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(o) = extra.as_object_mut() {
        // Recomputed by this build.
        o.remove("degraded");
        o.remove("cycle_ms");
    }
    Sample {
        t,
        present: c["status"].as_str() != Some("absent"),
        unit_active: st("unit_state"),
        unit_sub: st("unit_sub"),
        unit_result: None,
        file_state: st("enabled"),
        http_code: u("http").map(|x| x as u16),
        latency_ms: u("latency_ms"),
        probe_error: None,
        mem_kb: u("mem_kb"),
        rss_kb: u("rss_kb"),
        mem_max_kb: u("mem_max_kb"),
        cpu_nsec: None,
        cpu_pct: c.get("cpu_pct").and_then(Value::as_f64),
        pid: u("pid").unwrap_or(0) as u32,
        n_restarts: u("n_restarts_systemd").unwrap_or(0) as u32,
        active_enter: 0,
        journal_errors: 0,
        journal_warnings: 0,
        last_error: None,
        pushed_errors: 0,
        tunnel_conns: tun.get("ha_connections").and_then(Value::as_u64),
        tunnel_requests: tun.get("requests").and_then(Value::as_u64),
        tunnel_req_errors: tun.get("request_errors").and_then(Value::as_u64),
        disk_total_kb: host.get("disk_total_kb").and_then(Value::as_u64),
        disk_avail_kb: host.get("disk_avail_kb").and_then(Value::as_u64),
        ram_total_kb: host.get("ram_total_kb").and_then(Value::as_u64),
        ram_avail_kb: host.get("ram_avail_kb").and_then(Value::as_u64),
        load1: host.get("load1").and_then(Value::as_f64),
        health_says: c.get("galion_health_ok").and_then(Value::as_bool),
        extra,
    }
}

pub struct ReplayOut {
    pub engine: Engine,
    pub cycles: usize,
    pub notes: Vec<String>,
}

/// Replay a capture directory. `cfg` supplies thresholds/ports; reflexes,
/// alerts and file writes are forced off.
pub fn replay_dir(dir: &Path, cfg: Config) -> Result<ReplayOut, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut states: Vec<(u64, Value)> = vec![];
    let mut journal = String::new();
    let mut ss = None;
    let mut ss_procs = std::collections::BTreeMap::new();
    let mut health: HashMap<String, bool> = HashMap::new();
    let mut bodies: HashMap<u16, Value> = HashMap::new();
    let mut access: Vec<(String, String)> = vec![];
    let mut notes = vec![];
    let mut names: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    names.sort();
    for p in names {
        let n = p
            .file_name()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
        let read = || std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()));
        if n.starts_with("state") && n.ends_with(".json") {
            let v: Value = serde_json::from_str(&read()?).map_err(|e| format!("{n}: {e}"))?;
            let t = v["ts"]
                .as_str()
                .and_then(parse_rfc3339)
                .ok_or(format!("{n}: no ts"))?;
            states.push((t, v));
        } else if n.ends_with(".jsonl") {
            journal.push_str(&read()?);
            journal.push('\n');
        } else if n == "ss.txt" {
            let t = read()?;
            ss = Some(parse_ss(&t));
            ss_procs = crate::owners::parse_ss_procs(&t);
        } else if n == "health_status.json" {
            health = parse_galion_health(&read()?).unwrap_or_default();
        } else if let Some(port) = n
            .strip_prefix("body-")
            .and_then(|x| x.strip_suffix(".json"))
            .and_then(|x| x.parse().ok())
        {
            if let Some(b) = health_body_summary(&read()?) {
                bodies.insert(port, b);
            }
        } else if let Some(name) = n
            .strip_prefix("access-")
            .and_then(|x| x.strip_suffix(".log"))
        {
            access.push((name.to_string(), read()?));
        }
    }
    if states.is_empty() {
        return Err(format!("{}: no state-*.json snapshot", dir.display()));
    }
    states.sort_by_key(|x| x.0);
    let lines: Vec<JLine> = parse_journal_lines(&journal, &[]);
    notes.push(format!(
        "{} state snapshot(s) {} .. {}, {} journal line(s), listeners: {}, galion-health entries: {}, health bodies: {:?}",
        states.len(),
        crate::util::ts(states[0].0),
        crate::util::ts(states[states.len() - 1].0),
        lines.len(),
        ss.as_ref().map(|v| v.len().to_string()).unwrap_or("-".into()),
        health.len(),
        bodies.keys().collect::<Vec<_>>()
    ));
    let mut e = Engine::new(cfg)?;
    e.reflexes_enabled = false;
    e.live = false;
    e.write_files = false;
    e.alerts.path = None;
    e.alerts.to_journal = false;
    e.alerts.webhook = None;
    let units: Vec<&'static str> = e.comps.iter().filter_map(|c| c.spec.unit).collect();
    let mut hits: Vec<(String, Option<String>, Hit)> = vec![];
    for (name, text) in &access {
        for l in text.lines() {
            if let Some(h) = parse_line(l, states[0].0) {
                hits.push((name.clone(), None, h));
            }
        }
    }
    let mut prev = 0u64;
    for (k, (t, v)) in states.iter().enumerate() {
        let win: Vec<JLine> = lines
            .iter()
            .filter(|l| (k == 0 || l.t > prev) && l.t <= *t)
            .cloned()
            .collect();
        // Error counts use the last 5 min of the window (what a live cycle sees
        // first); the cortex reads the whole window.
        let recent: Vec<JLine> = win.iter().filter(|l| l.t + 300 > *t).cloned().collect();
        let counts = journal_counts(&recent, &units);
        let mut col = Collected {
            lines: win,
            listeners: ss.clone(),
            port_procs: ss_procs.clone(),
            ..Default::default()
        };
        for c in v["components"].as_array().into_iter().flatten() {
            let Some(name) = c["name"].as_str() else {
                continue;
            };
            let mut s = sample_from_state(c, *t);
            if let Some(spec) = e
                .comps
                .iter()
                .find(|x| x.spec.name == name)
                .map(|x| x.spec.clone())
            {
                if let Some(u) = spec.unit {
                    if let Some(j) = counts.get(u) {
                        s.journal_errors = j.errors;
                        s.journal_warnings = j.warnings;
                        s.last_error = j.last_error.clone();
                    }
                }
                if let Some(p) = spec.port {
                    if !matches!(s.http_code, Some(200..=399)) {
                        if let Some(b) = bodies.get(&p) {
                            s.extra["health_body"] = b.clone();
                        }
                    }
                }
                let hname = if name == "galion-one" { "one" } else { name };
                if let Some(ok) = health.get(hname) {
                    s.health_says = Some(*ok);
                }
            }
            if k == 0 {
                // Seed "since" from the capture so long-running states keep their age.
                if let (Some(cs), Some(since)) = (
                    e.comps.iter_mut().find(|x| x.spec.name == name),
                    c["status_since"].as_str().and_then(parse_rfc3339),
                ) {
                    if let Some(st) = c["status"].as_str().and_then(status_of) {
                        cs.status = st;
                        cs.status_since = since;
                    }
                }
            }
            col.samples.insert(name.to_string(), s);
        }
        for p in v["probe_errors"].as_array().into_iter().flatten() {
            if let Some(p) = p.as_str() {
                col.probe_errors.push(format!("captured: {p}"));
            }
        }
        let h = if k == 0 {
            std::mem::take(&mut hits)
        } else {
            vec![]
        };
        e.cycle_with(col, h, *t, Instant::now());
        prev = *t;
    }
    Ok(ReplayOut {
        cycles: states.len(),
        engine: e,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_a_vps_like_capture() {
        let dir = std::env::temp_dir().join(format!("nerve-replay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let comp = |name: &str, status: &str, http: Option<u16>| {
            json!({"name": name, "status": status, "status_since": "2026-10-18T22:35:04+02:00",
            "unit_state": "active", "http": http, "mem_kb": 1000, "extra": {}})
        };
        for (i, ts) in [
            "2026-10-20T19:05:05+02:00",
            "2026-10-20T19:05:35+02:00",
            "2026-10-20T19:06:05+02:00",
        ]
        .iter()
        .enumerate()
        {
            let mut cs = vec![
                comp("openmind-memory", "healthy", Some(200)),
                comp("hermes-rs", "healthy", Some(200)),
                comp("link-watch-rs", "healthy", None),
                comp("galion-desk", "healthy", Some(200)),
                comp("tuner", "healthy", Some(200)),
                comp("galion-one", "healthy", Some(200)),
                json!({"name": "galion-one-mouth", "status": "degrading", "status_since": "2026-10-18T22:35:04+02:00", "unit_state": null, "http": 503, "extra": {"mouth": "model loading"}}),
                comp("console", "healthy", Some(302)),
                comp("galion-health", "healthy", Some(200)),
                json!({"name": "cloudflared", "status": "healthy", "unit_state": "active", "tunnel": {"ha_connections": 4, "requests": 81900 + i as u64 * 20, "request_errors": 48560 + i as u64 * 15}}),
                json!({"name": "tuner-quick", "status": "healthy", "unit_state": "active", "tunnel": {"ha_connections": 1, "requests": 21, "request_errors": 0}}),
                json!({"name": "voice-worker", "status": "absent"}),
                json!({"name": "gaia", "status": "absent"}),
                json!({"name": "lxc-host", "status": "healthy", "host": {"disk_total_kb": 100, "disk_avail_kb": 75, "ram_total_kb": 100, "ram_avail_kb": 90, "load1": 0.5}}),
                json!({"name": "galion-nerve", "status": "healthy", "mem_kb": 3000}),
            ];
            cs.retain(|c| !c.is_null());
            std::fs::write(
                dir.join(format!("state-{i}.json")),
                json!({"ts": ts, "components": cs, "probe_errors": []}).to_string(),
            )
            .unwrap();
        }
        let base = 1_792_516_505u64; // 2026-10-20T19:15:05+02:00 is later; lines sit inside the window below
        let _ = base;
        let t0 = parse_rfc3339("2026-10-20T18:40:05+02:00").unwrap();
        let mut j = String::new();
        for k in 0..100u64 {
            let port = if k % 2 == 0 { 4001 } else { 3011 };
            let host = if k % 2 == 0 {
                "galion.studio"
            } else {
                "merlinelement.com"
            };
            j.push_str(&json!({"_SYSTEMD_UNIT": "cloudflared.service", "PRIORITY": "6", "__REALTIME_TIMESTAMP": ((t0 + k * 15) * 1_000_000).to_string(),
                "MESSAGE": format!("2026-10-20T16:40:00Z ERR Request failed error=\"dial tcp [::1]:{port}: connect: connection refused\" dest=https://{host}/ ip=198.41.200.13 type=http")}).to_string()); // repo-check: allow (test fixture address, never dialled)
            j.push('\n');
        }
        std::fs::write(dir.join("journal.jsonl"), j).unwrap();
        std::fs::write(dir.join("ss.txt"), "LISTEN 0 128 127.0.0.1:8420 0.0.0.0:*\nLISTEN 0 128 127.0.0.1:8652 0.0.0.0:*\nLISTEN 0 4096 *:22 *:*\n").unwrap();
        std::fs::write(dir.join("body-8002.json"), r#"{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}"#).unwrap();
        let mut cfg = Config::from_env(&|_| None).unwrap();
        cfg.data_dir = dir.join("data");
        let out = replay_dir(&dir, cfg).unwrap();
        assert_eq!(out.cycles, 3);
        let e = out.engine;
        let r = &e.shared.lock().unwrap().report;
        assert_eq!(
            r["pass"],
            false,
            "{}",
            serde_json::to_string_pretty(r).unwrap()
        );
        let causes: Vec<&str> = r["incidents"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|i| i["cause"].as_str())
            .collect();
        assert!(causes.contains(&"edge"), "{causes:?}");
        assert!(causes.contains(&"galion-one"), "{causes:?}");
        let one = r["incidents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cause"] == "galion-one")
            .unwrap();
        assert_eq!(one["severity"], "error");
        assert!(
            one["signals"][0]["msg"]
                .as_str()
                .unwrap()
                .contains("stuck loading for 1.9 d"),
            "{}",
            one["signals"][0]["msg"]
        );
        assert!(one["signals"][0]["msg"]
            .as_str()
            .unwrap()
            .contains("chatterbox"));
        let edge = r["incidents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cause"] == "edge")
            .unwrap();
        assert!(
            edge["title"].as_str().unwrap().contains(":3011 :4001"),
            "{}",
            edge["title"]
        );
        assert!(r["score"].as_u64().unwrap() < 70);
        assert_eq!(r["system"], "degraded");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
