// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Pure checks: path resolution, argument allowlists, config refusals,
//! cargo argv. No process is started.

use hermes_tools::config::{branch_ok, worktree_ok, CodingConfig};
use hermes_tools::guard::{self, Access};
use hermes_tools::tools::Cargo;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-tools-guard-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("wt/.git")).unwrap();
    std::fs::create_dir_all(d.join("wt/src")).unwrap();
    std::fs::write(d.join("wt/src/lib.rs"), "x").unwrap();
    d.canonicalize().unwrap()
}

#[test]
fn resolve_accepts_plain_relative_paths() {
    let d = tmp("ok");
    let wt = d.join("wt");
    assert_eq!(
        guard::resolve(&wt, "src/lib.rs", Access::Read).unwrap(),
        Path::new("src/lib.rs")
    );
    assert_eq!(
        guard::resolve(&wt, "./src/./lib.rs", Access::Read).unwrap(),
        Path::new("src/lib.rs")
    );
    let abs = wt.join("src/lib.rs");
    assert!(guard::resolve(&wt, abs.to_str().unwrap(), Access::Read).is_ok());
    assert!(guard::resolve(&wt, "src/new/file.rs", Access::Write).is_ok());
}

#[test]
fn resolve_refuses_escapes() {
    let d = tmp("esc");
    let wt = d.join("wt");
    std::os::unix::fs::symlink("/etc/hostname", wt.join("link-out")).unwrap();
    std::os::unix::fs::symlink("/etc", wt.join("dir-link")).unwrap();
    let cases = [
        ("../../etc/passwd", Access::Read, "traversal"),
        ("src/../../x", Access::Write, "traversal"),
        ("/etc/shadow", Access::Read, "absolute path outside"),
        ("link-out", Access::Read, "symlink"),
        ("dir-link/passwd", Access::Read, "symlink"),
        ("dir-link/new", Access::Write, "symlink"),
        (".git/config", Access::Read, "off-limits"),
        ("sub/.git/hooks/x", Access::Write, "off-limits"),
        (".cargo/config.toml", Access::Write, "not allowed"),
        (".github/workflows/x.yml", Access::Write, "not allowed"),
        ("", Access::Read, "empty"),
        (".", Access::Write, "worktree root"),
        ("src", Access::Read, "not a regular file"),
        ("missing.rs", Access::Read, "no such file"),
        ("a\u{0}b", Access::Read, "control"),
    ];
    for (p, a, want) in cases {
        let e = guard::resolve(&wt, p, a).expect_err(p);
        assert!(e.contains(want), "{p:?}: {e}");
    }
}

#[test]
fn only_keys_and_value_checks() {
    assert!(guard::only_keys(&json!({"path": "a"}), &["path"]).is_ok());
    let e = guard::only_keys(&json!({"path": "a", "cmd": "rm -rf /"}), &["path"]).unwrap_err();
    assert!(e.contains("\"cmd\""));
    assert!(guard::only_keys(&json!("x"), &["path"]).is_err());
    assert!(guard::ident_ok("hermes-tools"));
    assert!(!guard::ident_ok("--config=x"));
    assert!(!guard::ident_ok("a b"));
    assert!(guard::test_filter_ok("tests::adds"));
    assert!(!guard::test_filter_ok("--nocapture"));
    assert!(guard::commit_message_ok("fix: add\n\nbody").is_ok());
    assert!(guard::commit_message_ok("  ").is_err());
    assert!(guard::commit_message_ok("a\u{1b}[31m").is_err());
}

#[test]
fn branch_and_worktree_rules() {
    assert!(branch_ok("swarm/fake-ticket").is_ok());
    for b in ["main", "master", "-x", "a..b", "a b", "x.lock", "HEAD", ""] {
        assert!(branch_ok(b).is_err(), "{b}");
    }
    assert!(worktree_ok(Path::new("/opt/galion/openmind"), &[]).is_err());
    assert!(worktree_ok(Path::new("/opt/galion/openmind/sub"), &[]).is_err());
    assert!(
        worktree_ok(Path::new("/opt/galion"), &[]).is_err(),
        "contains a denied root"
    );
    assert!(worktree_ok(Path::new("/etc/x"), &[]).is_err());
    assert!(worktree_ok(Path::new("/"), &[]).is_err());
    let d = tmp("wt");
    assert!(worktree_ok(&d.join("wt"), &[]).is_ok());
    assert!(
        worktree_ok(&d.join("wt"), std::slice::from_ref(&d)).is_err(),
        "extra deny"
    );
    // .git as a file (git worktree link) is refused
    std::fs::create_dir_all(d.join("wt2")).unwrap();
    std::fs::write(d.join("wt2/.git"), "gitdir: /elsewhere").unwrap();
    assert!(worktree_ok(&d.join("wt2"), &[]).is_err());
}

