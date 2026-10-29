// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Shared list of release binaries and the workspace root for the release tests.

#![allow(dead_code)]

use std::path::PathBuf;

/// 200 MB (209715200 B): the release size cap of every binary (`default` row
/// of config/release/size-budget.tsv, read by `scripts/size-budget.sh`).
/// Owner decision t416u (2026-10-29 CET +01:00) raised it from 2 MiB (slots
/// 329-373).
pub const SIZE_LIMIT: u64 = 200 * 1024 * 1024;

/// One release binary: its package, binary name, the package's own features
/// that the release build turns on, and its size cap in bytes.
#[derive(Clone, Copy, Debug)]
pub struct ReleaseBin {
    pub pkg: &'static str,
    pub bin: &'static str,
    /// Features of `pkg` itself, passed as `--features <pkg>/<f>`.
    pub features: &'static [&'static str],
    pub budget: u64,
}

const fn rb(pkg: &'static str, bin: &'static str) -> ReleaseBin {
    ReleaseBin {
        pkg,
        bin,
        features: &[],
        budget: SIZE_LIMIT,
    }
}

/// The 12 release binaries of docs/release/artifacts-refresh-20261027.md
/// (slot 312 matrix) plus the two live units built from their own branches
/// until now (D-325-2, slot 337): gaia-cortex with `llm-advisor` (live
/// `gaia-cortex-73583ab-llm`) and galion-console-serve (second binary of
/// console-static, live `galion-console-serve-*`).
pub const RELEASE_BINS: [ReleaseBin; 14] = [
    rb("openmind-nerve", "openmind-nerve"),
    rb("hermes-gateway", "hermes-gateway"),
    rb("openmind-memory", "openmind-memory"),
    rb("openmind-queue", "openmind-queue"),
    rb("galion-one", "galion-one"),
    rb("galion-health", "galion-health"),
    rb("galion-desk", "galion-desk"),
    rb("link-watch", "link-watch"),
    rb("tuner", "tuner"),
    rb("console-static", "galion-console-static"),
    rb("galion-backup", "galion-backup"),
    rb("desk-hermes", "desk-hermes"),
    ReleaseBin {
        features: &["llm-advisor"],
        ..rb("gaia-cortex", "gaia-cortex")
    },
    rb("console-static", "galion-console-serve"),
];

pub const TARGETS: [&str; 2] = ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"];

pub fn workspace_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or(here)
}

pub fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Release packages, each once, in RELEASE_BINS order.
pub fn release_packages() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for r in RELEASE_BINS {
        if !out.contains(&r.pkg) {
            out.push(r.pkg);
        }
    }
    out
}

/// `-p a -p b ...` plus `--features pkg/f,...` for the release features of the
/// selected packages (cargo rejects `pkg/f` for a package that is not selected).
pub fn p_args(pkgs: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut feats: Vec<String> = Vec::new();
    for p in pkgs {
        out.push("-p".to_string());
        out.push((*p).to_string());
        for r in RELEASE_BINS.iter().filter(|r| r.pkg == *p) {
            for f in r.features {
                let pf = format!("{p}/{f}");
                if !feats.contains(&pf) {
                    feats.push(pf);
                }
            }
        }
    }
    if !feats.is_empty() {
        out.push("--features".to_string());
        out.push(feats.join(","));
    }
    out
}
