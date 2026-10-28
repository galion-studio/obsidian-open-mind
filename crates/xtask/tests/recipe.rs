// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The real xtask binary on captured host trees (VPS, swarm box) and the
//! synthetic GitHub ubuntu-24.04 runner. A run with GALION_XTASK_HOST_ROOT
//! never starts cargo, so these tests run nothing sandboxed.
//!
//! `GALION_REAL=1` adds `real_host_*`: the binary on THIS host (dry run).

use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn xtask(root: Option<&str>, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_xtask"));
    c.args(args);
    for k in [
        "GALION_XTASK_HOST_ROOT",
        "GALION_SANDBOX_TESTS",
        "GALION_SANDBOX_VPS_ONLY",
        "HERMES_RS_CODING_ALLOW_NO_LANDLOCK",
    ] {
        c.env_remove(k);
    }
    if let Some(r) = root {
        c.env("GALION_XTASK_HOST_ROOT", fixture(r));
    }
    // A run must never reach cargo: point CARGO at something that fails.
    c.env("CARGO", "/nonexistent/cargo-must-not-run");
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().expect("run xtask")
}

fn out(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn cargo_line(o: &Output) -> String {
    out(o)
        .lines()
        .find(|l| l.starts_with("xtask: cargo "))
        .unwrap_or_default()
        .to_string()
}

const ALL_OUT: &str = "xtask: cargo test --workspace --exclude openmind-sandbox --exclude hermes-tools --exclude cgroup-detect";

#[test]
fn swarm_box_default_excludes_all_and_refuses_opt_in() {
    let o = xtask(Some("box-20261028"), &[], &["test"]);
    assert!(o.status.success(), "{}", out(&o));
    let s = out(&o);
    assert!(
        s.contains("landlock=yes userns=yes cgroup2=yes euid=1000 vps_only=yes"),
        "{s}"
    );
    assert_eq!(cargo_line(&o), ALL_OUT);
    assert!(s.contains("exclude hermes-tools (host is sandbox-VPS-only (sandbox_vps_only))"));
    assert!(s.contains("simulation only, cargo not run"));
    for flag in ["--sandbox", "--sandbox=require"] {
        let o = xtask(Some("box-20261028"), &[], &["test", flag]);
        assert_eq!(o.status.code(), Some(3), "{flag}: {}", out(&o));
        assert!(out(&o).contains("refused"), "{}", out(&o));
        assert_eq!(cargo_line(&o), "");
    }
    let o = xtask(
        Some("box-20261028"),
        &[("GALION_SANDBOX_TESTS", "1")],
        &["test"],
    );
    assert_eq!(o.status.code(), Some(3), "{}", out(&o));
}

#[test]
fn vps_without_landlock_drops_hermes_tools_only() {
    let o = xtask(Some("vps-20261028"), &[], &["test", "--", "--locked"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("landlock=no userns=yes cgroup2=yes euid=65534 vps_only=no"));
    assert_eq!(
        cargo_line(&o),
        "xtask: cargo test --workspace --exclude hermes-tools --locked"
    );
    assert!(out(&o).contains("exclude hermes-tools (host lacks landlock)"));
    // F-320-1 override: the coding preset may run without Landlock.
    let o = xtask(
        Some("vps-20261028"),
        &[("HERMES_RS_CODING_ALLOW_NO_LANDLOCK", "1")],
        &["test"],
    );
    assert_eq!(cargo_line(&o), "xtask: cargo test --workspace");
    // require fails loudly instead of skipping.
    let o = xtask(Some("vps-20261028"), &[], &["test", "--sandbox=require"]);
    assert_eq!(o.status.code(), Some(3));
    assert!(
        out(&o).contains("hermes-tools needs landlock"),
        "{}",
        out(&o)
    );
    let p = xtask(Some("vps-20261028"), &[], &["probe"]);
    assert!(p.status.success());
    assert!(out(&p).contains("xtask: hermes-tools lacks landlock"));
    assert!(out(&p).contains("xtask: openmind-sandbox supported"));
}

#[test]
fn ci_runner_runs_them_after_the_sysctl_step() {
    // self-eval.yml test job: sysctl step, then --sandbox=require.
    let o = xtask(
        Some("gh-ubuntu-2404-sysctl"),
        &[],
        &["test", "--sandbox=require", "--", "--locked"],
    );
    assert!(o.status.success(), "{}", out(&o));
    let s = out(&o);
    for k in ["openmind-sandbox", "hermes-tools", "cgroup-detect"] {
        assert!(s.contains(&format!("xtask: include {k}")), "{s}");
    }
    assert_eq!(cargo_line(&o), "xtask: cargo test --workspace --locked");
    // F-335-1: the test processes learn the mode, so sandboxed.rs fails
    // instead of skipping on CI.
    assert!(
        s.contains("xtask: test env GALION_SANDBOX_TESTS=require"),
        "{s}"
    );
    // Without the sysctl step AppArmor blocks the uid_map: require fails.
    let o = xtask(
        Some("gh-ubuntu-2404-default"),
        &[],
        &["test", "--sandbox=require"],
    );
    assert_eq!(o.status.code(), Some(3), "{}", out(&o));
    assert!(out(&o).contains("hermes-tools needs userns"), "{}", out(&o));
    // ... and auto keeps openmind-sandbox (Landlock alone is enough).
    let o = xtask(Some("gh-ubuntu-2404-default"), &[], &["test"]);
    assert_eq!(
        cargo_line(&o),
        "xtask: cargo test --workspace --exclude hermes-tools"
    );
    assert!(out(&o).contains("xtask: test env GALION_SANDBOX_TESTS=auto"));
}

#[test]
fn opt_out_and_bad_args() {
    let o = xtask(
        Some("gh-ubuntu-2404-sysctl"),
        &[],
        &["test", "--no-sandbox"],
    );
    assert_eq!(cargo_line(&o), ALL_OUT);
    assert!(out(&o).contains("xtask: test env GALION_SANDBOX_TESTS=off"));
    let o = xtask(
        Some("gh-ubuntu-2404-sysctl"),
        &[("GALION_SANDBOX_VPS_ONLY", "1")],
        &["test"],
    );
    assert_eq!(cargo_line(&o), ALL_OUT);
    let o = xtask(Some("gh-ubuntu-2404-sysctl"), &[], &["test", "-p", "x"]);
    assert_eq!(o.status.code(), Some(2));
    let o = xtask(Some("gh-ubuntu-2404-sysctl"), &[], &["bogus"]);
    assert_eq!(o.status.code(), Some(2));
    let o = xtask(Some("gh-ubuntu-2404-sysctl"), &[], &["test", "--sandbox"]);
    assert!(o.status.success());
    assert_eq!(cargo_line(&o), "xtask: cargo test --workspace");
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

#[test]
fn real_host_dry_run() {
    if !real() {
        eprintln!("SKIP: set GALION_REAL=1");
        return;
    }
    let o = xtask(None, &[], &["test", "--dry-run"]);
    assert!(o.status.success(), "{}", out(&o));
    eprintln!("{}", out(&o));
    if std::path::Path::new("/workspace/swarm/RULES.md").exists() {
        // The swarm box: never the sandbox crates, never an opt-in.
        assert_eq!(cargo_line(&o), ALL_OUT);
        let o = xtask(None, &[], &["test", "--sandbox"]);
        assert_eq!(o.status.code(), Some(3), "{}", out(&o));
        assert_eq!(cargo_line(&o), "");
    }
}
