// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Shared list of release binaries and the workspace root for the two tests.

#![allow(dead_code)]

use std::path::PathBuf;

/// (package, binary) for the 12 release binaries of
/// docs/release/artifacts-refresh-20261027.md (slot 312 matrix).
pub const RELEASE_BINS: [(&str, &str); 12] = [
    ("openmind-nerve", "openmind-nerve"),
    ("hermes-gateway", "hermes-gateway"),
    ("openmind-memory", "openmind-memory"),
    ("openmind-queue", "openmind-queue"),
    ("galion-one", "galion-one"),
    ("galion-health", "galion-health"),
    ("galion-desk", "galion-desk"),
    ("link-watch", "link-watch"),
    ("tuner", "tuner"),
    ("console-static", "galion-console-static"),
    ("galion-backup", "galion-backup"),
    ("desk-hermes", "desk-hermes"),
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

/// `-p a -p b ...` for the given packages.
pub fn p_args(pkgs: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for p in pkgs {
        out.push("-p".to_string());
        out.push((*p).to_string());
    }
    out
}
