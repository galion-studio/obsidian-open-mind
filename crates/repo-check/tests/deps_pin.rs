// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Unit tests of the `deps` check (swarm/serde-order-pin): serde_json's
//! feature set is pinned once in the root `[workspace.dependencies]`, so a
//! `cargo build -p X` and a `cargo build --workspace` emit the same JSON key
//! order. Plus one test that runs the check over this repository.

use repo_check::*;
use std::path::Path;

const TOML_HDR: &str =
    "# SPDX-License-Identifier: MIT\n# SPDX-FileCopyrightText: 3000 Galion / galion-studio\n";

fn deps(path: &str, body: &str) -> Vec<Finding> {
    let text = format!("{TOML_HDR}{body}");
    scan_text(path, &text)
        .into_iter()
        .filter(|f| f.check == Check::Deps)
        .collect()
}

const ROOT_OK: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\nserde = { version = \"1\" }\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\n";

#[test]
fn root_with_pin_is_clean() {
    assert!(deps("Cargo.toml", ROOT_OK).is_empty());
}

#[test]
fn root_without_feature_is_flagged() {
    let body = ROOT_OK.replace(", features = [\"preserve_order\"]", "");
    let f = deps("Cargo.toml", &body);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].level, Level::Error);
    assert_eq!(f[0].line, 8);
    assert!(f[0].msg.contains("preserve_order"));
}

#[test]
fn root_feature_only_in_comment_is_flagged() {
    let body = ROOT_OK.replace(
        ", features = [\"preserve_order\"] }",
        " } # preserve_order later",
    );
    assert_eq!(deps("Cargo.toml", &body).len(), 1);
}

#[test]
fn root_missing_entry_is_flagged_once() {
    let body = "[workspace]\n\n[workspace.dependencies]\nserde = \"1\"\n";
    let f = deps("Cargo.toml", body);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].line, 0);
}

#[test]
fn plain_package_root_is_not_judged() {
    // A root Cargo.toml without [workspace.dependencies] is not this repo's
    // layout; nothing to pin.
    assert!(deps("Cargo.toml", "[package]\nname = \"x\"\n").is_empty());
}

#[test]
fn member_workspace_true_is_clean() {
    for body in [
        "[dependencies]\nserde_json = { workspace = true }\n",
        "[dependencies]\nserde_json = {workspace=true, optional = true}\n",
        "[dev-dependencies]\nserde_json = { workspace = true } # preserve_order: pinned in the root\n",
        "[dependencies]\nserde_json.workspace = true\n",
        "[features]\nserde-json = [\"dep:serde\", \"dep:serde_json\"]\n",
        "[dependencies]\nserde_jsonc = \"1\"\n",
    ] {
        assert!(deps("crates/a/Cargo.toml", body).is_empty(), "{body}");
    }
}

#[test]
fn member_adding_features_is_flagged() {
    for body in [
        "[dependencies]\nserde_json = { workspace = true, features = [\"preserve_order\"] }\n",
        "[dev-dependencies]\nserde_json = { workspace = true, features = [\"preserve_order\"] }\n",
        "[dependencies]\nserde_json = { workspace = true, features = [\"arbitrary_precision\"] }\n",
        "[dependencies]\nserde_json.features = [\"preserve_order\"]\n",
    ] {
        let f = deps("crates/a/Cargo.toml", body);
        assert_eq!(f.len(), 1, "{body}: {f:?}");
        assert_eq!(f[0].line, 4);
    }
}

#[test]
fn member_own_version_is_flagged() {
    for body in [
        "[dependencies]\nserde_json = \"1\"\n",
        "[dependencies]\nserde_json = { version = \"1.0.154\" }\n",
    ] {
        assert_eq!(deps("crates/a/Cargo.toml", body).len(), 1, "{body}");
    }
}

#[test]
fn member_table_form_is_flagged() {
    let body = "[dependencies.serde_json]\nworkspace = true\nfeatures = [\"preserve_order\"]\n";
    let f = deps("crates/a/Cargo.toml", body);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].line, 3);
}

#[test]
fn allow_marker_and_out_of_scope_paths() {
    let body = format!("[dependencies]\nserde_json = \"1\" # {ALLOW_MARKER}\n");
    assert!(deps("crates/a/Cargo.toml", &body).is_empty());
    // Separate workspaces and vendored code keep their own choice.
    let own = "[dependencies]\nserde_json = \"1\"\n";
    for p in [
        "edge/openmind-node/Cargo.toml",
        "contrib/x/Cargo.toml",
        "crates/a/sub/Cargo.toml",
        "crates/a/Cargo.lock",
    ] {
        assert!(deps(p, own).is_empty(), "{p}");
    }
}

#[test]
fn deps_check_is_listed() {
    assert_eq!(Check::parse("deps"), Some(Check::Deps));
    assert_eq!(Check::Deps.default_level(), Level::Error);
    assert!(Check::ALL.contains(&Check::Deps));
}

/// The repository itself passes: root pins preserve_order, no member adds
/// serde_json features. Cheap (reads ~60 Cargo.toml files), so not gated.
#[test]
fn this_repo_has_one_pin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec!["Cargo.toml".to_string()];
    for e in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        let rel = format!("crates/{}/Cargo.toml", e.file_name().to_string_lossy());
        if root.join(&rel).is_file() {
            files.push(rel);
        }
    }
    assert!(files.len() > 40, "{}", files.len());
    let mut bad = Vec::new();
    for rel in &files {
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        bad.extend(
            scan_text(rel, &text)
                .into_iter()
                .filter(|f| f.check == Check::Deps),
        );
    }
    assert!(bad.is_empty(), "{bad:?}");
    let root_text = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(root_text
        .lines()
        .any(|l| l.starts_with("serde_json") && l.contains("preserve_order")));
}
