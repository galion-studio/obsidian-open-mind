// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The Bookworm GNU cross-build reads the install script's binary list and
//! labels probe timings as a CI runner measurement.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

const LABEL: &str = "CI runner, not a Pi 5";

fn install_bins(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut grab = false;
    for line in text.lines() {
        if line.starts_with("bins=(") {
            grab = true;
            continue;
        }
        if grab {
            if line.starts_with(')') {
                break;
            }
            let name = line.trim().split('#').next().unwrap_or("").trim();
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

#[test]
fn license_line_is_unchanged() {
    let lic = read("LICENSE");
    assert!(
        lic.starts_with("MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n"),
        "LICENSE opening changed"
    );
}

#[test]
fn gnu_job_reuses_the_install_list_and_labels_the_runner() {
    let install = read("scripts/linux/pi-bookworm-install.sh");
    let script = read("scripts/linux/pi-bookworm-gnu.sh");
    let workflow = read(".github/workflows/pi-bookworm-gnu.yml");
    let doc = read("docs/PI-BOOKWORM.md");
    let bins = install_bins(&install);
    assert!(bins.len() >= 2, "install script binary list is empty");
    assert!(bins.iter().any(|name| name == "openmind-desktop"));
    assert!(script.contains("scripts/linux/pi-bookworm-install.sh"));
    assert!(script.contains("scripts/install-zig.sh"));
    assert!(script.contains("aarch64-unknown-linux-gnu"));
    assert!(script.contains(LABEL));
    assert!(
        !script.contains("pi-seed.sh"),
        "the gnu script must call the zig installer, not rebuild the musl seed"
    );
    assert!(!script.contains("/opt/obsidian"));
    assert!(workflow.contains(LABEL));
    assert!(workflow.contains("scripts/linux/pi-bookworm-gnu.sh build"));
    assert!(workflow.contains("scripts/linux/pi-bookworm-gnu.sh measure"));
    assert!(workflow.contains("aarch64-unknown-linux-gnu"));
    assert!(workflow.contains("contents: read"));
    assert!(!workflow.contains("contents: write"));
    assert!(!workflow.contains("actions: write"));
    assert!(doc.contains(LABEL));
    assert!(doc.contains("scripts/linux/pi-bookworm-gnu.sh"));
    assert!(doc.contains("scripts/linux/pi-bookworm-install.sh"));
    assert!(doc.contains("scripts/install-zig.sh"));
    assert!(doc.contains("not a Raspberry Pi 5 result"));
}

#[cfg(unix)]
fn run(args: &[&str]) -> std::process::Output {
    Command::new(root().join("scripts/linux/pi-bookworm-gnu.sh"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run gnu script")
}

#[cfg(unix)]
#[test]
fn list_matches_the_install_script_and_usage_exits_are_stable() {
    let listed = run(&["list"]);
    assert!(
        listed.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let names: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    assert_eq!(
        names,
        install_bins(&read("scripts/linux/pi-bookworm-install.sh"))
    );

    let help = run(&["--help"]);
    assert!(help.status.success());
    let bad = run(&["nope"]);
    assert_eq!(bad.status.code(), Some(2));
}
