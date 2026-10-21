// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the T3.1 threshold calibration (slot 207), gated:
//! `GALION_REAL=1` (run with `--release` for the release binary). Box only;
//! the binary opens no socket here (ports 20770-20779 stay reserved).
//!
//! 1. The real `openmind-nerve calibrate DIR...` on every live capture on
//!    this host: output equals the committed counts-only record
//!    (`tests/replay-corpus/calibration.json`) and holds no IPv4 address or
//!    public hostname; the tightened lines keep their headroom.
//! 2. `openmind-nerve replay tests/fixtures/cortex-t31-band --verdict` with
//!    `NERVE_SSHD=1`: the band fixture fires `sshd-invalid-user` (warn) and
//!    `memory-heavy-galion-desk` (info), exit 0 (no flap over 20 hold cycles).
//! 3. `replay --verdict` on every live capture: the `edge` incident (error)
//!    is still there, exit 0.
//! 4. `GALION_REAL_VPS=1`: a fresh read-only capture over ssh (`NERVE_T31_SSH`,
//!    default `slavik-vps`): `GET :8670/state` once and `journalctl -o json`
//!    30 min (5 fields) into a temp dir on the box (deleted afterwards), run
//!    through `calibrate`: same headroom rules on brand-new real windows;
//!    the live cortex's `GET :8674/health` and `/incidents` are printed
//!    (read-only).

use openmind_nerve::corpus::load_manifest;
use openmind_nerve::sensors::components::MEMORY_HEAVY_KIB;
use openmind_nerve::sensors::sshd::INVALID_USER_WARN;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

