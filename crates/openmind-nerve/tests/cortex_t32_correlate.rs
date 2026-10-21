// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex plan T3.2: cross-sensor correlation causes (no `GALION_REAL`
//! needed: replay and in-process engine only, nothing on the host is read
//! except a scratch auth log in the temp dir).
//!
//! 1. The golden fixture (`fixtures/cortex-replay`): every incident carries
//!    a `correlation` section; `edge` holds the dead-origin finding (sensor
//!    `edge`) and the cloudflared error-rate finding (`components`); the
//!    pre-T3.2 report is unchanged apart from that section and the scanner
//!    member floor (21 -> 16, the only score change).
//! 2. The real engine, live mode, fed an ssh credential flood (journal
//!    lines, TEST-NET addresses), a growing scratch auth log and a host at
//!    92 % disk: one `ssh-flood` incident holds the flood, the auth-log
//!    bleed and the host disk signal (rule `ssh-disk-bleed`); without the
//!    bleed, the disk stays its own incident; every finding is scored once.

use openmind_nerve::collect::Collected;
use openmind_nerve::config::Config;
use openmind_nerve::engine::Engine;
use openmind_nerve::probe::JLine;
use openmind_nerve::replay::replay_dir;
use openmind_nerve::sensors::{sshd::SshAuth, Sensors};
use openmind_nerve::state::Sample;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[allow(dead_code)] // shared golden helpers; each test uses a subset
mod support;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-t32-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn incident<'a>(r: &'a Value, cause: &str) -> &'a Value {
    r["incidents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["cause"] == cause)
        .unwrap_or_else(|| panic!("no {cause} incident: {}", r["incidents"]))
}

fn deduction<'a>(r: &'a Value, what: &str) -> &'a Value {
    r["deductions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["what"].as_str().unwrap().contains(what))
        .unwrap_or_else(|| panic!("no deduction {what}: {}", r["deductions"]))
}

#[test]
fn correlation_on_the_golden_fixture() {
    let d = tmp("fx");
    let dd = d.display().to_string();
    let cfg = Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| dd.clone())).unwrap();
    let out = replay_dir(&fixture().join("cortex-replay"), cfg).unwrap();
    let sh = out.engine.shared.lock().unwrap();
    let r = &sh.report;
    assert!(r["incidents"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["correlation"]["rules"][0] == "same-cause"));
    let edge = &incident(r, "edge")["correlation"];
    assert_eq!(edge["sensors"], json!(["components", "edge"]));
    assert_eq!(
        edge["findings"],
        json!([
            {"finding": "bleeding:edge-dead-origins", "source": "edge", "severity": "error"},
            {"finding": "bleeding:error-rate-cloudflared", "source": "components", "severity": "warn"}
        ])
    );
    assert_eq!(
        incident(r, "edge")["severity"],
        "error",
        "severity from signals"
    );
    // The scanner incident is a warn, its finding an error: member floor.
    let scan = incident(r, "scan:console");
    assert_eq!(scan["severity"], "warn");
    let sd = deduction(r, "scanner/exploit probes against console");
    assert_eq!(sd["points"], 10);
    assert!(sd["why"].as_str().unwrap().contains(
        "member floor: vulnerability:scanner-probes-console (error) costs 10 alone, so the incident costs 10 not 5"
    ));
    assert_eq!(
        (r["score"].as_u64(), r["grade"].as_str()),
        (Some(16), Some("F"))
    );
    // Every other deduction is the pre-T3.2 golden's.
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture().join("cortex-replay.expected.json")).unwrap(),
    )
    .unwrap();
    let pts = |v: &Value| -> Vec<u64> {
        v["deductions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["points"].as_u64().unwrap())
            .collect()
    };
    let (mut now, mut then) = (pts(r), pts(&golden));
    now.sort_unstable();
    then.sort_unstable();
    assert_eq!(then.iter().sum::<u64>() + 5, now.iter().sum::<u64>());
    assert_eq!(now.len(), then.len(), "no new or lost deduction");
    // Covered findings: each charged exactly once, through its incident.
    let covered: Vec<&str> = out
        .engine
        .score
        .as_ref()
        .unwrap()
        .covered
        .iter()
        .map(|c| c.finding.as_str())
        .collect();
    for k in [
        "bleeding:edge-dead-origins",
        "bleeding:error-rate-cloudflared",
        "vulnerability:scanner-probes-console",
    ] {
        assert!(covered.contains(&k), "{k} {covered:?}");
    }
    // Markdown: one `- correlated:` line per incident with members.
    let md = &sh.report_md;
    assert!(md.contains("- correlated: rules same-cause; sensors components, edge; findings bleeding:edge-dead-origins (error), bleeding:error-rate-cloudflared (warn); worst finding bleeding:edge-dead-origins (error)\n"), "{md}");
    assert_eq!(
        md.matches("\n- correlated: ").count(),
        4,
        "ddos:tuner has no member"
    );
    drop(sh);
    let _ = std::fs::remove_dir_all(&d);
}

const T0: u64 = 1_792_520_000;

fn engine(dir: &Path, auth: &Path) -> Engine {
    let comps = dir.join("components.json");
    std::fs::write(&comps, "[]").unwrap();
    let (d, c) = (
        dir.join("data").display().to_string(),
        comps.display().to_string(),
    );
    let cfg = Config::from_env(&|k| match k {
        "NERVE_DATA_DIR" => Some(d.clone()),
        "NERVE_COMPONENTS" => Some(c.clone()),
        _ => None,
    })
    .unwrap();
    let mut e = Engine::new(cfg).unwrap();
    e.reflexes_enabled = false;
    e.write_files = false;
    e.live = true; // the sshd sensor stats the scratch auth log
    let a = auth.display().to_string();
    let mut s = Sensors::empty();
    s.add_opt(SshAuth::from_env(&|k| match k {
        "NERVE_SSHD" => Some("1".into()),
        "NERVE_SSHD_AUTH_LOG" => Some(a.clone()),
        _ => None,
    }));
    e.sensors = s;
    e
}

