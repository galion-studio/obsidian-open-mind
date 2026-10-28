// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 397: real-binary test that the staged tmpfiles.d file
//! (deploy/staged/tmpfiles.d/hermes-coding.conf) closes the slot-381 gap:
//! with the staged coding drop-in, `hermes-gateway --check-config` exits 2
//! while /srv/hermes-coding is missing, and exits 0 `coding=on` once
//! `systemd-tmpfiles --create` + the runbook clone ran. Done in a throwaway
//! sim root (`systemd-tmpfiles --root=`; paths remapped under it), never on
//! the real /srv. Harness adapted from slot 355 `real_crashloop_r3.rs`.
//!
//! Gated: GALION_REAL=1. Binary from HERMES_GW_BIN (else
//! $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/hermes-gateway).
//! Only `--check-config` runs: nothing binds (port 22670 is only passed as
//! config), no sandbox, no namespaces, no network. Scratch under $TMPDIR.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const PORT: u16 = 22670;
const TMPFILES: &str = include_str!("../../../deploy/staged/tmpfiles.d/hermes-coding.conf");
const DROPIN: &str = include_str!("../../../deploy/staged/hermes-rs.service.d/coding-tools.conf");

fn on() -> bool {
    std::env::var("GALION_REAL").ok().as_deref() == Some("1")
}

fn bin() -> PathBuf {
    if let Ok(b) = std::env::var("HERMES_GW_BIN") {
        return PathBuf::from(b);
    }
    let t = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "../../target".into());
    PathBuf::from(t).join("x86_64-unknown-linux-musl/release/hermes-gateway")
}

fn env_of(key: &str) -> Option<String> {
    DROPIN
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .filter(|(k, _)| *k == key)
        .map(|(_, v)| v.trim().to_string())
        .next_back()
}

