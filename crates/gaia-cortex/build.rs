// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Build stamp for `gaia-cortex --version` and `/health` (F-357-1).
//!
//! Std only, no new dependencies. The stamp is a pure function of the source
//! commit, so two builds of the same commit stay byte-identical (the
//! reproducible-build check): it NEVER reads the wall clock.
//!
//! * commit: `GALION_BUILD_COMMIT` if set (use it for `git archive` builds),
//!   else `git rev-parse HEAD` of THIS workspace's checkout, else `unknown`.
//! * date (unix seconds, rendered in Europe/Zagreb at runtime):
//!   `SOURCE_DATE_EPOCH` if set (reproducible-builds.org convention), else the
//!   commit's committer date from git, else 0 (`unknown`).
//! * The two sources (env vs git) give the SAME bytes for one commit, so a
//!   `git archive` build stamped via env equals a checkout build.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn hex_commit(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GALION_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    // crates/gaia-cortex -> workspace root.
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest.clone());

    // Only trust git when the checkout's top level IS this workspace: a
    // `git archive` tree unpacked inside some other repo must not pick up
    // that repo's HEAD.
    let in_own_checkout = git(&root, &["rev-parse", "--show-toplevel"])
        .and_then(|t| std::fs::canonicalize(t).ok())
        .zip(std::fs::canonicalize(&root).ok())
        .is_some_and(|(a, b)| a == b);

    let env_commit = std::env::var("GALION_BUILD_COMMIT")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let (commit, from_git) = match env_commit {
        Some(c) if hex_commit(&c) => (c, false),
        Some(c) => panic!("GALION_BUILD_COMMIT must be 7-64 hex chars, got {c:?}"),
        None if in_own_checkout => match git(&root, &["rev-parse", "HEAD"]) {
            Some(c) if hex_commit(&c) => (c, true),
            _ => ("unknown".to_string(), false),
        },
        None => ("unknown".to_string(), false),
    };

    let epoch: u64 = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(s) if !s.trim().is_empty() => s
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("SOURCE_DATE_EPOCH must be unix seconds, got {s:?}")),
        _ if from_git => git(&root, &["show", "-s", "--format=%ct", "HEAD"])
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        _ => 0,
    };

    if in_own_checkout {
        // Re-stamp when HEAD moves (commit, checkout, branch update).
        for p in ["HEAD", "packed-refs"] {
            watch(&root, p);
        }
        if let Some(r) = git(&root, &["symbolic-ref", "-q", "HEAD"]) {
            watch(&root, &r);
        }
    }

    println!("cargo:rustc-env=GAIA_CORTEX_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=GAIA_CORTEX_BUILD_EPOCH={epoch}");
}

/// Emit rerun-if-changed for a git path, only if it exists (a missing path
/// would make cargo re-run this script on every build).
fn watch(root: &Path, git_path: &str) {
    if let Some(p) = git(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            git_path,
        ],
    ) {
        if Path::new(&p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
}