fn real() -> bool {
    std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
}

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn live_root() -> PathBuf {
    std::env::var("NERVE_CAPTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/nerve-capture"))
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-t31-real-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn calibrate(dirs: &[PathBuf]) -> (i32, String) {
    let out = Command::new(BIN)
        .arg("calibrate")
        .args(dirs)
        .env_clear()
        .output()
        .expect("run openmind-nerve calibrate");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn has_ipv4(s: &str) -> bool {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .any(|t| t.split('.').count() == 4 && t.split('.').all(|p| !p.is_empty() && p.len() <= 3))
}

/// Headroom rules on one calibrate report (same as tests/calibration.rs).
fn check_headroom(name: &str, r: &Value) {
    let inv = r["sshd_300s"]["invalid_user"]["max"].as_f64().unwrap();
    assert!(
        inv * 2.0 <= INVALID_USER_WARN as f64,
        "{name}: invalid_user {inv}"
    );
    for (comp, kb) in r["mem_kb_max"].as_object().unwrap() {
        let kb = kb.as_u64().unwrap();
        if comp == "console" {
            assert!(kb > MEMORY_HEAVY_KIB, "{name}: console {kb}");
        } else {
            assert!(kb * 4 <= MEMORY_HEAVY_KIB, "{name}: {comp} {kb}");
        }
    }
    let f10 = r["edge"]["floors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["min_rps"] == 10.0)
        .unwrap();
    assert!(f10["episodes"].as_u64().unwrap() <= 1, "{name}: {f10}");
}

fn live_dirs() -> Vec<PathBuf> {
    let es = load_manifest(
        &here().join("replay-corpus/corpus.json"),
        &here().join("fixtures"),
        &live_root(),
    )
    .unwrap();
    let mut v: Vec<PathBuf> = es
        .into_iter()
        .filter(|e| e.live && e.dir.is_dir())
        .map(|e| e.dir)
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn real_binary_calibrates_every_live_capture() {
    if !real() {
        eprintln!("GALION_REAL!=1: skipped");
        return;
    }
    let dirs = live_dirs();
    assert!(
        !dirs.is_empty(),
        "no live captures under {}",
        live_root().display()
    );
    let t = Instant::now();
    let (code, out) = calibrate(&dirs);
    let ms = t.elapsed().as_millis();
    assert_eq!(code, 0, "{out}");
    assert!(!has_ipv4(&out), "calibrate output holds an address");
    for bad in [".app", ".studio", ".com"] {
        assert!(
            !out.contains(bad),
            "calibrate output holds a hostname ({bad})"
        );
    }
    let got: Vec<Value> = serde_json::from_str(&out).unwrap();
    let want: Value = serde_json::from_str(
        &std::fs::read_to_string(here().join("replay-corpus/calibration.json")).unwrap(),
    )
    .unwrap();
    for r in &got {
        let name = r["name"].as_str().unwrap();
        assert_eq!(
            &want["captures"][name], r,
            "{name}: binary differs from the record"
        );
        check_headroom(name, r);
        eprintln!(
            "{name}: {} steps, invalid_user max {}, preauth_other max {}, edge rps max {}, floor10/8 episodes {}/{}, dead-origin failed/min min {}",
            r["steps"],
            r["sshd_300s"]["invalid_user"]["max"],
            r["sshd_300s"]["preauth_other"]["max"],
            r["edge"]["rps_60s"]["max"],
            r["edge"]["floors"][0]["episodes"],
            r["edge"]["floors"][1]["episodes"],
            r["dead_origin_failed_per_min_300s"]["min"]
        );
    }
    eprintln!("calibrate: {} captures in {ms} ms", got.len());
}

fn verdict(dir: &Path, env: &[(&str, &str)], data: &Path) -> (i32, Value) {
    let out = Command::new(BIN)
        .args(["replay", &dir.display().to_string(), "--verdict"])
        .env_clear()
        .env("NERVE_DATA_DIR", data)
        .envs(env.iter().copied())
        .output()
        .expect("run openmind-nerve replay");
    (
        out.status.code().unwrap_or(-1),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

#[test]
fn real_binary_fires_in_the_band_and_keeps_the_edge_incident() {
    if !real() {
        eprintln!("GALION_REAL!=1: skipped");
        return;
    }
    let data = scratch("verdict");
    let (code, v) = verdict(
        &here().join("fixtures/cortex-t31-band"),
        &[("NERVE_SSHD", "1")],
        &data,
    );
    assert_eq!(code, 0, "{v}");
    let f = v["final"]["findings"].to_string();
    assert!(f.contains("vulnerabilities:sshd-invalid-user warn"), "{f}");
    assert!(f.contains("bleeding:memory-heavy-galion-desk info"), "{f}");
    eprintln!(
        "band: final {} {}, timeline {}",
        v["final"]["score"], v["final"]["grade"], v["timeline"]
    );
    for d in live_dirs() {
        for env in [&[][..], &[("NERVE_SSHD", "1")][..]] {
            let (code, v) = verdict(&d, env, &data);
            assert_eq!(code, 0, "{}: flap {}", d.display(), v["flaps"]);
            let edge = v["final"]["incidents"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|i| i["cause"] == "edge")
                .cloned()
                .unwrap_or_else(|| panic!("{}: no edge incident", d.display()));
            assert_eq!(edge["severity"], "error");
            eprintln!(
                "{} sshd={}: final {} {} held {} {}, edge error",
                d.file_name().unwrap().to_string_lossy(),
                !env.is_empty(),
                v["final"]["score"],
                v["final"]["grade"],
                v["held"]["score"],
                v["held"]["grade"]
            );
        }
    }
    let _ = std::fs::remove_dir_all(&data);
}

fn ssh(host: &str, cmd: &str) -> String {
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, cmd])
        .output()
        .expect("ssh");
    assert!(
        out.status.success(),
        "ssh {cmd}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn real_fresh_vps_windows_keep_the_headroom() {
    if !real() || std::env::var("GALION_REAL_VPS").ok().as_deref() != Some("1") {
        eprintln!("GALION_REAL=1 GALION_REAL_VPS=1 needed: skipped");
        return;
    }
    let host = std::env::var("NERVE_T31_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let d = scratch("vps");
    std::fs::write(
        d.join("state-1.json"),
        ssh(&host, "curl -s -m 10 127.0.0.1:8670/state"),
    )
    .unwrap();
    std::fs::write(
        d.join("journal.jsonl"),
        ssh(&host, "journalctl -o json --output-fields=UNIT,_SYSTEMD_UNIT,MESSAGE,PRIORITY,__REALTIME_TIMESTAMP --since -30min --no-pager"),
    )
    .unwrap();
    let (code, out) = calibrate(std::slice::from_ref(&d));
    assert_eq!(code, 0, "{out}");
    assert!(!has_ipv4(&out));
    let r: Vec<Value> = serde_json::from_str(&out).unwrap();
    check_headroom("fresh-vps", &r[0]);
    eprintln!(
        "fresh VPS window: {} steps, ssh lines {}, invalid_user max {}, preauth_other max {}, edge rps max {}, floors {}",
        r[0]["steps"],
        r[0]["ssh_lines"],
        r[0]["sshd_300s"]["invalid_user"]["max"],
        r[0]["sshd_300s"]["preauth_other"]["max"],
        r[0]["edge"]["rps_60s"]["max"],
        r[0]["edge"]["floors"]
    );
    let health = ssh(&host, "curl -s -m 5 127.0.0.1:8674/health");
    let inc = ssh(&host, "curl -s -m 5 127.0.0.1:8674/incidents");
    let inc: Value = serde_json::from_str(&inc).unwrap_or(Value::Null);
    let open: Vec<String> = inc["open"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            format!(
                "{} {}",
                i["cause"].as_str().unwrap_or("?"),
                i["severity"].as_str().unwrap_or("?")
            )
        })
        .collect();
    eprintln!("live cortex :8674 /health {}", health.trim());
    eprintln!("live cortex open incidents: {open:?}");
    let _ = std::fs::remove_dir_all(&d);
}
