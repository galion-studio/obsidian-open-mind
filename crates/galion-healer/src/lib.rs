// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-healer: Rust port of galion-light `guardian/desk_heal.py` (cases,
//! follow-ups, nerve), `scripts/heal-dry.py` (plan), `scripts/heal-apply.py`
//! (fixture JSON repair) and the safe half of `revive.py` (stale gateway
//! lock). All repairs are allow-listed, dry-run by default and logged.

pub mod actions;
pub mod cases;
pub mod config;

use config::File;
use openmind_survival::{http_get, verdict, Runner};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::Duration;

pub fn write_atomic(path: &Path, body: &str) -> Result<(), String> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn append(path: &Path, line: &str) -> Result<(), String> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(format!("{}\n", line.trim()).as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn load<T: serde::de::DeserializeOwned + Default>(p: &Path) -> T {
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// desk_heal.py `probe()` for one configured probe: (ok, detail).
pub fn probe_one(p: &config::Probe) -> (bool, String) {
    let addr = match (p.host.as_str(), p.port).to_socket_addrs() {
        Ok(mut a) => match a.next() {
            Some(a) => a,
            None => return (false, "resolve: no address".into()),
        },
        Err(e) => return (false, format!("resolve: {e}")),
    };
    let out = http_get(addr, &p.path, Duration::from_millis(2500));
    verdict(&out.result, &p.ok_codes, p.loading_ok)
}

pub struct Deps<'a> {
    pub runner: &'a dyn Runner,
    pub proc_root: &'a Path,
    pub prober: &'a mut dyn FnMut(&config::Probe) -> (bool, String),
    pub now_unix: f64,
    pub stamp: String,
}

/// One 30 s cycle: probes, cases, files, then allow-listed actions.
/// Returns the pulse JSON.
pub fn run_cycle(f: &File, d: &mut Deps) -> Result<serde_json::Value, String> {
    let s = &f.healer;
    let dir = &s.state_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let results: Vec<(String, bool, String)> = f
        .probe
        .iter()
        .map(|p| {
            let (ok, detail) = (d.prober)(p);
            (p.name.clone(), ok, detail)
        })
        .collect();
    let mut open: cases::Open = load(&dir.join("open.json"));
    let probes = &f.probe;
    let prober = &mut *d.prober;
    let mut recheck = |name: &str| match probes.iter().find(|p| p.name == name) {
        Some(p) => prober(p),
        None => (false, "unknown probe".into()),
    };
    let out = cases::cycle(
        &mut open,
        &results,
        &mut recheck,
        d.now_unix,
        &d.stamp,
        s.follow_s,
    );
    for c in &out.desk {
        let row = serde_json::to_string(c).map_err(|e| e.to_string())?;
        append(&dir.join("desk.jsonl"), &row)?;
        let pretty = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
        write_atomic(
            &dir.join("inbox").join(format!("{}.json", c.id)),
            &(pretty + "\n"),
        )?;
    }
    for l in &out.nerve {
        append(&dir.join("nerve.jsonl"), l)?;
    }
    write_atomic(
        &dir.join("open.json"),
        &(serde_json::to_string_pretty(&open).map_err(|e| e.to_string())? + "\n"),
    )?;

    // streaks for restart rules
    let mut streak: BTreeMap<String, u32> = load(&dir.join("streak.json"));
    for (name, ok, _) in &results {
        let n = if *ok {
            0
        } else {
            streak.get(name).copied().unwrap_or(0) + 1
        };
        streak.insert(name.clone(), n);
    }
    write_atomic(
        &dir.join("streak.json"),
        &serde_json::to_string(&streak).map_err(|e| e.to_string())?,
    )?;

    let log = dir.join("actions.jsonl");
    let mut acted = Vec::new();
    if !s.hermes_home.is_empty() && out.down.contains(&s.gateway_probe) {
        let r = actions::clear_stale_gateway_lock(
            Path::new(&s.hermes_home),
            false,
            d.proc_root,
            s.apply,
        );
        if !r.starts_with("skip") {
            append(
                &log,
                &actions::log_row("clear-stale-gateway-lock", &s.hermes_home, s.apply, &r),
            )?;
            acted.push("clear-stale-gateway-lock");
        }
    }
    let mut last: BTreeMap<String, f64> = load(&dir.join("restarts.json"));
    for rule in &f.restart {
        let n = streak.get(&rule.probe).copied().unwrap_or(0);
        if n < rule.after_cycles {
            continue;
        }
        let since = d.now_unix - last.get(&rule.unit).copied().unwrap_or(0.0);
        if since < s.restart_min_gap_s as f64 {
            continue;
        }
        let r = actions::restart_unit(
            &rule.unit,
            &s.restart_backend,
            &s.supervisor_dir,
            d.runner,
            s.apply,
        );
        append(
            &log,
            &actions::log_row("restart-unit", &rule.unit, s.apply, &r),
        )?;
        last.insert(rule.unit.clone(), d.now_unix);
        acted.push("restart-unit");
    }
    write_atomic(
        &dir.join("restarts.json"),
        &serde_json::to_string(&last).map_err(|e| e.to_string())?,
    )?;

    let pulse = json!({"at": d.stamp, "down": out.down, "api_calls": out.api_calls, "actions": acted, "mode": actions::mode(s.apply)});
    write_atomic(
        &dir.join("pulse.json"),
        &(serde_json::to_string_pretty(&pulse).map_err(|e| e.to_string())? + "\n"),
    )?;
    Ok(pulse)
}
