// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Guard for D-312-4: every release binary built alone must see exactly the
//! same dependency features as inside the full release `-p` set.
//!
//! Cargo's `-p` feature unification is the only source of difference between
//! the two builds (same source, same profile, same flags), so equal feature
//! sets for every unit mean equal units. `cargo tree --offline --locked` is
//! read-only and fast (no build); the byte-level proof is
//! `real_release_repro.rs` (GALION_REAL=1).

mod common;

use common::{cargo, p_args, workspace_root, RELEASE_BINS, TARGETS};
use std::collections::BTreeSet;
use std::process::Command;

/// Lines `<package id>|<features>` of the normal + build graph.
fn tree(pkgs: &[&str], target: &str) -> BTreeSet<String> {
    let out = Command::new(cargo())
        .current_dir(workspace_root())
        .arg("tree")
        .args(p_args(pkgs))
        .args([
            "--offline",
            "--locked",
            "--target",
            target,
            "-e",
            "normal,build",
            "--prefix",
            "none",
            "--no-dedupe",
            "-f",
            "{p}|{f}",
        ])
        .output()
        .expect("run cargo tree");
    assert!(
        out.status.success(),
        "cargo tree {pkgs:?} --target {target} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.contains('|'))
        .map(str::to_string)
        .collect()
}

/// Packages of `solo` whose feature set is not one of the full-set ones.
fn mismatches(solo: &BTreeSet<String>, full: &BTreeSet<String>) -> Vec<String> {
    solo.iter()
        .filter(|l| !full.contains(*l))
        .map(|l| {
            let pkg = l.split('|').next().unwrap_or("");
            let in_full: Vec<&str> = full
                .iter()
                .filter(|f| f.split('|').next() == Some(pkg))
                .map(|f| f.split('|').nth(1).unwrap_or(""))
                .collect();
            format!("{l}  (full set: {in_full:?})")
        })
        .collect()
}

#[test]
fn every_release_bin_alone_has_the_full_set_features() {
    let all: Vec<&str> = RELEASE_BINS.iter().map(|(p, _)| *p).collect();
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for target in TARGETS {
        let full = tree(&all, target);
        assert!(
            full.len() > 40,
            "full set graph looks empty for {target}: {} lines",
            full.len()
        );
        for (pkg, _) in RELEASE_BINS {
            let solo = tree(&[pkg], target);
            assert!(!solo.is_empty(), "{pkg} {target}: empty graph");
            checked += solo.len();
            for m in mismatches(&solo, &full) {
                bad.push(format!("{target} -p {pkg}: {m}"));
            }
        }
    }
    eprintln!(
        "release_feature_unify: {checked} solo units checked, {} mismatches",
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "features differ between solo and full-set release builds (D-312-4); pin them in the root \
         [workspace.dependencies] or crates/galion-feature-pin:\n{}",
        bad.join("\n")
    );
}

/// Pairs of release binaries must also agree with each other (the slot 312
/// `-p openmind-nerve -p hermes-gateway` case).
#[test]
fn partial_sets_have_the_full_set_features() {
    let all: Vec<&str> = RELEASE_BINS.iter().map(|(p, _)| *p).collect();
    let target = TARGETS[0];
    let full = tree(&all, target);
    let partials: [&[&str]; 3] = [
        &["openmind-nerve", "hermes-gateway"],
        &["tuner", "desk-hermes", "link-watch"],
        &[
            "galion-backup",
            "openmind-memory",
            "galion-desk",
            "galion-one",
        ],
    ];
    for set in partials {
        let part = tree(set, target);
        let bad = mismatches(&part, &full);
        assert!(bad.is_empty(), "{set:?}:\n{}", bad.join("\n"));
    }
}

/// galion-common is linked by most release binaries: it must stay feature-free.
#[test]
fn galion_common_has_no_features() {
    let text = std::fs::read_to_string(workspace_root().join("crates/galion-common/Cargo.toml"))
        .expect("read galion-common Cargo.toml");
    let tables: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('['))
        .collect();
    assert!(
        !tables.contains(&"[features]"),
        "galion-common must not have [features] (D-312-4)"
    );
    assert!(
        !text.contains("optional = true"),
        "no optional deps in galion-common"
    );
}
