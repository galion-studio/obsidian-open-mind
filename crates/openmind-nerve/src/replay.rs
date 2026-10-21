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
//! | `systemctl-show.txt` | unit facts for components the snapshots lack (T2.3) | `systemctl show -p <probe::SHOW_PROPS> <units>` |
//! | `systemctl-runs.txt` | last run of the services behind timers (T2.3) | `systemctl show --timestamp=unix -p <probe::RUN_PROPS> <services>` |
//! | `http.txt` | `PORT CODE` per line: health probe of those components (T2.3) | `curl -s -o /dev/null -w '%{http_code}'` |
//!
//! A component of this build's table that a snapshot lacks (for example a
//! spec added after the capture's nerve build) is filled from the last three
//! files when they name it (the same facts for every cycle) and is otherwise
//! reported `absent` ("not in this capture"), never healthy or failing.
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

/// A sample for a component the snapshot lacks, from the capture's
/// `systemctl-show.txt` / `systemctl-runs.txt` / `http.txt` (T2.3). `None`:
/// the capture says nothing about it.
fn missing_sample(
    spec: &crate::config::Spec,
    t: u64,
    units: &HashMap<String, crate::probe::UnitInfo>,
    runs: &HashMap<String, crate::probe::LastRun>,
    http: &HashMap<u16, u16>,
) -> Option<Sample> {
    use crate::config::Kind;
    if !matches!(spec.kind, Kind::Service | Kind::Timer) {
        return None;
    }
    let info = spec.unit.and_then(|u| units.get(u));
    let code = spec.port.and_then(|p| http.get(&p)).copied();
    if info.is_none() && code.is_none() {
        return None;
    }
    let mut s = Sample {
        t,
        present: info.map_or(true, |i| i.found()),
        extra: json!({"source": "capture systemctl-show/http"}),
        ..Default::default()
    };
    if let Some(i) = info.filter(|i| i.found()) {
        s.unit_active = Some(i.active.clone());
        s.unit_sub = Some(i.sub.clone());
        s.unit_result = Some(i.result.clone());
        s.file_state = Some(i.file_state.clone());
        s.pid = i.main_pid;
        s.n_restarts = i.n_restarts;
        s.mem_max_kb = i.memory_max.map(|b| b / 1024);
        s.mem_kb = i.memory_current.map(|b| b / 1024);
    }
    if s.present {
        // curl prints 000 when nothing answered: probe down, not "HTTP 0".
        match code {
            Some(0) => s.probe_error = Some("no response (captured 000)".into()),
            c => s.http_code = c,
        }
        if spec.kind == Kind::Timer {
            if let Some(r) = spec
                .unit
                .and_then(crate::probe::timer_service)
                .and_then(|svc| runs.get(&svc))
            {
                s.extra["last_run"] = r.to_json();
            }
        }
    }
    Some(s)
}

pub struct ReplayOut {
    pub engine: Engine,
    pub cycles: usize,
    pub notes: Vec<String>,
}

/// Replay a capture directory. `cfg` supplies thresholds/ports; reflexes,
/// alerts and file writes are forced off.
pub fn replay_dir(dir: &Path, cfg: Config) -> Result<ReplayOut, String> {
    replay_dir_with(dir, cfg, 0, &mut |_| {})
}