/// One cycle: host at `used_pct` disk, `fails` failed passwords from 60
/// TEST-NET sources.
fn cycle(e: &mut Engine, t: u64, used_pct: u64, fails: usize) -> Value {
    let mut samples = HashMap::new();
    samples.insert(
        "lxc-host".to_string(),
        Sample {
            t,
            present: true,
            disk_total_kb: Some(100_000_000),
            disk_avail_kb: Some(100_000_000 - used_pct * 1_000_000),
            ..Default::default()
        },
    );
    samples.insert(
        "galion-nerve".to_string(),
        Sample {
            t,
            present: true,
            ..Default::default()
        },
    );
    let lines = (0..fails)
        .map(|k| JLine {
            t,
            unit: "ssh.service".into(),
            prio: 6,
            msg: format!(
                "Failed password for invalid user admin from 192.0.2.{} port 4{:04} ssh2", // repo-check: allow (TEST-NET-1 fixture address, never dialled)
                k % 60,
                k
            ),
        })
        .collect();
    let col = Collected {
        samples,
        probe_errors: vec![],
        lines,
        listeners: Some(vec![]),
        skipped: vec![],
    };
    e.cycle_with(col, vec![], t, Instant::now());
    e.shared.lock().unwrap().report.clone()
}

fn grow(p: &Path, bytes: usize) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
    f.write_all(&vec![b'x'; bytes]).unwrap();
}

#[test]
fn ssh_flood_auth_log_bleed_and_disk_are_one_incident() {
    let d = tmp("ssh");
    let auth = d.join("auth.log");
    std::fs::write(&auth, b"").unwrap();
    let mut e = engine(&d, &auth);
    // Cycle 1: disk 92 %, no flood, no growth: the disk is its own incident.
    let r = cycle(&mut e, T0, 92, 0);
    let host = incident(&r, "lxc-host");
    assert!(host["signals"][0]["msg"]
        .as_str()
        .unwrap()
        .starts_with("degrading: disk 92% used"));
    // Cycle 2: 400 failed passwords from 60 sources and +1.1 MiB auth log.
    grow(&auth, 1_153_434);
    let r = cycle(&mut e, T0 + 30, 92, 400);
    let ssh = incident(&r, "ssh-flood");
    assert_eq!(ssh["severity"], "critical");
    assert_eq!(ssh["components"], json!(["host", "lxc-host"]));
    let c = &ssh["correlation"];
    assert_eq!(c["rules"], json!(["same-cause", "ssh-disk-bleed"]));
    assert_eq!(c["sensors"], json!(["auth.log"]));
    assert_eq!(
        c["folded"],
        json!([{"from": "lxc-host", "component": "lxc-host", "rule": "ssh-disk-bleed"}])
    );
    let keys: Vec<&str> = c["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["finding"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "bleeding:sshd-auth-log-growth",
            "ddos:sshd-credential-flood"
        ]
    );
    assert!(ssh["why"]
        .as_str()
        .unwrap()
        .contains("disk signal(s) of lxc-host folded in"));
    assert!(ssh["title"]
        .as_str()
        .unwrap()
        .starts_with("ssh flood: ssh credential flood: 400 failed password(s) from 60 source(s)"));
    // The host incident goes quiet (resolves after 2 quiet cycles) instead
    // of costing a second deduction.
    // The host incident is still open (quiet), its signal now in ssh-flood.
    assert_eq!(incident(&r, "lxc-host")["cycles"], 1);
    let sd = deduction(&r, "critical incident: ssh flood");
    assert_eq!(sd["points"], 28, "25 + 3 for the folded host component");
    let ded = r["deductions"].as_array().unwrap();
    assert_eq!(ded.len(), 2, "{ded:?}");
    // Cycles 3-4: flood and bleed continue; the old host incident resolves.
    grow(&auth, 300_000);
    let _ = cycle(&mut e, T0 + 60, 92, 400);
    grow(&auth, 300_000);
    let r = cycle(&mut e, T0 + 90, 92, 400);
    let causes: Vec<&str> = r["incidents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["cause"].as_str().unwrap())
        .collect();
    assert_eq!(causes, ["ssh-flood"], "one incident left");
    assert_eq!(
        r["deductions"].as_array().unwrap().len(),
        1,
        "{}",
        r["deductions"]
    );
    assert_eq!(deduction(&r, "ssh flood")["points"], 28);
    assert_eq!(r["score"], 72);
    assert_eq!(r["pass"], false, "a critical incident never passes");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn without_the_bleed_the_disk_is_its_own_incident() {
    let d = tmp("nobleed");
    let auth = d.join("auth.log");
    std::fs::write(&auth, b"").unwrap();
    let mut e = engine(&d, &auth);
    let _ = cycle(&mut e, T0, 92, 0);
    let r = cycle(&mut e, T0 + 30, 92, 400);
    let ssh = incident(&r, "ssh-flood");
    assert_eq!(ssh["components"], json!(["host"]));
    assert_eq!(ssh["correlation"]["folded"], json!([]));
    assert_eq!(incident(&r, "lxc-host")["cycles"], 2);
    // Two stories, two deductions: 25 (flood) + the host's own.
    assert_eq!(
        r["deductions"].as_array().unwrap().len(),
        2,
        "{}",
        r["deductions"]
    );
    let _ = std::fs::remove_dir_all(&d);
}