fn id(a: &str) -> String {
    let o = Command::new("id").arg(a).output().expect("id");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn under(root: &Path, p: &str) -> PathBuf {
    root.join(p.trim_start_matches('/'))
}

/// Sim root with its own /etc (tmpfiles --root resolves names there) and the
/// staged file, root:root swapped for the test user when not root.
fn sim(base: &Path) -> (PathBuf, PathBuf) {
    let root = base.join("root");
    fs::create_dir_all(root.join("etc")).unwrap();
    fs::create_dir_all(root.join("srv")).unwrap();
    fs::set_permissions(root.join("srv"), fs::Permissions::from_mode(0o755)).unwrap();
    let (u, g, uid, gid) = (id("-un"), id("-gn"), id("-u"), id("-g"));
    fs::write(
        root.join("etc/passwd"),
        format!("root:x:0:0::/root:/bin/sh\n{u}:x:{uid}:{gid}::/:/bin/sh\n"),
    )
    .unwrap();
    fs::write(root.join("etc/group"), format!("root:x:0:\n{g}:x:{gid}:\n")).unwrap();
    let txt: String = TMPFILES
        .lines()
        .map(|l| {
            if u != "root" && l.trim_start().starts_with('d') {
                l.replace(" root root ", &format!(" {u} {g} "))
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let conf = base.join("hermes-coding.conf");
    fs::write(&conf, txt).unwrap();
    (root, conf)
}

fn tmpfiles(root: &Path, conf: &Path) -> i32 {
    Command::new("systemd-tmpfiles")
        .arg("--create")
        .arg(format!("--root={}", root.display()))
        .arg(conf)
        .status()
        .expect("systemd-tmpfiles")
        .code()
        .unwrap_or(-1)
}

/// The staged coding env, every path remapped into the sim root.
fn coding_env(root: &Path, toolchain: bool) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = DROPIN
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, val)| (k.to_string(), val.trim().to_string()))
        .collect();
    // Last value wins, as in systemd.
    let mut seen = BTreeSet::new();
    v.reverse();
    v.retain(|(k, _)| seen.insert(k.clone()));
    v.reverse();
    for (k, val) in v.iter_mut() {
        if val.starts_with('/') {
            *val = under(root, val).to_string_lossy().into_owned();
        }
        // The cgroup delegate needs the real unit; not part of this check.
        if k == "HERMES_RS_CODING_CGROUP" {
            *val = String::new();
        }
    }
    if !toolchain {
        v.retain(|(k, _)| k != "HERMES_RS_CODING_TOOLCHAIN");
    }
    v
}

fn check(home: &Path, envs: &[(String, String)]) -> (i32, String, String) {
    let o = Command::new(bin())
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", PORT.to_string())
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", home.join("rs-spend.json"))
        .env("API_SERVER_KEY", "t397-coding-dir-stage-key")
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .arg("--check-config")
        .output()
        .expect("spawn hermes-gateway");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn coding_line(out: &str) -> String {
    out.lines()
        .find(|l| l.starts_with("coding="))
        .unwrap_or("")
        .to_string()
}

#[test]
fn real_tmpfiles_closes_the_missing_srv_gap() {
    if !on() {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    assert!(bin().is_file(), "no gateway binary at {:?}", bin());
    let base = std::env::temp_dir().join(format!("slot397-gw-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let home = base.join("hermes-home");
    fs::create_dir_all(&home).unwrap();
    let (root, conf) = sim(&base);

    // A. Today's VPS shape: /srv/hermes-coding missing -> exit 2.
    let (c, o, e) = check(&home, &coding_env(&root, false));
    println!("A missing dir: exit={c} {} | {}", coding_line(&o), e.trim());
    assert_eq!(c, 2);
    assert_eq!(coding_line(&o), "coding=error");
    assert!(e.contains("No such file"), "{e}");

    // B. tmpfiles only (no clone yet) -> still exit 2, now "not a git checkout".
    assert_eq!(tmpfiles(&root, &conf), 0);
    let (c, o, e) = check(&home, &coding_env(&root, false));
    println!(
        "B dirs, no clone: exit={c} {} | {}",
        coding_line(&o),
        e.trim()
    );
    assert_eq!(c, 2);
    assert!(
        e.contains("No such file") || e.contains("not a git checkout"),
        "{e}"
    );

    // C. runbook step 1b: clone (here git init) -> exit 0 coding=on.
    let wt = under(&root, &env_of("HERMES_RS_CODING_WORKTREE").unwrap());
    assert!(Command::new("git")
        .args(["init", "-q"])
        .arg(&wt)
        .status()
        .unwrap()
        .success());
    let (c, o, e) = check(&home, &coding_env(&root, false));
    println!(
        "C dirs + clone: exit={c} {} | {}",
        coding_line(&o),
        e.trim()
    );
    assert_eq!(c, 0, "{e}");
    assert!(coding_line(&o).starts_with("coding=on worktree="), "{o}");
    assert!(coding_line(&o).contains("on_error=disable"), "{o}");

    // D. With the staged HERMES_RS_CODING_TOOLCHAIN=/opt/rust/toolchain kept:
    // missing on the VPS today (F-397-1) -> exit 2 again.
    let (c, o, e) = check(&home, &coding_env(&root, true));
    println!(
        "D staged toolchain missing: exit={c} {} | {}",
        coding_line(&o),
        e.trim()
    );
    assert_eq!(c, 2);
    assert!(e.contains("HERMES_RS_CODING_TOOLCHAIN"), "{e}");

    // E. Same, toolchain present (sim bin/cargo) -> exit 0.
    let tc = under(&root, &env_of("HERMES_RS_CODING_TOOLCHAIN").unwrap());
    fs::create_dir_all(tc.join("bin")).unwrap();
    fs::write(tc.join("bin/cargo"), b"").unwrap();
    let (c, o, e) = check(&home, &coding_env(&root, true));
    println!(
        "E toolchain present: exit={c} {} | {}",
        coding_line(&o),
        e.trim()
    );
    assert_eq!(c, 0, "{e}");

    // F. --check-config created nothing and the modes stayed 0700.
    for l in TMPFILES.lines().filter(|l| l.trim_start().starts_with('d')) {
        let p = under(&root, l.split_whitespace().nth(1).unwrap());
        let m = fs::metadata(&p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(m, 0o700, "{p:?}");
    }
    assert!(!under(&root, "/srv/hermes-coding/state/audit.jsonl").exists());

    fs::remove_dir_all(&base).unwrap();
}
