// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life tests of the `nerve` coverage gate: run the real repo-check
//! binary on the real tree and on throwaway fixture trees, and (opt-in)
//! compare the static verdict with the live nerve's view of who pushes.
//!
//! GALION_REAL=1 cargo test --release -p repo-check --test real_nerve_gate -- --nocapture
//! GALION_REAL_VPS=1 additionally reads http://127.0.0.1:8670/state on the VPS
//! over `ssh slavik-vps` (GET only; host alias from REPO_CHECK_VPS).

use std::path::{Path, PathBuf};
use std::process::Command;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_repo-check")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(root: &Path, extra: &[&str]) -> (i32, String) {
    let out = Command::new(bin())
        .arg("--root")
        .arg(root)
        .args(["--only", "nerve"])
        .args(extra)
        .output()
        .expect("run repo-check");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// `(package:bin, status)` pairs from the JSON report (std only).
fn bins_from_json(json: &str) -> Vec<(String, String)> {
    let Some(start) = json.find("\"nerve_bins\":[") else {
        return Vec::new();
    };
    json[start..]
        .split("{\"package\":\"")
        .skip(1)
        .map(|obj| {
            let pkg = obj.split('"').next().unwrap_or("").to_string();
            let field = |k: &str| {
                obj.split(&format!("\"{k}\":\""))
                    .nth(1)
                    .and_then(|r| r.split('"').next())
                    .unwrap_or("")
                    .to_string()
            };
            (format!("{pkg}:{}", field("bin")), field("status"))
        })
        .collect()
}

