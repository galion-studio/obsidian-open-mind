// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life test of the `perms` check (slot 356, D-318-2). Gated by
//! `GALION_REAL=1`.
//!
//! 1. Box: runs the real `repo-check` binary on a throwaway tree with a 666
//!    `link.env` (exit 1, finding), then after the staged `chmod 600` (exit 0),
//!    then rollback `chmod 666` (exit 1 again).
//! 2. VPS (also needs `GALION_REAL_VPS=<ssh host>`): runs a READ-ONLY
//!    `find -printf '%m %y %p\n'` over ssh (metadata only, nothing opened,
//!    nothing written on the host) and pipes it into `--stat-list -`. Expects the
//!    live `link.env` finding while D-318-2 is unapplied.

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_repo-check");
const LINK_ENV: &str = "/opt/galion/galion-light/publish/obsidian/link.env";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

#[cfg(unix)]
#[test]
fn real_box_tree_fix_and_rollback() {
    use std::os::unix::fs::PermissionsExt;
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let d = std::env::temp_dir().join(format!("rc-real-perms-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("publish/obsidian")).unwrap();
    std::fs::write(
        d.join("LICENSE"),
        "MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n\nPermission is hereby granted, free of charge, to any person\n",
    )
    .unwrap();
    let f = d.join("publish/obsidian/link.env");
    std::fs::write(&f, "GALION_EAR=127.0.0.1:8643\n").unwrap();
    let run = |mode: u32| {
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
        let o = Command::new(BIN)
            .args(["--root", d.to_str().unwrap(), "--only", "perms"])
            .output()
            .unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    let (c, out) = run(0o666);
    eprintln!("666 -> exit {c:?}\n{out}");
    assert_eq!(c, Some(1));
    assert!(out.contains("publish/obsidian/link.env") && out.contains("world-writable"));
    assert!(!out.contains("8643"), "contents never printed");
    let (c, out) = run(0o600);
    eprintln!("600 -> exit {c:?}\n{out}");
    assert_eq!(c, Some(0));
    let (c, _) = run(0o666);
    assert_eq!(c, Some(1), "rollback restores the finding");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn real_vps_stat_list() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let Ok(host) = std::env::var("GALION_REAL_VPS") else {
        eprintln!("skipped: set GALION_REAL_VPS=<ssh host>");
        return;
    };
    // Metadata only. Prunes the live working copy and secrets dirs; never
    // opens a file. Same prune list as docs/ops/link-env-perm-stage.md.
    let remote = "find /opt/galion -xdev \\( -path /opt/galion/openmind -o -path '*/secrets' -o -name .git -o -name node_modules -o -name target -o -name venv -o -name __pycache__ \\) -prune -o -printf '%m %y %p\\n' 2>/dev/null";
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            &host,
            remote,
        ])
        .stdin(Stdio::null())
        .output()
        .expect("ssh");
    let list = String::from_utf8_lossy(&o.stdout).into_owned();
    let lines = list.lines().count();
    eprintln!("VPS stat list: {lines} entries");
    assert!(lines > 1000, "listing too small: {lines}");
    let mut child = Command::new(BIN)
        .args(["--stat-list", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(list.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let json = String::from_utf8_lossy(&out.stdout).into_owned();
    let errors = json.matches("\"level\":\"error\"").count();
    let warns = json.matches("\"level\":\"warn\"").count();
    eprintln!(
        "exit {:?}, errors {errors}, warnings {warns}",
        out.status.code()
    );
    for item in json.split("{\"check\"").skip(1) {
        let file = item
            .split("\"file\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("");
        let level = if item.contains("\"level\":\"error\"") {
            "error"
        } else {
            "warn"
        };
        eprintln!("  {level} {file}");
    }
    if json.contains(LINK_ENV) {
        assert_eq!(out.status.code(), Some(1));
        eprintln!("D-318-2 still unapplied: {LINK_ENV} flagged");
    } else {
        eprintln!("{LINK_ENV} not flagged (fix applied or file moved)");
    }
}
