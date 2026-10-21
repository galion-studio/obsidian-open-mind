// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Threshold calibration guard (cortex plan T3.1, docs/cortex/calibration.md).
//! No `GALION_REAL` needed: it reads the saved captures only.
//!
//! - Tighten only: every calibrated line in `calibrate::CALIBRATED` is at or
//!   under its design value (unit test in `src/calibrate.rs`).
//! - Every real 300 s window of every live capture keeps 2x headroom under
//!   the tightened sshd `invalid_user` line; every service except the console
//!   stays under a quarter of the memory-heavy line, and the console stays
//!   over it (still flagged).
//! - The edge-rate floor kept at 10 req/s gives at most one hot episode per
//!   capture, and at least one real capture flaps (2+ episodes) at 8 req/s:
//!   the measured reason it was not tightened.
//! - Every live capture and the golden fixture still flag the `edge` incident.
//! - The measured numbers (counts only) match
//!   `tests/replay-corpus/calibration.json`; `CALIBRATION_BLESS=1` rewrites it.
//!
//! Env: `NERVE_CAPTURE_ROOT` (default `/workspace/swarm/nerve-capture`);
//! without it (CI) the live parts are skipped and only fixtures are checked.

use openmind_nerve::calibrate::{calibrate_dir, Report, CALIBRATED};
use openmind_nerve::corpus::{load_manifest, run_entry, Entry};
use openmind_nerve::sensors::components::MEMORY_HEAVY_KIB;
use openmind_nerve::sensors::sshd::INVALID_USER_WARN;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn live_root() -> PathBuf {
    std::env::var("NERVE_CAPTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/nerve-capture"))
}

fn entries() -> Vec<Entry> {
    load_manifest(
        &here().join("replay-corpus/corpus.json"),
        &here().join("fixtures"),
        &live_root(),
    )
    .unwrap()
}

/// One report per distinct live capture dir that is present on this host.
fn live_reports() -> BTreeMap<String, Report> {
    let mut out = BTreeMap::new();
    for e in entries().into_iter().filter(|e| e.live && e.dir.is_dir()) {
        let name = e.source.trim_start_matches("live:").to_string();
        if !out.contains_key(&name) {
            out.insert(name.clone(), calibrate_dir(&name, &e.dir).unwrap());
        }
    }
    out
}

#[test]
fn tightened_lines_keep_headroom_on_every_real_window() {
    let rs = live_reports();
    if rs.is_empty() {
        eprintln!("no live captures under {}: skipped", live_root().display());
        return;
    }
    let (mut windows, mut ssh_windows) = (0usize, 0usize);
    let mut worst_inv = (0.0f64, String::new());
    let mut worst_other_mem = (0u64, String::new());
    for (name, r) in &rs {
        windows += r.steps;
        let inv = r.sshd["invalid_user"];
        if r.ssh_lines > 0 {
            ssh_windows += inv.n;
        }
        if inv.max > worst_inv.0 {
            worst_inv = (inv.max, name.clone());
        }
        assert!(
            inv.max * 2.0 <= INVALID_USER_WARN as f64,
            "{name}: real 300 s window with {} invalid users leaves < 2x headroom under {INVALID_USER_WARN}",
            inv.max
        );
        for (comp, kb) in &r.mem_kb {
            if comp == "console" {
                assert!(
                    *kb > MEMORY_HEAVY_KIB,
                    "{name}: console {kb} KiB no longer memory-heavy"
                );
            } else {
                assert!(
                    kb * 4 <= MEMORY_HEAVY_KIB,
                    "{name}: {comp} {kb} KiB is within 4x of the memory-heavy line"
                );
                if *kb > worst_other_mem.0 {
                    worst_other_mem = (*kb, format!("{name}/{comp}"));
                }
            }
        }
    }
    eprintln!(
        "{} live captures, {windows} steps, {ssh_windows} sshd 300 s windows; worst invalid_user {} ({}), worst non-console service {} KiB ({})",
        rs.len(),
        worst_inv.0,
        worst_inv.1,
        worst_other_mem.0,
        worst_other_mem.1
    );
    for c in CALIBRATED {
        eprintln!("{}: {} -> {} ({})", c.name, c.original, c.current, c.basis);
    }
}

#[test]
fn kept_edge_floor_does_not_flap_and_a_tighter_one_does() {
    let rs = live_reports();
    if rs.is_empty() {
        eprintln!("no live captures: skipped");
        return;
    }
    let mut flaps_at_8 = vec![];
    for (name, r) in &rs {
        let at = |f: f64| r.edge_floors.iter().find(|x| x.floor == f).unwrap();
        assert!(
            at(10.0).episodes <= 1,
            "{name}: {} hot episodes at the kept 10 req/s floor",
            at(10.0).episodes
        );
        if at(8.0).episodes >= 2 {
            flaps_at_8.push(name.clone());
        }
    }
    eprintln!("captures that flap with an 8 req/s floor: {flaps_at_8:?}");
    assert!(
        !flaps_at_8.is_empty(),
        "no capture flaps at 8 req/s any more: re-run T3.1, the floor may be tightened"
    );
}

#[test]
fn live_captures_and_golden_still_flag_the_edge_incident() {
    let data = std::env::temp_dir().join(format!("nerve-calib-edge-{}", std::process::id()));
    let mut checked = vec![];
    for e in entries() {
        if e.live && !e.dir.is_dir() {
            continue;
        }
        if !(e.live || e.name == "fixture-cortex-replay") {
            continue;
        }
        let v = run_entry(&e, &data).unwrap();
        let inc = &v["result"]["final"]["incidents"];
        let edge = inc
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| i["cause"] == "edge");
        let edge = edge.unwrap_or_else(|| panic!("{}: no edge incident in {inc}", e.name));
        assert_eq!(edge["severity"], "error", "{}: {edge}", e.name);
        checked.push(e.name.clone());
    }
    eprintln!("edge incident (error) in: {checked:?}");
    assert!(checked.contains(&"fixture-cortex-replay".to_string()));
}

#[test]
fn band_fixture_sits_between_old_and_calibrated_lines() {
    let r = calibrate_dir("cortex-t31-band", &here().join("fixtures/cortex-t31-band")).unwrap();
    let inv = r.sshd["invalid_user"].max as u64;
    assert!(
        (INVALID_USER_WARN..30).contains(&inv),
        "band fixture invalid_user {inv}"
    );
    let desk = r.mem_kb["galion-desk"];
    assert!(desk > MEMORY_HEAVY_KIB && desk <= 512 * 1024, "{desk}");
}

#[test]
fn measured_numbers_match_the_committed_record() {
    let rs = live_reports();
    let path = here().join("replay-corpus/calibration.json");
    let got: BTreeMap<String, Value> = rs.iter().map(|(k, r)| (k.clone(), r.to_json())).collect();
    if std::env::var("CALIBRATION_BLESS").is_ok_and(|v| v == "1") {
        assert!(!got.is_empty(), "bless needs the live captures");
        let doc = json!({
            "about": "T3.1 threshold calibration (slot 207, docs/cortex/calibration.md): what `openmind-nerve calibrate` measures on every live capture, counts only (no addresses, names or hostnames). Checked by tests/calibration.rs; CALIBRATION_BLESS=1 rewrites it.",
            "captures": got,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let text = want.to_string();
    for bad in ["galion.", ".app", ".studio", "from 1", "from 2"] {
        assert!(!text.contains(bad), "calibration.json holds {bad:?}");
    }
    for (k, v) in &got {
        assert_eq!(
            &want["captures"][k], v,
            "{k}: measurement drifted (CALIBRATION_BLESS=1 if intended)"
        );
    }
}
