// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The `openmind` check (D-391-2): git writes aimed at the live VPS tree are
//! errors, cd / read-only git there are warnings. Every sample line is built
//! from [`LIVE_TREE`] at runtime so this file does not flag itself.

use repo_check::*;

fn om(path: &str, line: &str) -> Vec<(Level, usize)> {
    let mut out = Vec::new();
    openmind_writes(path, line, &mut out);
    out.into_iter().map(|f| (f.level, f.line)).collect()
}

fn t(s: &str) -> String {
    s.replace("@T", LIVE_TREE)
}

#[test]
fn git_writes_are_errors() {
    for l in [
        "git -C @T commit -m x",
        "git -C @T add docs && git -C @T commit -m y",
        "sudo git -C \"@T\" checkout swarm/overnight-loop-20261027",
        "git -c user.name=x -C @T/ reset --hard origin/x",
        "git --git-dir=@T/.git --work-tree=@T fetch origin",
        "git --work-tree @T pull",
        "cd @T && git commit -am report",
        "pushd @T; git push origin HEAD",
        "GIT_DIR=@T/.git git log",
        "export GIT_WORK_TREE=@T",
        "git clone https://github.com/x/y.git @T",
        "/usr/bin/git -C @T stash",
        "git -C @T remote add origin x",
        "git -C @T branch -D old",
        "git -C @T config user.email a@b",
        "git -C @T worktree add /tmp/w",
        "`git -C @T checkout --detach 202887d5`",
    ] {
        let f = om("scripts/x.sh", &t(l));
        assert!(
            f.iter().any(|(lv, _)| *lv == Level::Error),
            "expected error for {l:?}: {f:?}"
        );
    }
}

#[test]
fn reads_and_cd_are_warnings() {
    for l in [
        "git -C @T rev-parse HEAD",
        "GIT_OPTIONAL_LOCKS=0 git -C @T log -g --oneline",
        "git -C @T status --porcelain",
        "git -C @T remote -v",
        "git -C @T branch --show-current",
        "git -C @T config --get remote.origin.url",
        "cd @T",
        "Setup: `cd @T/release`.",
        "WorkingDirectory=@T",
        "    let c = Command::new(\"x\").current_dir(\"@T\");",
    ] {
        let f = om("docs/x.md", &t(l));
        assert!(!f.is_empty(), "expected a finding for {l:?}");
        assert!(
            f.iter().all(|(lv, _)| *lv == Level::Warn),
            "expected warn only for {l:?}: {f:?}"
        );
    }
}

#[test]
fn unrelated_mentions_are_clean() {
    for l in [
        "ExecStart=@T/dist/bin-ws/galion-one",
        "the live tree @T stays untouched",
        "git -C /opt/galion/swarm-reports-git commit -m x",
        "git -C @T-old commit -m x",
        "cd @T-backup && git commit -m x",
        "cd /workspace/swarm/wt/x && git commit -m x",
        "hermes_bin: \"@T/dist/bin/desk-hermes\",",
    ] {
        assert!(om("scripts/x.sh", &t(l)).is_empty(), "{l:?}");
    }
}

#[test]
fn allow_marker_silences_and_line_numbers_are_right() {
    let marker = concat!("repo-check", ": allow");
    let text = t(&format!(
        "# hdr\nok line\ngit -C @T commit -m x\ngit -C @T commit -m y # {marker}\n"
    ));
    assert_eq!(om("scripts/x.sh", &text), vec![(Level::Error, 3)]);
}

#[test]
fn scan_text_routes_and_list_names_it() {
    let f = scan_text("TEACHER/BOSS.md", &t("git -C @T commit -m x\n"));
    assert!(f
        .iter()
        .any(|x| x.check == Check::Openmind && x.level == Level::Error));
    assert_eq!(Check::parse("openmind"), Some(Check::Openmind));
    assert!(Check::ALL.contains(&Check::Openmind));
    assert_eq!(Check::Openmind.default_level(), Level::Error);
}

#[test]
fn read_only_table() {
    assert!(git_read_only("log", &[]));
    assert!(git_read_only("remote", &["-v"]));
    assert!(!git_read_only("remote", &["add", "o", "u"]));
    assert!(git_read_only("stash", &["list"]));
    assert!(!git_read_only("stash", &[]));
    assert!(!git_read_only("commit", &[]));
    assert!(!git_read_only("frobnicate", &[]));
}

/// Real run of the release repo-check binary on a simulated tree (GALION_REAL=1).
#[test]
fn real_openmind_rule_binary() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("om-rule-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(
        root.join("LICENSE"),
        "MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n\nPermission is hereby granted, free of charge, to any person\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/ok.md"),
        t("read: git -C @T rev-parse HEAD\n"),
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_repo-check");
    let run = |args: &[&str]| {
        std::process::Command::new(bin)
            .arg("--root")
            .arg(&root)
            .args(args)
            .output()
            .unwrap()
    };
    let o = run(&["--only", "openmind", "--json"]);
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    assert_eq!(o.status.code(), Some(0), "{s}");
    assert!(
        s.contains("\"warnings\":1") && s.contains("\"errors\":0"),
        "{s}"
    );
    std::fs::write(
        root.join("scripts/push-report.sh"),
        t("#!/bin/sh\ncd @T\ngit checkout -B swarm/r && git commit -m r\n"),
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/two.sh"),
        t("git -C @T push origin swarm/r\n"),
    )
    .unwrap();
    let o = run(&["--only", "openmind"]);
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    eprintln!("{s}");
    assert_eq!(o.status.code(), Some(1), "{s}");
    assert!(s.contains("scripts/two.sh:1"), "{s}");
    // The cd on line 2 stays in force: the git writes on line 3 are errors.
    assert!(s.contains("warn  openmind scripts/push-report.sh:2"), "{s}");
    assert!(s.contains("error openmind scripts/push-report.sh:3"), "{s}");
    let o = run(&["--only", "openmind", "--warn", "openmind"]);
    assert_eq!(o.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shell_cd_stays_in_force_until_next_cd() {
    let text = t("#!/bin/sh\ncd @T\ngit status\ngit commit -m a\ncd /tmp/own\ngit commit -m b\npushd @T/docs\ngit add x\npopd\ngit push\n");
    let f = om("scripts/r.sh", &text);
    assert_eq!(
        f,
        vec![
            (Level::Warn, 2),
            (Level::Warn, 3),
            (Level::Error, 4),
            (Level::Warn, 7),
            (Level::Error, 8),
        ]
    );
    // Not sticky in docs: each line stands alone.
    let f = om("docs/r.md", &text);
    assert!(
        f.iter()
            .all(|(l, n)| *l == Level::Warn && [2, 7].contains(n)),
        "{f:?}"
    );
}
