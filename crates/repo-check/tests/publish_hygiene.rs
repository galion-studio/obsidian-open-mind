// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Crate publish hygiene (slot 296, swarm/crate-publish-hygiene): every
//! workspace crate sets `publish = false` and every path dependency carries a
//! real version requirement that matches the target crate. That is what lets
//! `deny.toml` run `wildcards = "deny"` with `allow-wildcard-paths = false`.
//!
//! The static checks always run. `GALION_REAL=1` also runs the real
//! `cargo-deny` binary (if on PATH) over this repo and over a throwaway
//! two-crate workspace as a negative control.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// Manifests in scope: crates/*/Cargo.toml plus edge/openmind-node.
fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(root.join("crates"))
        .expect("crates dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("Cargo.toml"))
        .filter(|p| p.is_file())
        .collect();
    let edge = root.join("edge/openmind-node/Cargo.toml");
    if edge.is_file() {
        v.push(edge);
    }
    v.sort();
    v
}

/// Value of `key = "..."` inside a single-line inline table / line.
fn quoted(line: &str, key: &str) -> Option<String> {
    let mut rest = line;
    while let Some(i) = rest.find(key) {
        let before = rest[..i].chars().last();
        let after = rest[i + key.len()..].trim_start();
        if matches!(before, None | Some(' ') | Some('{') | Some(',')) && after.starts_with('=') {
            let v = after[1..].trim_start();
            let v = v.strip_prefix('"')?;
            return v.find('"').map(|e| v[..e].to_string());
        }
        rest = &rest[i + key.len()..];
    }
    None
}

/// `publish = false` present in the [package] table?
pub fn has_publish_false(text: &str) -> bool {
    let mut in_pkg = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            in_pkg = t == "[package]";
            continue;
        }
        if in_pkg && t.replace(' ', "") == "publish=false" {
            return true;
        }
    }
    false
}

/// Path deps in dependency tables: (line no, dep name, path, version req).
pub fn path_deps(text: &str) -> Vec<(usize, String, String, Option<String>)> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim();
        if t.starts_with('[') {
            in_deps = t
                .trim_matches(|c| c == '[' || c == ']')
                .ends_with("dependencies");
            continue;
        }
        if !in_deps || t.starts_with('#') {
            continue;
        }
        if let Some(p) = quoted(t, "path") {
            let name = t.split('=').next().unwrap_or("").trim().to_string();
            out.push((i + 1, name, p, quoted(t, "version")));
        }
    }
    out
}

fn crate_version(root_manifest: &str, crate_manifest: &str) -> String {
    let ws = root_manifest
        .lines()
        .skip_while(|l| l.trim() != "[workspace.package]")
        .find_map(|l| {
            l.trim()
                .strip_prefix("version = \"")
                .map(|v| v.trim_end_matches('"').to_string())
        })
        .expect("workspace version");
    for l in crate_manifest.lines() {
        let t = l.trim();
        if t.starts_with("version.workspace") {
            return ws;
        }
        if let Some(v) = t.strip_prefix("version = \"") {
            return v.trim_end_matches('"').to_string();
        }
    }
    ws
}

#[test]
fn parser_self_test() {
    let t = "[package]\nname = \"a\"\npublish = false\n\n[dependencies]\nb = { path = \"../b\" }\nc = { path = \"../c\", version = \"0.1.0\" }\nserde = { workspace = true }\n[lib]\npath = \"src/lib.rs\"\n";
    assert!(has_publish_false(t));
    assert!(!has_publish_false(
        "[package]\nname = \"a\"\n[x]\npublish = false\n"
    ));
    let d = path_deps(t);
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(d[0].3, None);
    assert_eq!(d[1].3.as_deref(), Some("0.1.0"));
}

#[test]
fn every_crate_sets_publish_false() {
    let root = repo_root();
    let ms = manifests(&root);
    assert!(ms.len() >= 60, "found only {} manifests", ms.len());
    let bad: Vec<_> = ms
        .iter()
        .filter(|m| !has_publish_false(&fs::read_to_string(m).unwrap()))
        .collect();
    assert!(bad.is_empty(), "missing publish = false: {bad:?}");
}

#[test]
fn every_path_dep_has_matching_version() {
    let root = repo_root();
    let root_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let mut n = 0;
    let mut bad = Vec::new();
    for m in manifests(&root) {
        let text = fs::read_to_string(&m).unwrap();
        for (line, name, p, ver) in path_deps(&text) {
            n += 1;
            let target = m.parent().unwrap().join(&p).join("Cargo.toml");
            let want = crate_version(&root_toml, &fs::read_to_string(&target).unwrap());
            match ver {
                None => bad.push(format!("{}:{line} {name}: no version", m.display())),
                Some(v) if v != want => {
                    bad.push(format!("{}:{line} {name}: {v} != {want}", m.display()))
                }
                _ => {}
            }
        }
    }
    assert!(n >= 100, "only {n} path deps found; parser broken?");
    assert!(bad.is_empty(), "{bad:#?}");
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn deny_bin() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("cargo-deny"))
            .find(|c| c.is_file())
    })
}

fn deny_bans(dir: &Path, bin: &Path) -> (bool, String) {
    let o = Command::new(bin)
        .args(["--offline", "check", "bans"])
        .current_dir(dir)
        .output()
        .expect("run cargo-deny");
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.success(), s)
}

#[test]
fn real_cargo_deny_bans_repo() {
    if !real() {
        return;
    }
    let bin = deny_bin().expect("GALION_REAL=1 needs cargo-deny on PATH");
    let (ok, err) = deny_bans(&repo_root(), &bin);
    assert!(ok, "cargo-deny bans failed:\n{err}");
    assert!(err.contains("bans ok"), "{err}");
}

#[test]
fn real_cargo_deny_negative_control() {
    if !real() {
        return;
    }
    let bin = deny_bin().expect("GALION_REAL=1 needs cargo-deny on PATH");
    let dir = std::env::temp_dir().join(format!("publish-hygiene-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for c in ["a", "b"] {
        fs::create_dir_all(dir.join(c).join("src")).unwrap();
        fs::write(dir.join(c).join("src/lib.rs"), "").unwrap();
    }
    fs::write(
        dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\"]\n",
    )
    .unwrap();
    fs::copy(repo_root().join("deny.toml"), dir.join("deny.toml")).unwrap();
    let pkg = |n: &str, extra: &str| {
        format!("[package]\nname = \"{n}\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT\"\npublish = false\n{extra}")
    };
    fs::write(dir.join("b/Cargo.toml"), pkg("b", "")).unwrap();
    let a = dir.join("a/Cargo.toml");
    // Unversioned path dep: must be denied even with publish = false.
    fs::write(&a, pkg("a", "[dependencies]\nb = { path = \"../b\" }\n")).unwrap();
    let (ok_bad, err_bad) = deny_bans(&dir, &bin);
    // Versioned path dep: must pass.
    fs::write(
        &a,
        pkg(
            "a",
            "[dependencies]\nb = { path = \"../b\", version = \"0.1.0\" }\n",
        ),
    )
    .unwrap();
    let (ok_good, err_good) = deny_bans(&dir, &bin);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !ok_bad && err_bad.contains("wildcard"),
        "negative control passed:\n{err_bad}"
    );
    assert!(ok_good, "versioned path dep denied:\n{err_good}");
}
