// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 398 chmod-plan-refresh, real tests (`GALION_REAL=1` and
//! `GALION_REAL_VPS=<ssh host>`). READ-ONLY on the VPS: `find -printf` and
//! `test -e` only (metadata, nothing opened, nothing changed).
//!
//! 1. the staged 69-row plan verified against the live listing: no row
//!    missing (removed trees) and no drift; rows still pending until D-356-3a.
//! 2. the F-379-1 legacy Hermes trees are still gone.
//! 3. `--plan-ow-tree` over the live tts/stt/vision venvs: only `o-w` rows.

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_repo-check");
const PLAN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/staged/world-writable-dirs/chmod-plan.tsv"
);

fn host() -> Option<String> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return None;
    }
    let h = std::env::var("GALION_REAL_VPS").ok();
    if h.is_none() {
        eprintln!("skipped: set GALION_REAL_VPS=<ssh host>");
    }
    h
}

fn ssh(host: &str, remote: &str) -> String {
    let o = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            host,
            remote,
        ])
        .stdin(Stdio::null())
        .output()
        .expect("ssh");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn rc(args: &[&str], stdin: &str) -> (Option<i32>, String, String) {
    let mut c = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn real_vps_staged_plan_still_valid() {
    let Some(h) = host() else { return };
    let remote = "find /opt/galion -xdev \\( -path /opt/galion/openmind -o -path '*/secrets' -o -name .git -o -name node_modules -o -name target -o -name venv -o -name __pycache__ \\) -prune -o -printf '%m %y %u %g %p\\n' 2>/dev/null; find /opt/galion/console-dist/node_modules -printf '%m %y %u %g %p\\n' 2>/dev/null";
    let list = ssh(&h, remote);
    eprintln!("VPS owner stat list: {} entries", list.lines().count());
    assert!(list.lines().count() > 1000);
    let (c, out, _) = rc(&["--stat-list", "-", "--plan-verify", PLAN], &list);
    let mut states = std::collections::BTreeMap::new();
    for l in out.lines().filter(|l| !l.starts_with("plan-verify")) {
        *states
            .entry(l.split_whitespace().next().unwrap_or("").to_string())
            .or_insert(0) += 1;
    }
    eprintln!("verify exit {c:?}: {states:?}");
    eprintln!("{}", out.lines().last().unwrap_or(""));
    assert!(out.contains("69 rows"), "{out}");
    assert!(
        out.contains("0 drift/missing/owner"),
        "a staged row is missing or drifted: refresh the plan\n{out}"
    );
    // pending (3) until the owner applies D-356-3a, applied (0) after
    assert!(matches!(c, Some(0) | Some(3)), "{c:?}");
    // regenerating from the live listing gives the same rows
    let (c, tsv, err) = rc(
        &[
            "--stat-list",
            "-",
            "--chmod-plan",
            "--plan-tree",
            "/opt/galion/console-dist",
            "--plan-reasons",
            "dir-o+w-no-sticky,tree-normalize",
        ],
        &list,
    );
    eprintln!("regenerate exit {c:?}: {}", err.trim());
    let rows = |t: &str| -> Vec<String> {
        t.lines()
            .filter(|l| !l.starts_with('#'))
            .map(str::to_string)
            .collect()
    };
    let staged = std::fs::read_to_string(PLAN).unwrap();
    if c == Some(0) && out.contains(" 0 applied") {
        assert_eq!(rows(&tsv), rows(&staged), "live listing gives other rows");
        eprintln!("regenerated rows identical to the staged plan (69)");
    }
}

#[test]
fn real_vps_legacy_hermes_trees_gone() {
    let Some(h) = host() else { return };
    let out = ssh(
        &h,
        "for p in /opt/galion/tooling/hermes-agent /opt/galion/galion-light/shadow/hermes-agent; do if [ -e \"$p\" ]; then echo \"EXISTS $p\"; else echo \"GONE $p\"; fi; done; ls -A /opt/galion/galion-light/shadow | wc -l",
    );
    eprint!("{out}");
    assert_eq!(out.matches("GONE ").count(), 2, "{out}");
    assert_eq!(out.lines().last().map(str::trim), Some("0"), "{out}");
}

#[test]
fn real_vps_venv_ow_plan() {
    let Some(h) = host() else { return };
    let trees = [
        "/opt/galion/tts/venv",
        "/opt/galion/stt/venv",
        "/opt/galion/vision/venv",
    ];
    let remote = format!(
        "find /opt/galion /opt/galion/tts /opt/galion/stt /opt/galion/vision -maxdepth 0 -printf '%m %y %u %g %p\\n'; find {} -xdev -printf '%m %y %u %g %p\\n' 2>/dev/null",
        trees.join(" ")
    );
    let list = ssh(&h, &remote);
    eprintln!("VPS venv stat list: {} entries", list.lines().count());
    assert!(list.lines().count() > 1000);
    let mut args = vec!["--stat-list", "-", "--chmod-plan"];
    for t in &trees {
        args.extend(["--plan-ow-tree", t]);
    }
    let (c, tsv, err) = rc(&args, &list);
    eprintln!("plan exit {c:?}: {}", err.trim());
    assert_eq!(c, Some(0));
    let mut by = std::collections::BTreeMap::new();
    for l in tsv.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = l.split('\t').collect();
        assert_eq!(f[5], "tree-o-w", "{l}");
        let old = u32::from_str_radix(f[3], 8).unwrap();
        let new = u32::from_str_radix(f[4], 8).unwrap();
        assert_eq!(new, old & !0o002, "{l}");
        *by.entry(format!("{} {}->{} {}", f[1], f[3], f[4], f[2]))
            .or_insert(0) += 1;
    }
    for (k, n) in &by {
        eprintln!("  {n:>3} {k}");
    }
    let n: usize = by.values().sum();
    eprintln!("venv o-w rows: {n}");
}
