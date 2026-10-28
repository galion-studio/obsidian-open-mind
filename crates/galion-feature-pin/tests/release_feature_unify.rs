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

use common::{cargo, p_args, release_packages, workspace_root, RELEASE_BINS, TARGETS};
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
    let all = release_packages();
    let mut bad = Vec::new();
    let mut checked = 0usize;
    for target in TARGETS {
        let full = tree(&all, target);
        assert!(
            full.len() > 40,
            "full set graph looks empty for {target}: {} lines",
            full.len()
        );
        for pkg in release_packages() {
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
    let all = release_packages();
    let target = TARGETS[0];
    let full = tree(&all, target);
    let partials: [&[&str]; 5] = [
        &["openmind-nerve", "hermes-gateway"],
        // slot 337: gaia-cortex links openmind-nerve as a library; console-static
        // carries galion-console-serve.
        &["gaia-cortex", "openmind-nerve"],
        &["console-static", "gaia-cortex", "hermes-gateway"],
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

/// The section body (until the next `[` table line) of every `[[bin]]` /
/// `[features]` table in a manifest, as raw lines.
fn tables<'a>(text: &'a str, header: &str) -> Vec<Vec<&'a str>> {
    let mut out = Vec::new();
    let mut cur: Option<Vec<&str>> = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            if let Some(c) = cur.take() {
                out.push(c);
            }
            if line == header {
                cur = Some(Vec::new());
            }
        } else if let Some(c) = cur.as_mut() {
            c.push(line);
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

/// D-325-2 (slot 337): every RELEASE_BINS row names a real binary of its
/// package and only features that the package declares, so the list cannot go
/// stale silently.
#[test]
fn release_bins_are_real_binaries_and_features() {
    for r in RELEASE_BINS {
        let path = workspace_root()
            .join("crates")
            .join(r.pkg)
            .join("Cargo.toml");
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            text.contains(&format!("name = \"{}\"", r.pkg)),
            "{} is not package {}",
            path.display(),
            r.pkg
        );
        let bins: Vec<String> = tables(&text, "[[bin]]")
            .iter()
            .flat_map(|t| t.iter())
            .filter_map(|l| l.strip_prefix("name = "))
            .map(|v| v.trim_matches('"').to_string())
            .collect();
        let implicit = bins.is_empty()
            && r.bin == r.pkg
            && workspace_root()
                .join("crates")
                .join(r.pkg)
                .join("src/main.rs")
                .is_file();
        assert!(
            implicit || bins.iter().any(|b| b == r.bin),
            "{}: no binary {} (has {bins:?})",
            r.pkg,
            r.bin
        );
        let feats: Vec<String> = tables(&text, "[features]")
            .iter()
            .flat_map(|t| t.iter())
            .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_string()))
            .collect();
        for f in r.features {
            assert!(
                feats.iter().any(|x| x == f),
                "{}: feature {f} not declared ({feats:?})",
                r.pkg
            );
        }
        assert!(r.budget > 0, "{}: no size budget", r.bin);
    }
    let mut names: Vec<&str> = RELEASE_BINS.iter().map(|r| r.bin).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), RELEASE_BINS.len(), "duplicate binary names");
}

/// The per-bin caps in RELEASE_BINS are the caps of scripts/size-budget.sh:
/// 2 MiB, and 2.5 MiB for hermes-gateway only.
#[test]
fn release_budgets_match_size_budget_script() {
    let script = std::fs::read_to_string(workspace_root().join("scripts/size-budget.sh"))
        .expect("read scripts/size-budget.sh");
    assert!(
        script.contains("LIMIT=$((2 * 1024 * 1024))"),
        "size-budget.sh LIMIT changed"
    );
    assert!(
        script.contains("HERMES_GATEWAY_LIMIT=$((5 * 1024 * 1024 / 2))"),
        "size-budget.sh HERMES_GATEWAY_LIMIT changed"
    );
    assert_eq!(common::SIZE_LIMIT, 2 * 1024 * 1024);
    assert_eq!(common::HERMES_GATEWAY_LIMIT, 2_621_440);
    for r in RELEASE_BINS {
        let want = if r.bin == "hermes-gateway" {
            common::HERMES_GATEWAY_LIMIT
        } else {
            common::SIZE_LIMIT
        };
        assert_eq!(r.budget, want, "{} budget", r.bin);
    }
    for new in ["gaia-cortex", "galion-console-serve"] {
        assert!(
            RELEASE_BINS.iter().any(|r| r.bin == new),
            "{new} missing from RELEASE_BINS (D-325-2)"
        );
    }
}
