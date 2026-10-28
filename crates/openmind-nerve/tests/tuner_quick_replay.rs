// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 420 (nerve-tuner-quick-diag): the live nerve 9baa450 scored 68 D with
//! `tuner-quick` "failing: unit/endpoint not found". Root cause: the
//! transient `tuner-quick.service` (a hand-made `cloudflared tunnel --url`
//! quick tunnel, see docs/cutover/tuner.md) did not survive the VPS shutdown
//! of 2026-10-22; systemd reports LoadState=not-found ever since. Two nerve
//! defects made that look worse than it is:
//!
//! 1. `state.rs` ignored `critical` for a missing unit (always "failing");
//!    tuner-quick is non-critical, so it now reads "degrading".
//! 2. replay fed a not-found component back as present-but-inactive.
//!
//! The score cost (+2 on the edge incident: 15 + 2 per extra component) only
//! goes away with the staged table `components-live-d420.json`
//! (tuner-quick `expected: false` -> absent), owner decision D-420-1.
//!
//! Hermetic legs replay the scrubbed live `/state` fixture
//! (2026-10-28 20:10 +01:00, `last_error` nulled) through the real binary.
//! `GALION_REAL=1` + `NERVE_TQ_SSH=<ssh host>` adds a leg on a fresh
//! read-only `GET /state` of the live nerve (nothing written on the VPS).

use openmind_nerve::config::parse_specs;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn live_table() -> PathBuf {
    root().join("../../deploy/staged/nerve/components-live.json")
}

fn d420_table() -> PathBuf {
    root().join("../../deploy/staged/nerve/components-live-d420.json")
}

fn replay(dir: &Path, table: &Path) -> Value {
    let data = tempdir("data");
    let out = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
        .arg("replay")
        .arg(dir)
        .arg("--state")
        .env("NERVE_COMPONENTS", table)
        .env("NERVE_DATA_DIR", &data)
        .env("NERVE_REFLEX", "0")
        .output()
        .expect("run replay");
    let _ = std::fs::remove_dir_all(&data);
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "replay JSON: {e}; stderr {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn tempdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "nerve-tq-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn comp<'a>(s: &'a Value, name: &str) -> &'a Value {
    s["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == name))
        .unwrap_or_else(|| panic!("no component {name}"))
}

fn incident_with<'a>(s: &'a Value, name: &str) -> Option<&'a Value> {
    s["incidents"]["open"].as_array().and_then(|a| {
        a.iter().find(|i| {
            i["components"]
                .as_array()
                .is_some_and(|c| c.iter().any(|x| x == name))
        })
    })
}

/// The two replays every leg checks.
fn check(dir: &Path) {
    let base = replay(dir, &live_table());
    let q = comp(&base, "tuner-quick");
    assert_eq!(q["status"], "degrading", "{q}");
    assert_eq!(q["reasons"][0], "degrading: unit/endpoint not found", "{q}");
    let inc = incident_with(&base, "tuner-quick").expect("tuner-quick in an incident");
    assert_eq!(inc["cause"], "edge", "{inc}");
    let fixed = replay(dir, &d420_table());
    let q = comp(&fixed, "tuner-quick");
    assert_eq!(q["status"], "absent", "{q}");
    assert!(incident_with(&fixed, "tuner-quick").is_none());
    let (a, b) = (
        base["self_eval"]["score"].as_u64().unwrap(),
        fixed["self_eval"]["score"].as_u64().unwrap(),
    );
    // Replay has no journal, so cloudflared's closed-port signal is absent and
    // edge is tuner-quick alone: a warn incident (5) that disappears.
    assert!(b > a, "d420 table must score higher: {a} -> {b}");
    eprintln!(
        "replay {}: live table {a} {} -> d420 table {b} {}",
        dir.display(),
        base["self_eval"]["grade"],
        fixed["self_eval"]["grade"]
    );
}

#[test]
fn d420_table_changes_only_tuner_quick() {
    let read = |p: PathBuf| std::fs::read_to_string(&p).unwrap();
    let live = parse_specs(&read(live_table())).unwrap();
    let d420 = parse_specs(&read(d420_table())).unwrap();
    assert_eq!(live.len(), d420.len());
    for (l, d) in live.iter().zip(&d420) {
        assert_eq!(l.name, d.name);
        if l.name == "tuner-quick" {
            assert!(l.expected && !d.expected);
            assert!(!d.critical);
            assert_eq!(d.restarts_per_hour, 0);
            assert!(d.note.starts_with("RETIRED"));
        } else {
            assert_eq!(format!("{l:?}"), format!("{d:?}"), "{}", l.name);
        }
    }
}

#[test]
fn replay_of_live_state_fixture() {
    check(&root().join("tests/fixtures/tuner-quick"));
}

#[test]
fn real_live_state_tuner_quick() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 and NERVE_TQ_SSH");
        return;
    }
    let host = std::env::var("NERVE_TQ_SSH").expect("NERVE_TQ_SSH (ssh host alias)");
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &host])
        .arg("curl -s -m 10 http://127.0.0.1:8670/state")
        .output()
        .expect("ssh");
    let mut v: Value = serde_json::from_slice(&out.stdout).expect("live /state is JSON");
    let q = comp(&v, "tuner-quick").clone();
    eprintln!(
        "live {} cycle {}: tuner-quick {} {} unit_state {} | score {} {}",
        v["ts"],
        v["cycle"],
        q["status"],
        q["reasons"],
        q["unit_state"],
        v["self_eval"]["score"],
        v["self_eval"]["grade"]
    );
    let not_found = q["reasons"].as_array().is_some_and(|r| {
        r.iter().any(|x| {
            x.as_str()
                .is_some_and(|s| s.ends_with("unit/endpoint not found"))
        })
    });
    assert!(not_found, "live tuner-quick is no longer not-found: {q}");
    // Live score arithmetic: the edge deduction carries +2 for tuner-quick.
    let inc = incident_with(&v, "tuner-quick").expect("live edge incident holds tuner-quick");
    let n = inc["components"].as_array().unwrap().len() as u64;
    if inc["severity"] == "error" {
        let edge = v["self_eval"]["deductions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| {
                d["what"]
                    .as_str()
                    .is_some_and(|w| w.contains("incident: edge"))
            })
            .expect("edge deduction");
        assert_eq!(edge["points"].as_u64().unwrap(), 15 + (2 * (n - 1)).min(6));
        let without = 15 + (2 * (n - 2)).min(6);
        eprintln!(
            "live edge deduction {} with {n} component(s); without tuner-quick {without}: score {} -> {}",
            edge["points"],
            v["self_eval"]["score"],
            v["self_eval"]["score"].as_u64().unwrap() + edge["points"].as_u64().unwrap() - without
        );
    }
    // Scrub (no error lines, alerts, logs or cortex detail) and replay.
    let comps: Vec<Value> = v["components"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .map(|c| {
            c["last_error"] = Value::Null;
            c.clone()
        })
        .collect();
    let snap = serde_json::json!({"ts": v["ts"], "cycle": v["cycle"], "mode": v["mode"],
        "service": v["service"], "components": comps});
    let dir = tempdir("live");
    std::fs::write(dir.join("state-live.json"), snap.to_string()).unwrap();
    check(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
}