fn field_num(json: &str, key: &str) -> i64 {
    json.split(&format!("\"{key}\":"))
        .nth(1)
        .and_then(|r| {
            r.split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(-1)
}

#[test]
fn real_tree_lists_gaps() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let root = repo_root();
    let (code, table) = run(&root, &["--nerve-gaps"]);
    println!("{table}");
    let (jcode, json) = run(&root, &["--json"]);
    assert_eq!(code, jcode);
    let bins = bins_from_json(&json);
    let gaps: Vec<&str> = bins
        .iter()
        .filter(|(_, s)| s == "gap")
        .map(|(b, _)| b.as_str())
        .collect();
    let errors = field_num(&json, "errors");
    println!(
        "real tree: {} bins, {} gaps, {errors} errors, exit {code}",
        bins.len(),
        gaps.len()
    );
    assert_eq!(
        errors as usize,
        gaps.len(),
        "one error per gap, no malformed allowlist"
    );
    assert_eq!(
        field_num(&json, "warnings"),
        0,
        "no stale/redundant entries"
    );
    assert_eq!(code, if gaps.is_empty() { 0 } else { 1 });
    let st = |id: &str| {
        bins.iter()
            .find(|(b, _)| b == id)
            .map(|(_, s)| s.as_str())
            .unwrap_or("missing")
    };
    // The two live pushers and the nerve itself.
    assert_eq!(st("openmind-memory:openmind-memory"), "covered");
    assert_eq!(st("hermes-gateway:hermes-gateway"), "covered");
    assert_eq!(st("openmind-nerve:openmind-nerve"), "probe-only");

    // Bin set equals what Cargo itself sees.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let meta = Command::new(cargo)
        .current_dir(&root)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .output()
        .expect("cargo metadata");
    assert!(meta.status.success());
    let meta = String::from_utf8_lossy(&meta.stdout);
    let cargo_bins = meta.matches("\"kind\":[\"bin\"]").count();
    println!("cargo metadata bin targets: {cargo_bins}");
    assert_eq!(cargo_bins, bins.len());
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn real_fixture_tree_exit_codes() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let d = std::env::temp_dir().join(format!("repo-check-nerve-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    write(&d, "Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
    write(
        &d,
        "crates/nerve-client/Cargo.toml",
        "[package]\nname = \"nerve-client\"\n",
    );
    write(&d, "crates/nerve-client/src/lib.rs", "");
    write(
        &d,
        "crates/svc/Cargo.toml",
        "[package]\nname = \"svc\"\n[dependencies]\nnerve-client = { path = \"../nerve-client\" }\n",
    );
    write(&d, "crates/svc/src/main.rs", "fn main() {}\n");
    write(&d, "crates/tool/Cargo.toml", "[package]\nname = \"tool\"\n");
    write(&d, "crates/tool/src/main.rs", "fn main() {}\n");
    write(&d, "allow.txt", "tool:tool  offline linter\n");

    // Default allowlist path missing: tool is a gap -> exit 1 + warning.
    let (c, out) = run(&d, &["--nerve-gaps"]);
    println!("{out}");
    assert_eq!(c, 1);
    assert!(out.contains("1 error(s), 1 warning(s)"), "{out}");

    // Override path: all covered or probe-only -> exit 0.
    let (c, out) = run(&d, &["--nerve-allowlist", "allow.txt", "--nerve-gaps"]);
    println!("{out}");
    assert_eq!(c, 0, "{out}");
    assert!(
        out.contains("2 bins, 1 covered, 1 probe-only, 0 gap(s)"),
        "{out}"
    );

    // A new uncovered bin and a stale entry.
    write(&d, "crates/svc/src/bin/sidecar.rs", "fn main() {}\n");
    write(
        &d,
        "allow.txt",
        "tool:tool  offline linter\nold:old  removed crate\n",
    );
    let (c, out) = run(&d, &["--nerve-allowlist", "allow.txt"]);
    println!("{out}");
    // sidecar shares svc's deps, so it is covered; only the stale warning remains.
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("stale probe-only entry old:old"), "{out}");
    write(&d, "crates/svc/Cargo.toml", "[package]\nname = \"svc\"\n");
    let (c, out) = run(&d, &["--nerve-allowlist", "allow.txt"]);
    println!("{out}");
    assert_eq!(c, 1);
    assert!(
        out.contains("bin svc:svc does not depend on nerve-client"),
        "{out}"
    );
    assert!(
        out.contains("bin svc:sidecar does not depend on nerve-client"),
        "{out}"
    );
    // --warn nerve downgrades the gate.
    let (c, _) = run(&d, &["--nerve-allowlist", "allow.txt", "--warn", "nerve"]);
    assert_eq!(c, 0);
    let _ = std::fs::remove_dir_all(&d);
}

/// Live nerve component name -> workspace bin that runs it on the VPS.
const LIVE: &[(&str, &str)] = &[
    ("openmind-memory", "openmind-memory:openmind-memory"),
    ("hermes-rs", "hermes-gateway:hermes-gateway"),
    ("link-watch-rs", "link-watch:link-watch"),
    ("galion-desk", "galion-desk:galion-desk"),
    ("tuner", "tuner:tuner"),
    ("galion-one", "galion-one:galion-one"),
    ("galion-health", "galion-health:galion-health"),
];

#[test]
fn real_vps_nerve_agrees() {
    if !real() || std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let host = std::env::var("REPO_CHECK_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", &host])
        .arg("curl -s -m 5 http://127.0.0.1:8670/state")
        .output()
        .expect("ssh");
    assert!(out.status.success(), "ssh/curl failed");
    let state = String::from_utf8_lossy(&out.stdout).into_owned();
    let (_, json) = run(&repo_root(), &["--json"]);
    let bins = bins_from_json(&json);
    let mut checked = 0;
    for (comp, bin_id) in LIVE {
        // Component objects carry `"name":"<comp>",...,"push_age_s":N|null`.
        let Some(obj) = state.split(&format!("\"name\":\"{comp}\",")).nth(1) else {
            println!("{comp:<16} not in nerve state");
            continue;
        };
        let age = obj
            .split("\"push_age_s\":")
            .nth(1)
            .and_then(|r| r.split([',', '}']).next())
            .unwrap_or("?");
        let pushes = age != "null" && age != "?";
        let status = bins
            .iter()
            .find(|(b, _)| b == bin_id)
            .map(|(_, s)| s.as_str())
            .unwrap_or("missing");
        println!("{comp:<16} push_age_s={age:<5} {bin_id:<34} {status}");
        assert_eq!(
            pushes,
            status == "covered",
            "{comp}: live push ({age}) disagrees with static verdict ({status})"
        );
        checked += 1;
    }
    assert!(checked >= 5, "too few live components matched ({checked})");
}
