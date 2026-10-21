// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the replay regression corpus (cortex plan T5.3),
//! gated: `GALION_REAL=1` (run with `--release` for the release binary).
//! Slot 193: box only, no ports needed (replay opens no socket); ports
//! 20630-20639 stay reserved.
//!
//! 1. The real `openmind-nerve replay DIR --verdict` binary, with a clean
//!    environment plus the entry's own env, on every corpus capture present
//!    on this host: output equals the committed expected verdict (`result`),
//!    exit 0 (no flap); wall time per capture recorded.
//! 2. The binary on a capture where hermes-rs fails, recovers and fails
//!    again: exit 3 and the flap named; failing once: exit 0.
//! 3. `GALION_REAL_VPS=1`: a fresh read-only capture from the VPS over ssh
//!    (`NERVE_T53_SSH`, default `slavik-vps`): 3x `GET :8670/state` 30 s
//!    apart, `GET :8660/status.json`, `ss -ltn`, `journalctl -o json` 30 min
//!    (5 fields), into a temp dir on the box (deleted afterwards). Replayed
//!    with `--verdict`: no flap over 3 + 20 cycles, and every incident the
//!    replay opens is also open in the live nerve's own `GET /self-eval`
//!    taken right after the capture (live-only incidents are printed: pushed
//!    signals such as spend caps are not in a `/state` capture).

use openmind_nerve::corpus::{diff, load_manifest};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

fn real() -> bool {
    std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
}

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-t53-real-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn verdict(dir: &Path, env: &[(String, String)], data: &Path) -> (Output, Option<Value>) {
    let out = Command::new(BIN)
        .args(["replay", &dir.display().to_string(), "--verdict"])
        .env_clear()
        .env("NERVE_DATA_DIR", data)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .output()
        .expect("run openmind-nerve");
    let v = serde_json::from_slice(&out.stdout).ok();
    (out, v)
}

