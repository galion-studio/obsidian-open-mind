// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Repo lint (runs in every `cargo test --workspace`): a test file that
//! switches on `GALION_REAL` AND runs the sandbox (uses `openmind_sandbox`,
//! the sandboxed `hermes_tools`, or orchestrates `real_escape`) must call
//! `vps_sandbox_gate::sandbox_real`, so an ambient `GALION_REAL=1` can never
//! start a sandbox run off the VPS (F-383-10, sandbox_vps_only).

use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Mentions the `GALION_REAL` switch itself (not GALION_REAL_BASE / _BIN_DIR /
/// _CGROUP_PARENT / _VPS).
fn mentions_switch(src: &str) -> bool {
    src.match_indices("GALION_REAL").any(|(i, m)| {
        src[i + m.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c == '_' || c.is_ascii_alphanumeric()))
    })
}

fn runs_sandbox(src: &str) -> bool {
    ["openmind_sandbox", "hermes_tools::", "real_escape"]
        .iter()
        .any(|k| src.contains(k))
}

#[test]
fn sandbox_real_tests_use_the_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for top in ["crates", "edge"] {
        let Ok(rd) = std::fs::read_dir(root.join(top)) else {
            continue;
        };
        for c in rd.flatten() {
            rs_files(&c.path().join("tests"), &mut files);
        }
    }
    assert!(files.len() > 50, "found only {} test files", files.len());
    let mut checked = 0;
    let mut bad = Vec::new();
    for f in &files {
        if f.strip_prefix(&root)
            .is_ok_and(|r| r.starts_with("crates/vps-sandbox-gate"))
        {
            continue;
        }
        let src = std::fs::read_to_string(f).unwrap_or_default();
        if !mentions_switch(&src) || !runs_sandbox(&src) {
            continue;
        }
        checked += 1;
        if !src.contains("vps_sandbox_gate::sandbox_real") {
            bad.push(f.strip_prefix(&root).unwrap_or(f).display().to_string());
        }
    }
    println!("sandbox real test files checked: {checked}");
    assert!(
        checked >= 5,
        "expected >= 5 sandbox real test files, got {checked}"
    );
    assert!(
        bad.is_empty(),
        "GALION_REAL sandbox tests without vps_sandbox_gate::sandbox_real: {bad:?}"
    );
}
