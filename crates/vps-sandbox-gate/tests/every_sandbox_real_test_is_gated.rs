// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Repo lint (runs in every `cargo test --workspace`): a test file that
//! switches on `GALION_REAL` AND runs the sandbox (uses `openmind_sandbox`,
//! the sandboxed `hermes_tools`, or orchestrates `real_escape`) must call
//! `vps_sandbox_gate::sandbox_real`, so an ambient `GALION_REAL=1` can never
//! start a sandbox run off the VPS (F-383-10, sandbox_vps_only).
//!
//! Second rule (D-400-2, slot 444): in a suite that RUNS the sandbox even
//! without `GALION_REAL` (it probes `openmind_sandbox::features()`, or is
//! listed in [`SANDBOX_SUITES`]), EVERY `#[test]` fn must reach the gate,
//! directly or through a local helper fn that calls it. Before slot 444 the
//! 8 plain tests of `hermes-tools/tests/sandboxed.rs` ran the sandbox on the
//! box in a plain `cargo test --workspace`; that is why workspace runs had
//! to exclude hermes-tools.

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

/// Suites known to run the sandbox in every test (kept even if the
/// `features()` probe moves elsewhere).
const SANDBOX_SUITES: [&str; 1] = ["crates/hermes-tools/tests/sandboxed.rs"];

/// `(name, body)` of every top-level `fn` in `src` (brace matching; good
/// enough for our own test files, which are rustfmt'ed).
fn fns(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0;
    while let Some(off) = src[i..].find("\nfn ") {
        let start = i + off + 4;
        let name: String = src[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some(open) = src[start..].find('{').map(|o| start + o) else {
            break;
        };
        let (mut depth, mut j) = (0usize, open);
        while j < b.len() {
            match b[j] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let end = j.min(b.len());
        out.push((name, src[open..end].to_string()));
        i = end;
    }
    out
}

/// Names of `#[test]` fns in `src`.
fn test_fns(src: &str) -> Vec<String> {
    src.match_indices("#[test]")
        .filter_map(|(i, _)| {
            let rest = &src[i..];
            let f = rest.find("\nfn ")? + 4;
            Some(
                rest[f..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect(),
            )
        })
        .collect()
}

/// `#[test]` fns of `src` that never reach `vps_sandbox_gate::sandbox_real`.
fn ungated_tests(src: &str) -> Vec<String> {
    let all = fns(src);
    let helpers: Vec<&str> = all
        .iter()
        .filter(|(_, body)| body.contains("vps_sandbox_gate::sandbox_real"))
        .map(|(n, _)| n.as_str())
        .collect();
    let tests = test_fns(src);
    all.iter()
        .filter(|(n, _)| tests.contains(n))
        .filter(|(n, body)| {
            !body.contains("vps_sandbox_gate::sandbox_real")
                && !helpers
                    .iter()
                    .any(|h| h != n && body.contains(&format!("{h}(")))
        })
        .map(|(n, _)| n.clone())
        .collect()
}

#[test]
fn every_test_in_a_sandbox_running_suite_reaches_the_gate() {
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
    let mut suites = 0;
    let mut tests = 0;
    let mut bad = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
        if rel.starts_with("crates/vps-sandbox-gate") || rel.starts_with("crates/openmind-sandbox")
        {
            // the sandbox crate's own tests are covered by the first rule
            continue;
        }
        let src = std::fs::read_to_string(f).unwrap_or_default();
        let listed = SANDBOX_SUITES.iter().any(|s| *s == rel);
        if !listed && !src.contains("openmind_sandbox::features()") {
            continue;
        }
        suites += 1;
        tests += test_fns(&src).len();
        for t in ungated_tests(&src) {
            bad.push(format!("{rel}::{t}"));
        }
    }
    for s in SANDBOX_SUITES {
        assert!(
            root.join(s).is_file(),
            "listed sandbox suite {s} is gone; update SANDBOX_SUITES"
        );
    }
    println!("sandbox-running suites: {suites}, tests checked: {tests}");
    assert!(suites >= 1 && tests >= 10, "suites {suites} tests {tests}");
    assert!(
        bad.is_empty(),
        "tests in sandbox-running suites that never reach vps_sandbox_gate::sandbox_real: {bad:?}"
    );
}

#[test]
fn gate_reach_check_sees_direct_helper_and_missing() {
    let src = "use x;\n\nfn skip(t: &str) -> bool {\n    if !vps_sandbox_gate::sandbox_real(t) {\n        return true;\n    }\n    false\n}\n\n#[test]\nfn via_helper() {\n    if skip(\"via_helper\") {\n        return;\n    }\n}\n\n#[test]\nfn direct() {\n    if !vps_sandbox_gate::sandbox_real(\"d\") {\n        return;\n    }\n}\n\n#[test]\nfn plain() {\n    let v = vec![1];\n    assert_eq!(v.len(), 1);\n}\n";
    assert_eq!(test_fns(src), ["via_helper", "direct", "plain"]);
    assert_eq!(ungated_tests(src), ["plain"]);
}
