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

// ---------------------------------------------------------------------------
// Slot 379 (D-356-3): staged chmod plan.
// ---------------------------------------------------------------------------

fn rc(args: &[&str], stdin: Option<&str>) -> (Option<i32>, String, String) {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut si = child.stdin.take().unwrap();
        if let Some(s) = stdin {
            si.write_all(s.as_bytes()).unwrap();
        }
    }
    let o = child.wait_with_output().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn find_list(root: &std::path::Path) -> String {
    let o = Command::new("find")
        .arg(root)
        .args(["-printf", "%m %y %u %g %p\\n"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Box: a throwaway tree with the live shape (777 sessions dirs, 666 link.env,
/// a console-dist with 666 files / 777 dirs). Build the plan, run the
/// generated guarded sh, verify (applied), run the rollback sh, verify
/// (pending again), then a drift case where the guard must refuse.
#[cfg(unix)]
#[test]
fn real_box_chmod_plan_apply_and_rollback() {
    use std::os::unix::fs::PermissionsExt;
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let d = std::env::temp_dir().join(format!("rc-real-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let set = |p: &str, m: u32| {
        std::fs::set_permissions(d.join(p), std::fs::Permissions::from_mode(m)).unwrap()
    };
    for p in [
        "arena/homes/g2-0/sessions",
        "arena/homes/g2-1/sessions",
        "galion-src/app/src/app/sessions",
        "galion-light/publish/obsidian",
        "console-dist/static/__grok",
        "console-dist/node_modules/srvx/bin",
        "tmp",
    ] {
        std::fs::create_dir_all(d.join(p)).unwrap();
    }
    for (f, m) in [
        ("galion-src/app/src/app/sessions/page.tsx", 0o777),
        ("galion-light/publish/obsidian/link.env", 0o666),
        ("console-dist/static/favicon.svg", 0o666),
        ("console-dist/static/__grok/styles.css", 0o666),
        ("console-dist/static/app.js", 0o644),
        ("console-dist/node_modules/srvx/bin/srvx.mjs", 0o777),
        ("tmp/scratch.txt", 0o666),
    ] {
        std::fs::write(d.join(f), "x").unwrap();
        set(f, m);
    }
    for p in [
        "arena/homes/g2-0/sessions",
        "arena/homes/g2-1/sessions",
        "galion-src/app/src/app/sessions",
        "console-dist/node_modules/srvx",
        "console-dist/node_modules/srvx/bin",
    ] {
        set(p, 0o777);
    }
    set("tmp", 0o1777);
    let tree = d.join("console-dist").to_string_lossy().into_owned();
    let list = find_list(&d);
    let (c, tsv, err) = rc(
        &["--stat-list", "-", "--chmod-plan", "--plan-tree", &tree],
        Some(&list),
    );
    eprintln!("plan exit {c:?}: {}", err.trim());
    assert_eq!(c, Some(0));
    let rows: Vec<&str> = tsv.lines().filter(|l| !l.starts_with('#')).collect();
    eprintln!("{} rows:\n{}", rows.len(), rows.join("\n"));
    // 3 sessions dirs, 2 srvx dirs, srvx.mjs, favicon, styles.css, link.env
    assert_eq!(rows.len(), 9);
    assert!(!tsv.contains("tmp/scratch.txt") && !tsv.contains("app.js"));
    let plan = std::env::temp_dir().join(format!("rc-real-plan-{}.tsv", std::process::id()));
    std::fs::write(&plan, &tsv).unwrap();
    let plan_s = plan.to_str().unwrap();
    let (c, out, _) = rc(&["--stat-list", "-", "--plan-verify", plan_s], Some(&list));
    eprintln!("verify before: exit {c:?} {}", out.lines().last().unwrap());
    assert_eq!(c, Some(3), "all pending");

    let (_, sh, _) = rc(
        &[
            "--stat-list",
            "-",
            "--chmod-plan",
            "--plan-tree",
            &tree,
            "--sh",
        ],
        Some(&list),
    );
    let (_, rb_sh, _) = rc(
        &[
            "--stat-list",
            "-",
            "--chmod-plan",
            "--plan-tree",
            &tree,
            "--sh",
            "--rollback",
        ],
        Some(&list),
    );
    let apply = |script: &str| {
        let o = Command::new("sh").arg("-c").arg(script).output().unwrap();
        (
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    let (c, out) = apply(&sh);
    eprintln!("apply sh exit {c:?}: {}", out.trim());
    assert_eq!(c, Some(0));
    assert!(out.contains("changed 9, skipped 0"));
    let after = find_list(&d);
    let (c, out, _) = rc(&["--stat-list", "-", "--plan-verify", plan_s], Some(&after));
    eprintln!(
        "verify after apply: exit {c:?} {}",
        out.lines().last().unwrap()
    );
    assert_eq!(c, Some(0), "all applied");
    let (c, out, _) = rc(&["--stat-list", "-", "--only", "perms"], Some(&after));
    eprintln!(
        "perms lint after apply: exit {c:?} {}",
        out.lines().last().unwrap()
    );
    assert_eq!(c, Some(0));

    let (c, out) = apply(&rb_sh);
    eprintln!("rollback sh exit {c:?}: {}", out.trim());
    assert_eq!(c, Some(0));
    let back = find_list(&d);
    let (c, _, _) = rc(&["--stat-list", "-", "--plan-verify", plan_s], Some(&back));
    assert_eq!(c, Some(3), "rollback restores every old mode");
    assert_eq!(back, list, "byte-identical stat list after rollback");

    // drift: someone changed one path after the plan was made
    set("console-dist/static/favicon.svg", 0o664);
    let (c, out) = apply(&sh);
    eprintln!("apply with drift exit {c:?}: {}", out.trim());
    assert_ne!(c, Some(0));
    assert!(out.contains("SKIP drift") && out.contains("favicon.svg"));
    assert!(out.contains("changed 8, skipped 1"));
    let (c, out, _) = rc(
        &["--stat-list", "-", "--plan-verify", plan_s],
        Some(&find_list(&d)),
    );
    eprintln!(
        "verify with drift: exit {c:?} {}",
        out.lines().last().unwrap()
    );
    assert_eq!(c, Some(1));
    // a symlink swapped in for a planned dir is never chmodded
    let (_, _) = apply(&rb_sh);
    let s0 = d.join("arena/homes/g2-0/sessions");
    std::fs::remove_dir(&s0).unwrap();
    std::os::unix::fs::symlink(d.join("tmp"), &s0).unwrap();
    let (_, out) = apply(&sh);
    assert!(
        out.contains("SKIP type") && out.contains("g2-0/sessions"),
        "{out}"
    );
    let tmp_mode = std::fs::metadata(d.join("tmp"))
        .unwrap()
        .permissions()
        .mode()
        & 0o7777;
    assert_eq!(tmp_mode, 0o1777, "symlink target untouched");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_file(&plan);
}

/// VPS (read-only): owner-aware `find`, plan for D-356-3 (+ console-dist),
/// and verify that every row is still pending (nothing applied yet).
#[test]
fn real_vps_chmod_plan_pending() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let Ok(host) = std::env::var("GALION_REAL_VPS") else {
        eprintln!("skipped: set GALION_REAL_VPS=<ssh host>");
        return;
    };
    let remote = "find /opt/galion -xdev \\( -path /opt/galion/openmind -o -path '*/secrets' -o -name .git -o -name node_modules -o -name target -o -name venv -o -name __pycache__ \\) -prune -o -printf '%m %y %u %g %p\\n' 2>/dev/null; find /opt/galion/console-dist/node_modules -printf '%m %y %u %g %p\\n' 2>/dev/null";
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
    eprintln!("VPS owner stat list: {} entries", list.lines().count());
    assert!(list.lines().count() > 1000);
    let (c, tsv, err) = rc(
        &[
            "--stat-list",
            "-",
            "--chmod-plan",
            "--plan-tree",
            "/opt/galion/console-dist",
        ],
        Some(&list),
    );
    eprintln!("plan exit {c:?}: {}", err.trim());
    assert_eq!(c, Some(0));
    let mut by_reason = std::collections::BTreeMap::new();
    for l in tsv.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = l.split('\t').collect();
        *by_reason
            .entry(format!("{} {}->{} {}", f[5], f[3], f[4], f[2]))
            .or_insert(0) += 1;
    }
    for (k, n) in &by_reason {
        eprintln!("  {n:>3} {k}");
    }
    let plan = std::env::temp_dir().join(format!("rc-vps-plan-{}.tsv", std::process::id()));
    std::fs::write(&plan, &tsv).unwrap();
    let (c, out, _) = rc(
        &["--stat-list", "-", "--plan-verify", plan.to_str().unwrap()],
        Some(&list),
    );
    eprintln!("verify: exit {c:?} {}", out.lines().last().unwrap_or(""));
    let _ = std::fs::remove_file(&plan);
    assert_eq!(c, Some(3), "D-356-3 not applied yet: every row pending");
}