fn causes(v: &Value) -> BTreeSet<String> {
    v["incidents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["cause"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn real_binary_replays_the_corpus() {
    if !real() {
        eprintln!("GALION_REAL!=1: skipped");
        return;
    }
    let live_root = std::env::var("NERVE_CAPTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/nerve-capture"));
    let es = load_manifest(
        &here().join("replay-corpus/corpus.json"),
        &here().join("fixtures"),
        &live_root,
    )
    .unwrap();
    let data = scratch("data");
    let mut ran = 0;
    for e in &es {
        if !e.dir.is_dir() {
            eprintln!("{}: not on this host, skipped", e.name);
            continue;
        }
        let env: Vec<(String, String)> =
            e.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let t = Instant::now();
        let (out, got) = verdict(&e.dir, &env, &data);
        let ms = t.elapsed().as_millis();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}: {}",
            e.name,
            String::from_utf8_lossy(&out.stderr)
        );
        let got = got.unwrap_or_else(|| panic!("{}: no JSON on stdout", e.name));
        let want: Value = serde_json::from_str(
            &std::fs::read_to_string(
                here().join(format!("replay-corpus/expected/{}.json", e.name)),
            )
            .unwrap(),
        )
        .unwrap();
        let d = diff(&want["result"], &got);
        assert!(
            d.is_empty(),
            "{}: binary verdict drifts from expected: {d:#?}",
            e.name
        );
        eprintln!(
            "real {}: {} ms, final {} {} held {} {}, timeline[0..3] {:?}, flaps 0",
            e.name,
            ms,
            got["final"]["score"],
            got["final"]["grade"],
            got["held"]["score"],
            got["held"]["grade"],
            &got["timeline"].as_array().unwrap()[..3]
        );
        ran += 1;
    }
    assert!(ran >= 2);
    eprintln!(
        "real binary: {ran} of {} corpus captures replayed, all match",
        es.len()
    );
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn real_binary_flags_a_flapping_capture() {
    if !real() {
        eprintln!("GALION_REAL!=1: skipped");
        return;
    }
    let tpl: Value = serde_json::from_str(
        &std::fs::read_to_string(here().join("fixtures/cortex-replay/state-0.json")).unwrap(),
    )
    .unwrap();
    let data = scratch("flap-data");
    for (tag, failing, want) in [
        (
            "flap",
            (|k: usize| !(4..8).contains(&k)) as fn(usize) -> bool,
            3,
        ),
        ("once", (|k: usize| k < 4) as fn(usize) -> bool, 0),
    ] {
        let d = scratch(tag);
        for k in 0..12usize {
            let mut v = tpl.clone();
            let secs = 5 + 30 * k;
            v["ts"] = Value::from(format!(
                "2026-10-20T19:{:02}:{:02}+02:00",
                5 + secs / 60,
                secs % 60
            ));
            for c in v["components"].as_array_mut().unwrap() {
                if c["name"] == "hermes-rs" && failing(k) {
                    c["status"] = Value::from("failing");
                    c["unit_state"] = Value::from("failed");
                    c["http"] = Value::Null;
                }
            }
            std::fs::write(d.join(format!("state-{k:02}.json")), v.to_string()).unwrap();
        }
        let (out, got) = verdict(&d, &[], &data);
        let got = got.unwrap();
        eprintln!(
            "real {tag}: exit {:?}, flaps {}, stderr {}",
            out.status.code(),
            got["flaps"],
            String::from_utf8_lossy(&out.stderr).trim()
        );
        assert_eq!(out.status.code(), Some(want), "{tag}");
        if want == 3 {
            assert!(got["flaps"]
                .to_string()
                .contains("incident:hermes-rs=critical"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }
    let _ = std::fs::remove_dir_all(&data);
}

fn ssh(host: &str, cmd: &str) -> Vec<u8> {
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=15", host, cmd])
        .output()
        .expect("ssh");
    assert!(
        out.status.success(),
        "ssh {cmd}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

#[test]
fn real_vps_capture_replays_without_flaps() {
    if !real() || !std::env::var("GALION_REAL_VPS").is_ok_and(|v| v == "1") {
        eprintln!("GALION_REAL=1 GALION_REAL_VPS=1 not set: skipped");
        return;
    }
    let host = std::env::var("NERVE_T53_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let d = scratch("vps");
    // Read-only: GETs, ss, journalctl. Nothing is written on the VPS.
    for i in 1..=3 {
        let s = ssh(&host, "curl -s --max-time 10 http://127.0.0.1:8670/state");
        std::fs::write(d.join(format!("state-{i}.json")), s).unwrap();
        if i < 3 {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }
    let live: Value = serde_json::from_slice(&ssh(
        &host,
        "curl -s --max-time 10 http://127.0.0.1:8670/self-eval",
    ))
    .unwrap();
    std::fs::write(
        d.join("health_status.json"),
        ssh(
            &host,
            "curl -s --max-time 10 http://127.0.0.1:8660/status.json",
        ),
    )
    .unwrap();
    std::fs::write(d.join("ss.txt"), ssh(&host, "ss -ltn")).unwrap();
    std::fs::write(
        d.join("journal.jsonl"),
        ssh(&host, "journalctl -o json --output-fields=UNIT,_SYSTEMD_UNIT,MESSAGE,PRIORITY,__REALTIME_TIMESTAMP --since -30min --no-pager"),
    )
    .unwrap();
    let data = scratch("vps-data");
    let t = Instant::now();
    let (out, got) = verdict(&d, &[], &data);
    let ms = t.elapsed().as_millis();
    let got = got.unwrap_or_else(|| panic!("no verdict: {}", String::from_utf8_lossy(&out.stderr)));
    let replay_causes = causes(&got["final"]);
    let live_causes = causes(&live);
    eprintln!(
        "real VPS capture ({} ms): replay final {} {} held {} {} incidents {replay_causes:?} flaps {}; live nerve cycle {} {} {} incidents {live_causes:?}",
        ms,
        got["final"]["score"],
        got["final"]["grade"],
        got["held"]["score"],
        got["held"]["grade"],
        got["flaps"],
        live["cycle"],
        live["score"],
        live["grade"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "flap on the live capture: {}",
        got["flaps"]
    );
    let missing: Vec<&String> = replay_causes.difference(&live_causes).collect();
    assert!(
        missing.is_empty(),
        "replay opened incidents the live nerve does not have: {missing:?}"
    );
    let live_only: Vec<&String> = live_causes.difference(&replay_causes).collect();
    eprintln!("live-only incidents (not reproducible from a /state capture): {live_only:?}");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&data);
}
