// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Guard for the Pi 5 musl seed: profile pi-seed is opt-level 3, the
//! workspace release profile stays opt-level "z", the workflow builds both
//! musl targets and uploads the seed tarball, and every named size-budget
//! binary has a --help/--version probe line.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Body of a toml table, from the header line up to the next `[` header.
fn section(text: &str, header: &str) -> String {
    let mut out = Vec::new();
    let mut on = false;
    for line in text.lines() {
        if on {
            if line.starts_with('[') {
                break;
            }
            out.push(line);
        } else if line == header {
            on = true;
        }
    }
    assert!(on, "missing {header}");
    out.join("\n")
}

fn budget_bins(tsv: &str) -> Vec<String> {
    tsv.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let name = line.split_whitespace().next().unwrap_or("");
            if name.is_empty() || name == "default" {
                None
            } else {
                Some(name.to_string())
            }
        })
        .collect()
}

fn probes(script: &str) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for line in script.lines() {
        let Some(rest) = line.strip_prefix("# probe: ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let name = parts.next().expect("probe name").to_string();
        let flags: Vec<String> = parts.map(str::to_string).collect();
        assert!(!flags.is_empty(), "{name} has no flags");
        for flag in &flags {
            assert!(
                flag == "--help" || flag == "--version",
                "{name} probe {flag}"
            );
        }
        out.push((name, flags));
    }
    out
}

#[test]
fn license_header_is_the_galion_mit_text() {
    let lic = read("LICENSE");
    assert!(
        lic.starts_with("MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n"),
        "LICENSE opening changed"
    );
}

#[test]
fn pi_seed_profile_is_opt_level_3_and_release_stays_z() {
    let cargo = read("Cargo.toml");
    let release = section(&cargo, "[profile.release]");
    assert!(release.contains("opt-level = \"z\""));
    assert!(release.contains("lto = true"));
    assert!(release.contains("codegen-units = 1"));
    assert!(release.contains("strip = true"));
    assert!(!release.contains("opt-level = 3"));

    let seed = section(&cargo, "[profile.pi-seed]");
    assert!(seed.contains("inherits = \"release\""));
    assert!(seed.contains("opt-level = 3"));
    assert!(seed.contains("lto = true"));
    assert!(seed.contains("codegen-units = 1"));
    assert!(seed.contains("panic = \"abort\""));
    assert!(seed.contains("strip = true"));
}

#[test]
fn workflow_builds_both_targets_and_uploads_the_tarball() {
    let w = read(".github/workflows/pi-seed.yml");
    assert!(w.contains("aarch64-unknown-linux-musl"));
    assert!(w.contains("x86_64-unknown-linux-musl"));
    assert!(w.contains("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER: rust-lld"));
    assert!(w.contains("qemu-user"));
    assert!(w.contains("scripts/linux/pi-seed.sh"));
    assert!(w.contains("contents: read"));
    assert!(w.contains("obsidian-pi5-seed-${{ github.sha }}"));
    assert!(w.contains("dist/obsidian-pi5-seed-${{ github.sha }}.tar.gz"));
    assert!(!w.contains("contents: write"));
}

#[test]
fn every_budget_binary_has_a_help_or_version_probe() {
    let bins = budget_bins(&read("config/release/size-budget.tsv"));
    let script = read("scripts/linux/pi-seed.sh");
    let rows = probes(&script);
    let mut names: Vec<String> = rows.iter().map(|(n, _)| n.clone()).collect();
    let mut expected = bins.clone();
    names.sort();
    expected.sort();
    assert_eq!(
        names, expected,
        "probe lines must match the size-budget names"
    );
    assert!(script.contains("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld"));
    assert!(script.contains("WantedBy=default\\.target"));
    assert!(script.contains("systemd/user"));
    assert!(script.contains("\"$root/LICENSE\""));
    assert!(read("docs/PI5-SEED.md").contains("qemu-user"));
    assert!(read("docs/PI5-SEED.md").contains("not been run on Raspberry Pi 5"));
}
