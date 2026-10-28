// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 355: real-binary test for the hermes-rs R3 crash loop of
//! 2026-10-27 19:45 CET (exit 2, "coding: config error: worktree
//! /var/lib/hermes-rs/coding-worktree overlaps the denied path /var/lib").
//! Harness pieces adapted from slot 340 `real_offhost_keyrot.rs` (same repo, MIT).
//!
//! Gated: runs only with GALION_REAL=1. Uses the release hermes-gateway from
//! HERMES_GW_BIN (else $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release).
//! Optional HERMES_GW_BASE_BIN = a build before the fix (6845c350 or the
//! live b7fbbc78): it must show the blind spot (`--check-config` says ok for
//! the config that crash-loops). Ports 22250-22259, loopback only, `--mock`
//! (no network, no spend). No sandbox and no namespace probe runs here: every
//! case fails or is checked before the coding preflight.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const P_REPRO: u16 = 22250;
const P_DISABLE: u16 = 22251;
const API: &str = "t355-crashloop-gateway-key";

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

fn http_get(port: u16, path: &str, auth: bool) -> u16 {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return 0;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if auth {
        req.push_str(&format!("Authorization: Bearer {API}\r\n"));
    }
    req.push_str("\r\n");
    let _ = s.write_all(req.as_bytes());
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out)
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

struct Gw(Child);
impl Drop for Gw {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn base_cmd(exe: &Path, home: &Path, port: u16, envs: &[(&str, String)]) -> Command {
    let mut c = Command::new(exe);
    c.env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", home.join("rs-spend.json"))
        .env("HERMES_RS_CRON_STATE", home.join("rs-cron-state.json"))
        .env("API_SERVER_KEY", API)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        c.env(k, v);
    }
    c
}

