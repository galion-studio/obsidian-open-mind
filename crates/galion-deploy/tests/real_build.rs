// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rehearsal inputs (Rust port of scripts/real/deploy-sim-build.sh, which this branch
//! deletes). Gated by GALION_REAL=1.
//!
//! * Default: verify that every binary the rehearsals use exists in
//!   `$DEPLOY_SIM_BIN` (default /workspace/swarm/sim/deploy-sim/bin) with the sha256
//!   pinned below (the deploy-sim run-5 set), and that tuner-new is byte-identical to
//!   the swarm/tuner-cutover artefact.
//! * `GALION_REAL_BUILD=1`: build them instead (static x86_64 musl, release, from
//!   detached git worktrees under /workspace/swarm/sim/deploy-rust/src) into
//!   `$GALION_REAL_BUILD_OUT` (default /workspace/swarm/sim/deploy-rust/bin), then
//!   verify the same pins (reproducibility check). `GALION_REAL_BUILD_ONLY=a,b`
//!   limits the set. Point `DEPLOY_SIM_BIN` at the output to rehearse with it.
//!
//! Box only; nothing is copied anywhere else, nothing touches the VPS.

use std::path::{Path, PathBuf};
use std::process::Command;

const TRIPLE: &str = "x86_64-unknown-linux-musl";

/// (output name, git rev or "" for this tree, package, bin, sha256 pin)
const SET: &[(&str, &str, &str, &str, &str)] = &[
    (
        "simsup",
        "",
        "simsup",
        "simsup",
        "d9b40213c88ec7a5e5885a1dcb977861bc9c5ff35f0de08b0dba222a38b8ff72",
    ),
    (
        "sim-broken",
        "",
        "simsup",
        "sim-broken",
        "cf3c5ab9b1c9a4886a4d41b91cd395dd693342774a787539a7b80b6f5908a3d1",
    ),
    (
        "sim-hang",
        "",
        "simsup",
        "sim-hang",
        "d3a31fa16959cb73b474e081f7811bff44826690f5c743ce6d70b5176f89f1c5",
    ),
    (
        "tuner-old",
        "13a296ce15b5bbd66adc26f6bfae787b2c1b5190",
        "tuner",
        "tuner",
        "067c56bede5ad2494c33f686c6f98f012cc7c13ffd4f7ec6515955faf0653090",
    ),
    (
        "tuner-new",
        "5fd1611",
        "tuner",
        "tuner",
        "47a17df2234fdd5c741f13e9403228e1d4f4dc5f73ac7b41e7996d5e3c511370",
    ),
    (
        "nerve-old",
        "a63b730",
        "galion-nerve",
        "galion-nerve",
        "3b150dea692ffac098945e9413c4fe1caf95ee3a50d3f6c937e09df3598d6d3d",
    ),
    (
        "nerve-new",
        "5802d0e1dd69f3f71b3a1926f00d626ddd9c788d",
        "openmind-nerve",
        "openmind-nerve",
        "ad9a5b29d8ce71337c9945e10e8117700da6c4b3dd3d5aeb1f2f96e183bd106a",
    ),
    (
        "room-old",
        "13a296ce15b5bbd66adc26f6bfae787b2c1b5190",
        "room",
        "room",
        "d06001090e1a42dd71e2df88eb76c234559e98f6c5addfca95de9519bba2eac1",
    ),
    (
        "room-new",
        "5bb27e1",
        "room",
        "room",
        "05075415e0c54025ca4e23d4297d5e70e983741b392992b0879865635c460949",
    ),
    (
        "desk-old",
        "13a296ce15b5bbd66adc26f6bfae787b2c1b5190",
        "galion-desk",
        "galion-desk",
        "869b60fc5ba0078f79202ffbc4b6da3a197747749245f1d37cbdb1dd700da76d",
    ),
    (
        "desk-new",
        "a54c792",
        "galion-desk",
        "galion-desk",
        "b46b148f2b65d8f51b997a8023242e86fca6908c565511aa3678c4ec7893fce5",
    ),
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn env_or(k: &str, d: &str) -> PathBuf {
    std::env::var(k)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(d))
}

fn run(c: &mut Command) {
    eprintln!("$ {c:?}");
    let st = c.status().unwrap();
    assert!(st.success(), "{c:?} failed: {st}");
}

fn worktree(rev: &str) -> PathBuf {
    if rev.is_empty() {
        return repo().join("sim/deploy-sim");
    }
    let d = env_or("DEPLOY_RUST_SIM", "/workspace/swarm/sim/deploy-rust")
        .join("src")
        .join(&rev[..7]);
    if !d.exists() {
        run(Command::new("git")
            .arg("-C")
            .arg(repo())
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&d)
            .arg(rev));
    }
    run(Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["checkout", "-q", "--detach", rev]));
    d
}

fn build(name: &str, rev: &str, pkg: &str, bin: &str, out: &Path) {
    let dir = worktree(rev);
    let tgt = env_or("CARGO_TARGET_DIR", "/workspace/swarm/target/deploy-rust")
        .join("rehearsal")
        .join(if rev.is_empty() { "self" } else { &rev[..7] });
    run(Command::new("nice")
        .args([
            "-n",
            "10",
            env!("CARGO"),
            "build",
            "-q",
            "--release",
            "--locked",
            "-j",
            "2",
            "-p",
            pkg,
            "--bin",
            bin,
            "--target",
            TRIPLE,
        ])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", &tgt)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld"));
    std::fs::create_dir_all(out).unwrap();
    let dst = out.join(name);
    std::fs::copy(tgt.join(TRIPLE).join("release").join(bin), &dst).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn rehearsal_inputs() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1");
        return;
    }
    let building = std::env::var("GALION_REAL_BUILD").as_deref() == Ok("1");
    let only: Vec<String> = std::env::var("GALION_REAL_BUILD_ONLY")
        .map(|v| v.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    let dir = if building {
        env_or(
            "GALION_REAL_BUILD_OUT",
            "/workspace/swarm/sim/deploy-rust/bin",
        )
    } else {
        env_or("DEPLOY_SIM_BIN", "/workspace/swarm/sim/deploy-sim/bin")
    };
    let mut bad = Vec::new();
    for (name, rev, pkg, bin, pin) in SET {
        if !only.is_empty() && !only.iter().any(|o| o == name) {
            continue;
        }
        if building {
            build(name, rev, pkg, bin, &dir);
        }
        let got =
            galion_deploy::sha256::file_hex(&dir.join(name)).unwrap_or_else(|_| "missing".into());
        let ok = got == *pin;
        eprintln!(
            "{} {name:<11} {got}{}",
            if ok { "OK  " } else { "DIFF" },
            if ok { "" } else { " (pinned: different)" }
        );
        if !ok {
            bad.push(name.to_string());
        }
    }
    let art = Path::new("/workspace/swarm/artifacts/tuner/tuner-x86_64-unknown-linux-musl");
    if art.exists() && (only.is_empty() || only.iter().any(|o| o == "tuner-new")) {
        let same = std::fs::read(art).ok() == std::fs::read(dir.join("tuner-new")).ok();
        eprintln!("tuner-new == swarm/tuner-cutover artefact: {same}");
        assert!(same);
    }
    assert!(bad.is_empty(), "sha mismatch: {bad:?} in {}", dir.display());
}
