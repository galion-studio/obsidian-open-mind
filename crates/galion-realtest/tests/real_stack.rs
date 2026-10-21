// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life test: boots the whole stack from release binaries and runs every
//! scenario. Gated: runs only with `GALION_REAL=1`. Ports 19500-19551.
//! Also the port of `scripts/real/real-harness.sh` (deleted): the test builds
//! the release binaries itself (CPU-polite, into `$CARGO_TARGET_DIR/real-bins`)
//! unless `GALION_REAL_BIN_DIR` points at a prebuilt `<target>/release`, and
//! with `GALION_REAL_VPS=1` it also runs the READ-ONLY live probe
//! (`galion-realtest live`: GET-only over ssh; never writes or restarts).
//!
//!   GALION_REAL=1 [GALION_REAL_VPS=1] cargo test -p galion-realtest --test real_stack -- --nocapture

mod sh3;

use std::path::PathBuf;
use std::process::Command;

const PKGS: [&str; 10] = [
    "galion-realtest",
    "galion-boot",
    "galion-one",
    "openmind-memory",
    "tuner",
    "galion-desk",
    "link-watch",
    "guardian-tick",
    "persona-plex-relay",
    // optional in the harness; without it the nerve-ingest scenario is SKIP
    "openmind-nerve",
];

fn release_dir() -> PathBuf {
    let mut args = Vec::new();
    for p in PKGS {
        args.extend(["-p", p]);
    }
    sh3::release_bins(None, &args, &[])
}

#[test]
fn real_box_stack() {
    if !sh3::enabled("real_box_stack") {
        return;
    }
    let root = sh3::repo_root();
    let rel = release_dir();
    let out = rel.join("../realtest/cargo-test");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_galion-realtest"));
    cmd.arg("stack")
        .arg("--repo")
        .arg(&root)
        .arg("--bin-dir")
        .arg(&rel)
        .arg("--out")
        .arg(&out);
    if rel.join("openmind-nerve").exists() {
        cmd.arg("--with-nerve");
    }
    let st = cmd.status().expect("run galion-realtest");
    let junit = std::fs::read_to_string(out.join("junit.xml")).expect("junit.xml written");
    assert!(junit.contains("<testsuite name=\"one-pairing\""));
    assert!(std::fs::read_to_string(out.join("report.md"))
        .unwrap()
        .contains("+0"));
    assert!(
        st.success(),
        "unexpected failures; see {}",
        out.join("report.md").display()
    );
}

#[test]
fn real_live_probe_read_only() {
    if !sh3::enabled("real_live_probe_read_only") {
        return;
    }
    if !sh3::vps_enabled() {
        eprintln!("real_live_probe_read_only: skipped (set GALION_REAL_VPS=1)");
        return;
    }
    let out = sh3::target_dir().join("realtest/live");
    let st = Command::new(env!("CARGO_BIN_EXE_galion-realtest"))
        .arg("live")
        .arg("--ssh-host")
        .arg(sh3::vps_host())
        .arg("--out")
        .arg(&out)
        .status()
        .expect("run galion-realtest live");
    let report = std::fs::read_to_string(out.join("report.md")).unwrap_or_default();
    println!("{report}");
    assert!(
        st.success(),
        "live probe found mismatches; see {}",
        out.join("report.md").display()
    );
}
