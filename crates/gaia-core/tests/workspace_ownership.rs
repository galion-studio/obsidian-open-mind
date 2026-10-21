// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Fails if any workspace crate is not in the Gaia ownership registry, or if
//! the registry names a crate that is not in the workspace.
//!
//! The workspace is `members = ["crates/*"]`, so the crate list is every
//! `crates/<dir>/Cargo.toml` and its `[package] name`.

use gaia_core::ownership::{crate_owner, CRATES};
use std::collections::BTreeSet;
use std::path::Path;

fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = t.strip_prefix("name") {
                let v = rest.trim_start().strip_prefix('=')?.trim();
                return Some(v.trim_matches('"').to_string());
            }
        }
    }
    None
}

fn workspace_crates() -> BTreeSet<String> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(&crates).expect("read crates/") {
        let p = e.expect("dir entry").path().join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&p) {
            let name = package_name(&text).unwrap_or_else(|| panic!("no [package] name in {p:?}"));
            out.insert(name);
        }
    }
    out
}

#[test]
fn every_workspace_crate_is_mapped() {
    let ws = workspace_crates();
    assert!(
        ws.len() >= 46,
        "expected at least 46 crates, found {}",
        ws.len()
    );
    let unmapped: Vec<_> = ws.iter().filter(|n| crate_owner(n).is_none()).collect();
    assert!(
        unmapped.is_empty(),
        "crates not mapped to a Gaia system/function in gaia_core::ownership::CRATES: {unmapped:?}"
    );
}

#[test]
fn registry_has_no_stale_rows() {
    let ws = workspace_crates();
    let stale: Vec<_> = CRATES
        .iter()
        .map(|r| r.name)
        .filter(|n| !ws.contains(*n))
        .collect();
    assert!(stale.is_empty(), "registry rows with no crate: {stale:?}");
}

#[test]
fn package_name_parser() {
    assert_eq!(
        package_name("[package]\nname = \"x-y\"\n[dependencies]\nname = \"z\"").as_deref(),
        Some("x-y")
    );
    assert_eq!(package_name("[workspace]\nname = \"w\""), None);
}
