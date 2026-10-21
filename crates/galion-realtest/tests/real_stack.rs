// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life test: boots the whole stack from release binaries and runs every
//! scenario. Gated: runs only with `GALION_REAL=1` and needs the release
//! binaries (`cargo build --release -p galion-boot -p galion-one ...`, or
//! `scripts/real/real-harness.sh`). Ports 19500-19551 (override the base with
//! `REALTEST_BASE_PORT`). Mirrors `.github/workflows/realtest.yml`: when an
//! `openmind-nerve` release binary is present the stack runs `--with-nerve`
//! and the nerve-ingest suite must actually run (no skip).

use std::path::PathBuf;
use std::process::Command;

fn release_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("release")
}

#[test]
fn real_box_stack() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (needs release binaries)");
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = release_dir().join("../realtest/cargo-test");
    let with_nerve = release_dir().join("openmind-nerve").is_file();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_galion-realtest"));
    cmd.arg("stack");
    if with_nerve {
        cmd.arg("--with-nerve");
    }
    if let Ok(p) = std::env::var("REALTEST_BASE_PORT") {
        cmd.arg("--base-port").arg(p);
    }
    let st = cmd
        .arg("--repo")
        .arg(&root)
        .arg("--bin-dir")
        .arg(release_dir())
        .arg("--out")
        .arg(&out)
        .status()
        .expect("run galion-realtest");
    let junit = std::fs::read_to_string(out.join("junit.xml")).expect("junit.xml written");
    assert!(junit.contains("<testsuite name=\"one-pairing\""));
    if with_nerve {
        let nerve = junit
            .split("<testsuite name=\"nerve-ingest\"")
            .nth(1)
            .and_then(|s| s.split("</testsuite>").next())
            .expect("nerve-ingest suite in junit.xml");
        assert!(
            nerve.contains("name=\"ingest-signal-202\"") && !nerve.contains("<skipped"),
            "nerve-ingest must run, not skip, with --with-nerve"
        );
    }
    assert!(std::fs::read_to_string(out.join("report.md"))
        .unwrap()
        .contains("+0"));
    assert!(
        st.success(),
        "unexpected failures; see {}",
        out.join("report.md").display()
    );
}