/// The last captured snapshot moved `k` cycles past the capture: cumulative
/// tunnel counters keep growing at the rate seen across the capture, so a
/// condition that held while capturing keeps holding (T3.2 stretch).
fn stretch_state(first: &(u64, Value), last: &(u64, Value), extra_s: u64) -> Value {
    let mut v = last.1.clone();
    let span = last.0.saturating_sub(first.0);
    let rate = |name: &str, key: &str| -> u64 {
        if span == 0 {
            return 0;
        }
        let pick = |s: &Value| {
            s["components"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| c["name"] == name)
                .and_then(|c| c["tunnel"][key].as_u64())
        };
        match (pick(&first.1), pick(&last.1)) {
            (Some(a), Some(b)) => b.saturating_sub(a) * extra_s / span,
            _ => 0,
        }
    };
    let names: Vec<String> = v["components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect();
    for name in names {
        let (dr, de) = (rate(&name, "requests"), rate(&name, "request_errors"));
        if let Some(c) = v["components"]
            .as_array_mut()
            .and_then(|a| a.iter_mut().find(|c| c["name"] == name.as_str()))
        {
            for (k, d) in [("requests", dr), ("request_errors", de)] {
                if let Some(x) = c["tunnel"][k].as_u64() {
                    c["tunnel"][k] = json!(x + d);
                }
            }
        }
    }
    v
}

/// [`replay_dir`] with a hook to configure the engine before the first cycle
/// (the reflex dry-run replay turns reflexes back on, forced dry) and
/// `stretch` extra cycles that repeat the last capture window `interval_s`
/// apart: journal lines shifted forward, tunnel counters advanced.
pub fn replay_dir_with(
    dir: &Path,
    cfg: Config,
    stretch: usize,
    setup: &mut dyn FnMut(&mut Engine),
) -> Result<ReplayOut, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut states: Vec<(u64, Value)> = vec![];
    let mut journal = String::new();
    let mut ss = None;
    let mut ss_procs = std::collections::BTreeMap::new();
    let mut health: HashMap<String, bool> = HashMap::new();
    let mut bodies: HashMap<u16, Value> = HashMap::new();
    let mut access: Vec<(String, String)> = vec![];
    let mut units_cap: HashMap<String, crate::probe::UnitInfo> = HashMap::new();
    let mut runs_cap: HashMap<String, crate::probe::LastRun> = HashMap::new();
    let mut http_cap: HashMap<u16, u16> = HashMap::new();
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
        } else if n == "systemctl-show.txt" {
            for u in crate::probe::parse_show(&read()?) {
                units_cap.insert(u.id.clone(), u);
            }
        } else if n == "systemctl-runs.txt" {
            for r in crate::probe::parse_last_runs(&read()?) {
                runs_cap.insert(r.unit.clone(), r);
            }
        } else if n == "http.txt" {
            for l in read()?.lines() {
                let mut it = l.split_whitespace();
                if let (Some(Some(p)), Some(Some(c))) = (
                    it.next().map(|x| x.parse::<u16>().ok()),
                    it.next().map(|x| x.parse::<u16>().ok()),
                ) {
                    http_cap.insert(p, c);
                }
            }
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
    let interval = e.cfg.interval_s.max(1);
    setup(&mut e);
    let units: Vec<&'static str> = e.comps.iter().filter_map(|c| c.spec.unit).collect();
    let mut hits: Vec<(String, Option<String>, Hit)> = vec![];
    for (name, text) in &access {
        for l in text.lines() {
            if let Some(h) = parse_line(l, states[0].0) {
                hits.push((name.clone(), None, h));
            }
        }
    }
    // (cycle time, snapshot, journal window) for every cycle, real then stretched.
    let mut plan: Vec<(u64, Value, Vec<JLine>)> = vec![];
    let mut prev = 0u64;
    for (k, (t, v)) in states.iter().enumerate() {
        let win: Vec<JLine> = lines
            .iter()
            .filter(|l| (k == 0 || l.t > prev) && l.t <= *t)
            .cloned()
            .collect();
        plan.push((*t, v.clone(), win));
        prev = *t;
    }
    if stretch > 0 {
        let first = &states[0];
        let last = &states[states.len() - 1];
        let tail: Vec<JLine> = lines
            .iter()
            .filter(|l| l.t + interval > last.0 && l.t <= last.0)
            .cloned()
            .collect();
        for k in 1..=stretch as u64 {
            let shift = k * interval;
            let win = tail
                .iter()
                .map(|l| JLine {
                    t: l.t + shift,
                    ..l.clone()
                })
                .collect();
            plan.push((last.0 + shift, stretch_state(first, last, shift), win));
        }
        notes.push(format!(
            "stretched {stretch} cycle(s) of {interval} s past the capture (last window {} journal line(s) repeated)",
            tail.len()
        ));
    }
    for (k, (t, v, win)) in plan.iter().enumerate() {
        let (t, win) = (t, win.clone());
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
        for cs in e.comps.iter_mut() {
            if col.samples.contains_key(cs.spec.name) {
                continue;
            }
            match missing_sample(&cs.spec, *t, &units_cap, &runs_cap, &http_cap) {
                Some(s) => {
                    col.samples.insert(cs.spec.name.to_string(), s);
                }
                None => {
                    cs.status = Status::Absent;
                    cs.reasons = vec!["absent: not in this capture".into()];
                }
            }
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
    }
    Ok(ReplayOut {
        cycles: plan.len(),
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

#[cfg(test)]
mod t23_tests {
    use super::*;

    #[test]
    fn capture_files_fill_components_the_snapshot_lacks() {
        let dir = std::env::temp_dir().join(format!("nerve-replay-t23-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("state-1.json"),
            json!({"ts": "2026-10-21T18:00:00+02:00", "components": [], "probe_errors": []})
                .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("systemctl-show.txt"), "Id=tanga-proto.service\nLoadState=loaded\nActiveState=active\nSubState=running\nResult=success\nUnitFileState=enabled\nMainPID=7\nNRestarts=0\n\nId=galion-backup.timer\nLoadState=loaded\nActiveState=active\nSubState=waiting\nResult=success\nUnitFileState=enabled\nMainPID=0\nNRestarts=0\n").unwrap();
        std::fs::write(dir.join("systemctl-runs.txt"), "Id=galion-backup.service\nLoadState=loaded\nActiveState=inactive\nResult=success\nExecMainStatus=0\nExecMainExitTimestamp=@1792576800\n").unwrap();
        std::fs::write(dir.join("http.txt"), "8671 200\n19120 000\n").unwrap();
        let d = dir.display().to_string();
        let cfg = Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| d.clone())).unwrap();
        let out = replay_dir(&dir, cfg).unwrap();
        let st = |n: &str| {
            let c = out.engine.comps.iter().find(|c| c.spec.name == n).unwrap();
            (c.status, c.reasons.clone())
        };
        assert_eq!(
            st("tanga-proto").0,
            Status::Healthy,
            "{:?}",
            st("tanga-proto")
        );
        assert_eq!(
            st("galion-backup").0,
            Status::Healthy,
            "{:?}",
            st("galion-backup")
        );
        let q = st("openmind-queue");
        assert_eq!(q.0, Status::Degrading, "{q:?}");
        assert!(q.1[0].contains("captured 000"), "{q:?}");
        assert_eq!(st("galion-nerve-shadow").0, Status::Absent);
        assert_eq!(
            st("galion-nerve-shadow").1,
            vec!["absent: not in this capture".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