fn cfg_from(vars: &[(&str, String)]) -> Result<Option<CodingConfig>, String> {
    let m: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    CodingConfig::from_env(
        &move |k| m.get(k).cloned(),
        Path::new("/tmp/hermes-home-test"),
    )
}

#[test]
fn config_off_by_default_and_refuses_bad_setups() {
    assert!(cfg_from(&[]).unwrap().is_none());
    let d = tmp("cfg");
    let wt = d.join("wt").display().to_string();
    let e = cfg_from(&[("HERMES_RS_CODING_WORKTREE", wt.clone())]).unwrap_err();
    assert!(e.contains("BRANCH"), "{e}");
    let e = cfg_from(&[
        ("HERMES_RS_CODING_WORKTREE", wt.clone()),
        ("HERMES_RS_CODING_BRANCH", "main".into()),
    ])
    .unwrap_err();
    assert!(e.contains("protected"), "{e}");
    let e = cfg_from(&[
        ("HERMES_RS_CODING_WORKTREE", wt.clone()),
        ("HERMES_RS_CODING_BRANCH", "swarm/x".into()),
        ("HERMES_RS_CODING_STATE", format!("{wt}/state")),
    ])
    .unwrap_err();
    assert!(e.contains("outside the worktree"), "{e}");
    let e = cfg_from(&[
        ("HERMES_RS_CODING_WORKTREE", wt.clone()),
        ("HERMES_RS_CODING_BRANCH", "swarm/x".into()),
        ("HERMES_RS_CODING_JOBS", "16".into()),
    ])
    .unwrap_err();
    assert!(e.contains("outside 1..=4"), "{e}");
    let c = cfg_from(&[
        ("HERMES_RS_CODING_WORKTREE", wt.clone()),
        ("HERMES_RS_CODING_BRANCH", "swarm/x".into()),
    ])
    .unwrap()
    .unwrap();
    assert_eq!(c.cargo_jobs, 2);
    assert_eq!(c.nice, 19);
    assert!(!c.require_cgroup);
    assert!(c.hide.contains(&PathBuf::from("/tmp/hermes-home-test")));
    let c = cfg_from(&[
        ("HERMES_RS_CODING_WORKTREE", wt),
        ("HERMES_RS_CODING_BRANCH", "swarm/x".into()),
        ("HERMES_RS_CODING_CGROUP", "delegate".into()),
    ])
    .unwrap()
    .unwrap();
    assert!(c.cgroup_delegate && c.require_cgroup && c.cgroup_parent.is_none());
}

#[test]
fn cargo_argv_is_fixed_and_allowlisted() {
    let d = tmp("cargo");
    let tc = d.join("tc");
    std::fs::create_dir_all(tc.join("bin")).unwrap();
    std::fs::write(tc.join("bin/cargo"), "").unwrap();
    let c = cfg_from(&[
        (
            "HERMES_RS_CODING_WORKTREE",
            d.join("wt").display().to_string(),
        ),
        ("HERMES_RS_CODING_BRANCH", "swarm/x".into()),
        ("HERMES_RS_CODING_TOOLCHAIN", tc.display().to_string()),
    ])
    .unwrap()
    .unwrap();
    let v = Cargo::argv(&c, &json!({"command": "test", "test_filter": "adds"})).unwrap();
    let tail: Vec<&str> = v[4..].iter().map(String::as_str).collect();
    assert_eq!(
        tail,
        [
            "test",
            "--offline",
            "--color",
            "never",
            "-j",
            "2",
            "adds",
            "--",
            "--test-threads",
            "2"
        ]
    );
    assert!(v[0].ends_with("/nice") && v[1] == "-n" && v[2] == "19");
    let v = Cargo::argv(&c, &json!({"command": "clippy", "package": "adder"})).unwrap();
    assert!(v.ends_with(&["--all-targets", "--", "-D", "warnings"].map(String::from)));
    for bad in [
        json!({"command": "run"}),
        json!({"command": "install"}),
        json!({"command": "publish"}),
        json!({"command": "test", "args": "rm -rf /"}),
        json!({"command": "build", "package": "--config=build.rustc-wrapper='rm'"}),
        json!({"command": "build", "test_filter": "x"}),
        json!({"command": "test", "test_filter": "--nocapture"}),
        json!({"command": "build", "release": "yes"}),
        json!({"command": "build", "nocapture": true}),
        json!({"command": "test", "nocapture": "--exact"}),
        json!({"command": "rm -rf /"}),
    ] {
        assert!(Cargo::argv(&c, &bad).is_err(), "{bad}");
    }
    let c2 = cfg_from(&[
        (
            "HERMES_RS_CODING_WORKTREE",
            d.join("wt").display().to_string(),
        ),
        ("HERMES_RS_CODING_BRANCH", "swarm/x".into()),
    ])
    .unwrap()
    .unwrap();
    assert!(Cargo::argv(&c2, &json!({"command": "build"}))
        .unwrap_err()
        .contains("not configured"));
}
