// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Guard (slot 211, int5-candle-repocheck): a `[profile.release.package.<crate>]`
//! override for a workspace crate that owns binaries must repeat `strip = true`.
//! Cargo builds the root unit of such a crate with the profile *override*, and
//! an override without `strip` falls back to cargo's default (`strip =
//! "debuginfo"`), so its binaries keep `.symtab` and fail
//! `scripts/size-budget.sh` (found on galion-voice-serve, integration-5 0d68f46).

use std::path::Path;

/// `(crate name, section body)` for every `[profile.release.package.X]`.
fn package_overrides(manifest: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut cur: Option<usize> = None;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            cur = t
                .strip_prefix("[profile.release.package.")
                .and_then(|r| r.strip_suffix(']'))
                .map(|name| {
                    out.push((name.trim_matches('"').to_string(), Vec::new()));
                    out.len() - 1
                });
        } else if let Some(i) = cur {
            if !t.is_empty() && !t.starts_with('#') {
                out[i].1.push(t.replace(' ', ""));
            }
        }
    }
    out
}

/// True when `crates/<name>` is a workspace crate with at least one binary.
fn has_bins(root: &Path, name: &str) -> bool {
    let dir = root.join("crates").join(name);
    dir.join("src/main.rs").is_file()
        || dir.join("src/bin").is_dir()
        || std::fs::read_to_string(dir.join("Cargo.toml"))
            .map(|m| m.contains("[[bin]]"))
            .unwrap_or(false)
}

fn violations(root: &Path, manifest: &str) -> Vec<String> {
    package_overrides(manifest)
        .into_iter()
        .filter(|(name, _)| has_bins(root, name))
        .filter(|(_, body)| {
            !body
                .iter()
                .any(|l| l == "strip=true" || l == "strip=\"symbols\"")
        })
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn bin_crate_overrides_keep_strip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let names: Vec<String> = package_overrides(&manifest)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(
        names.iter().any(|n| n == "galion-voice-cpu"),
        "override list: {names:?}"
    );
    let bad = violations(&root, &manifest);
    assert!(
        bad.is_empty(),
        "add `strip = true` to [profile.release.package.X] for: {bad:?}"
    );
}

#[test]
fn guard_catches_a_missing_strip() {
    // Negative control: the integration-5 0d68f46 shape must be reported.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let old = "[profile.release]\nstrip = true\n[profile.release.package.galion-voice-cpu]\nopt-level = 3\n\n[patch.crates-io]\n";
    assert_eq!(violations(&root, old), vec!["galion-voice-cpu".to_string()]);
    // Library-only crates (no root binary unit) are not affected.
    let lib = "[profile.release.package.galion-chatterbox]\nopt-level = 3\n";
    assert!(violations(&root, lib).is_empty());
    let fixed = "[profile.release.package.galion-voice-cpu]\nopt-level = 3\nstrip = true\n";
    assert!(violations(&root, fixed).is_empty());
}
