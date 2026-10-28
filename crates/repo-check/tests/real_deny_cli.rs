// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of `repo-check --deny-file` (gated: `GALION_REAL=1`, slot
//! 428 publish-gates-main; no network, no ports).
//!
//! 1. Runs the real binary on this repository with a deny file in a temp dir
//!    outside the tree holding a token that IS in the tracked tree (built at
//!    runtime from this file's own name) and a TEST-NET address: exit 1, the
//!    hit names this file, the output never contains either value.
//! 2. A deny file inside `--root` is refused with exit 2.
//! 3. If `GALION_DENY_FILE` points at the operator list (kept outside the
//!    repo, never committed), exports `git archive HEAD` (the public tree,
//!    honouring export-ignore) and requires 0 deny errors, and that no entry
//!    appears in the output.

use std::path::{Path, PathBuf};
use std::process::Command;

fn gated() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return false;
    }
    true
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_repo-check"))
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-deny-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn real_deny_cli_on_repo() {
    if !gated() {
        return;
    }
    let t = tmpdir("cli");
    // A token present in the tracked tree: this test file's own stem.
    let present = ["real", "deny", "cli"].join("_");
    let absent = ["203", "0", "113", "201"].join(".");
    let deny = t.join("deny.txt");
    std::fs::write(&deny, format!("# test list\n{present}\n{absent}\n")).unwrap();
    let out = bin()
        .arg("--root")
        .arg(root())
        .args(["--only", "deny", "--deny-file"])
        .arg(&deny)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    eprintln!("{stdout}");
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(stdout.contains("crates/repo-check/tests/real_deny_cli.rs:"));
    assert!(stdout.contains("entry #1"));
    assert!(!stdout.contains("entry #2"), "absent entry reported");
    for line in stdout.lines() {
        assert!(
            !line.contains(&present)
                || line.starts_with("error deny     crates/repo-check/tests/real_deny_cli.rs:"),
            "value leaked: {line}"
        );
        assert!(!line.contains(&absent), "value leaked: {line}");
    }
    // The only place the value appears is the path; strip it and re-check.
    let scrubbed = stdout.replace("real_deny_cli.rs", "");
    assert!(!scrubbed.contains(&present), "value leaked in message");

    // JSON mode carries the same, also without the value.
    let js = bin()
        .arg("--root")
        .arg(root())
        .args(["--json", "--only", "deny", "--deny-file"])
        .arg(&deny)
        .output()
        .unwrap();
    let j = String::from_utf8_lossy(&js.stdout).replace("real_deny_cli.rs", "");
    assert!(j.contains("\"check\":\"deny\""));
    assert!(!j.contains(&present) && !j.contains(&absent));

    // Inside-root deny file is refused.
    let inside = root().join("target-deny-inside.txt");
    std::fs::write(&inside, "x-y-z-w\n").unwrap();
    let r = bin()
        .arg("--root")
        .arg(root())
        .arg("--deny-file")
        .arg(&inside)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&inside);
    assert_eq!(r.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&r.stderr).contains("inside --root"));
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn real_operator_list_public_tree_clean() {
    if !gated() {
        return;
    }
    let Some(list) = std::env::var_os("GALION_DENY_FILE") else {
        eprintln!("skipped: GALION_DENY_FILE not set");
        return;
    };
    let t = tmpdir("export");
    let tar = t.join("tree.tar");
    let st = Command::new("git")
        .current_dir(root())
        .args(["archive", "--format=tar", "-o"])
        .arg(&tar)
        .arg("HEAD")
        .status()
        .unwrap();
    assert!(st.success());
    let tree = t.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    assert!(Command::new("tar")
        .arg("-xf")
        .arg(&tar)
        .arg("-C")
        .arg(&tree)
        .status()
        .unwrap()
        .success());
    let out = bin()
        .arg("--root")
        .arg(&tree)
        .args(["--only", "deny", "--deny-file"])
        .arg(&list)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    eprintln!("{stdout}");
    let entries = repo_check::parse_deny_list(&std::fs::read_to_string(&list).unwrap());
    assert!(!entries.is_empty());
    for e in &entries {
        assert!(
            !stdout.contains(e.as_str()),
            "an entry value reached the output"
        );
    }
    let _ = std::fs::remove_dir_all(&t);
    assert_eq!(out.status.code(), Some(0), "public tree has deny hits");
}