/// (exit code, stdout, stderr, elapsed ms) of a run that must end by itself.
fn run(
    exe: &Path,
    home: &Path,
    port: u16,
    envs: &[(&str, String)],
    args: &[&str],
) -> (i32, String, String, u128) {
    let t0 = Instant::now();
    let mut ch = base_cmd(exe, home, port, envs)
        .args(args)
        .spawn()
        .expect("spawn");
    loop {
        if let Some(st) = ch.try_wait().unwrap() {
            let mut o = String::new();
            let mut e = String::new();
            ch.stdout.take().unwrap().read_to_string(&mut o).unwrap();
            ch.stderr.take().unwrap().read_to_string(&mut e).unwrap();
            return (st.code().unwrap_or(-1), o, e, t0.elapsed().as_millis());
        }
        if t0.elapsed() > Duration::from_secs(10) {
            let _ = ch.kill();
            let _ = ch.wait();
            return (
                -9,
                String::new(),
                "timeout: still running after 10 s".into(),
                10_000,
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_up(port: u16) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(15) {
        if http_get(port, "/health", false) == 200 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

/// An existing directory under /var/lib (the live case was a path there).
fn var_lib_dir() -> String {
    [
        "/var/lib/dpkg",
        "/var/lib/misc",
        "/var/lib/systemd",
        "/var/lib",
    ]
    .iter()
    .find(|p| Path::new(p).is_dir())
    .unwrap()
    .to_string()
}

fn fresh(root: &Path, name: &str) -> PathBuf {
    let d = root.join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn real_crashloop_r3() {
    if !on() {
        eprintln!("SKIP: set GALION_REAL=1 for the real-life test");
        return;
    }
    let exe = bin();
    assert!(exe.is_file(), "no gateway binary at {exe:?}");
    let root = std::env::temp_dir().join(format!("t355-crashloop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let wt_bad = var_lib_dir();
    let bad: Vec<(&str, String)> = vec![
        ("HERMES_RS_CODING_WORKTREE", wt_bad.clone()),
        ("HERMES_RS_CODING_BRANCH", "hermes/ticket".into()),
    ];

    // 1. Reproduce: the live config shape exits 2 at start, no bind.
    let home = fresh(&root, "h1");
    let (code, _, err, ms) = run(&exe, &home, P_REPRO, &bad, &["--mock"]);
    println!(
        "R1 start, worktree {wt_bad}: exit={code} in {ms} ms: {}",
        err.trim()
    );
    assert_eq!(code, 2);
    assert!(err.contains("coding: config error: worktree"), "{err}");
    assert!(err.contains("overlaps the denied path /var/lib"), "{err}");
    assert!(err.contains("ReadWritePaths"), "hint missing: {err}");
    assert_eq!(
        http_get(P_REPRO, "/health", false),
        0,
        "must not have bound"
    );
    // Each systemd restart repeats the same exit: 3 runs, 3x exit 2.
    let codes: Vec<i32> = (0..3)
        .map(|_| run(&exe, &home, P_REPRO, &bad, &["--mock"]).0)
        .collect();
    println!("R1 restart x3: exits {codes:?}");
    assert_eq!(codes, [2, 2, 2]);

    // 2. --check-config now catches it (exit 2) before any restart.
    let (code, out, err, _) = run(&exe, &home, P_REPRO, &bad, &["--check-config"]);
    println!(
        "R2 --check-config bad: exit={code} out_tail={:?} err={}",
        out.lines().last(),
        err.trim()
    );
    assert_eq!(code, 2);
    assert!(out.contains("coding=error"), "{out}");
    if let Ok(base) = std::env::var("HERMES_GW_BASE_BIN") {
        let (bc, bo, _, _) = run(Path::new(&base), &home, P_REPRO, &bad, &["--check-config"]);
        println!(
            "R2-before {base}: --check-config bad: exit={bc} (blind spot when 0), coding line: {}",
            bo.lines().any(|l| l.starts_with("coding="))
        );
        assert_eq!(bc, 0, "the pre-fix binary should not see the coding error");
    }

    // 3. The /srv retry shape: worktree ok, state not writable -> exit 2 in check.
    let wt = fresh(&root, "wt");
    fs::create_dir_all(wt.join(".git")).unwrap();
    let lock = fresh(&root, "locked");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o500)).unwrap();
    let locked_env: Vec<(&str, String)> = vec![
        ("HERMES_RS_CODING_WORKTREE", wt.display().to_string()),
        ("HERMES_RS_CODING_BRANCH", "hermes/ticket".into()),
        (
            "HERMES_RS_CODING_STATE",
            lock.join("state").display().to_string(),
        ),
    ];
    let root_user = fs::metadata("/proc/self").map(|m| {
        use std::os::unix::fs::MetadataExt;
        m.uid() == 0
    });
    let (code, out, err, _) = run(&exe, &home, P_REPRO, &locked_env, &["--check-config"]);
    println!(
        "R3 --check-config state not writable: exit={code} last={:?} err={}",
        out.lines().last(),
        err.trim()
    );
    if root_user.unwrap_or(false) {
        println!("R3 running as root: mode bits ignored, write check skipped");
    } else {
        assert_eq!(code, 2);
        assert!(err.contains("not writable"), "{err}");
    }
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o700)).unwrap();

    // 4. Good config: check says coding=on, exit 0; nothing created.
    let good: Vec<(&str, String)> = vec![
        ("HERMES_RS_CODING_WORKTREE", wt.display().to_string()),
        ("HERMES_RS_CODING_BRANCH", "hermes/ticket".into()),
        (
            "HERMES_RS_CODING_STATE",
            root.join("state").display().to_string(),
        ),
    ];
    let (code, out, _, _) = run(&exe, &home, P_REPRO, &good, &["--check-config"]);
    let line = out
        .lines()
        .find(|l| l.starts_with("coding="))
        .unwrap_or("")
        .to_string();
    println!("R4 --check-config good: exit={code} {line}");
    assert_eq!(code, 0);
    assert!(line.starts_with("coding=on "), "{line}");
    assert!(
        !root.join("state").exists(),
        "check must not create the state dir"
    );
    let (code, out, _, _) = run(&exe, &home, P_REPRO, &[], &["--check-config"]);
    println!(
        "R4 --check-config no coding env: exit={code} {:?}",
        out.lines().last()
    );
    assert_eq!(code, 0);
    assert!(out.lines().any(|l| l == "coding=off"), "{out}");

    // 5. HERMES_RS_CODING_ON_ERROR=disable: same bad config, chat stays up.
    let home5 = fresh(&root, "h5");
    let mut dis = bad.clone();
    dis.push(("HERMES_RS_CODING_ON_ERROR", "disable".into()));
    let t0 = Instant::now();
    let g = Gw(base_cmd(&exe, &home5, P_DISABLE, &dis)
        .arg("--mock")
        .spawn()
        .expect("spawn"));
    let up = wait_up(P_DISABLE);
    let up_ms = t0.elapsed().as_millis();
    let models = http_get(P_DISABLE, "/v1/models", true);
    let health = http_get(P_DISABLE, "/health", false);
    let mut g = g;
    let _ = g.0.kill();
    let mut err = String::new();
    g.0.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    let _ = g.0.wait();
    drop(g);
    println!("R5 ON_ERROR=disable: up={up} in {up_ms} ms, /health={health}, /v1/models={models}");
    for l in err
        .lines()
        .filter(|l| l.contains("coding") || l.contains("listening"))
    {
        println!("R5 log: {l}");
    }
    assert!(up);
    assert_eq!(health, 200);
    assert_eq!(models, 200);
    assert!(err.contains("coding tools OFF, gateway continues"), "{err}");

    fs::remove_dir_all(&root).unwrap();
    assert_eq!(http_get(P_DISABLE, "/health", false), 0, "port 22251 freed");
}
