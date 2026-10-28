// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Wave-4 slot 434 (D-416-3): the release recipe must reproduce its sha256.
//! Gated: runs only with GALION_REAL=1 (three full release builds).
//!
//! R1 `release-recipe build` (gcc) writes artefact + .sha256 + .recipe, <= budget
//! R2 the artefact is a real gateway: `--check-config` exit 0, `--mock` start,
//!    GET /health 200 on 127.0.0.1, port closed after kill
//! R3 `check` passes; R4 `verify` rebuilds in another path and passes
//! R5 tampered recipe sha -> `check` fails; R6 recorded toolchain drift ->
//!    `verify` fails before building; R7 recorded commit that builds other bytes
//!    -> `verify` FAILS "does NOT reproduce"; R8 edited cargo_argv -> `check` fails
//!
//! Env: XTASK_RECIPE_COMMIT (default HEAD), XTASK_RECIPE_NEG_COMMIT (default
//! 83e573e2, the 373 land tree: same gateway minus the 657a9988 fold, other
//! bytes), XTASK_RECIPE_DIR (work dir), XTASK_RECIPE_PORT (default 23044),
//! XTASK_CARGO (passed through, e.g. cargo-slot). No network, loopback only.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn xtask(args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run xtask");
    println!(
        "$ xtask {} -> {}\n{}{}",
        args.join(" "),
        out.status,
        String::from_utf8_lossy(&out.stdout),
        tail(&String::from_utf8_lossy(&out.stderr), 6)
    );
    out
}

fn tail(s: &str, n: usize) -> String {
    let v: Vec<&str> = s.lines().collect();
    let mut t = v[v.len().saturating_sub(n)..].join("\n");
    t.push('\n');
    t
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn git(args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("git");
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn field(recipe: &str, k: &str) -> String {
    recipe
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")))
        .unwrap_or_else(|| panic!("recipe has no {k}"))
        .to_string()
}

/// Copy artefact + recipe into `dir`, rewriting the recipe with `edit`.
fn tampered(art: &Path, dir: &Path, edit: impl Fn(&str) -> String) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let name = art.file_name().unwrap();
    let dst = dir.join(name);
    fs::copy(art, &dst).unwrap();
    let r = fs::read_to_string(format!("{}.recipe", art.display())).unwrap();
    fs::write(format!("{}.recipe", dst.display()), edit(&r)).unwrap();
    dst
}

fn get(port: u16, path: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = s.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes(),
    );
    let mut b = Vec::new();
    let _ = s.read_to_end(&mut b);
    let t = String::from_utf8_lossy(&b).to_string();
    let code = t
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, t.split_once("\r\n\r\n").map(|x| x.1.to_string()).unwrap_or_default())
}

fn gw_cmd(exe: &Path, home: &Path, port: u16) -> Command {
    let mut c = Command::new(exe);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("HERMES_HOME", home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_STATE_DB", home.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", home.join("rs-spend.json"))
        .env("HERMES_RS_CRON_STATE", home.join("rs-cron-state.json"))
        .env("HERMES_RS_CRON_OUTPUT", home.join("rs-cron-output"))
        .env("HERMES_RS_MCP", "0")
        .env("API_SERVER_KEY", "t434-recipe-key");
    c
}

#[test]
fn real_release_recipe() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 for the real-life test");
        return;
    }
    let commit = std::env::var("XTASK_RECIPE_COMMIT").unwrap_or_else(|_| "HEAD".into());
    let neg = std::env::var("XTASK_RECIPE_NEG_COMMIT").unwrap_or_else(|_| "83e573e2".into());
    let neg_full = git(&["rev-parse", "--verify", &format!("{neg}^{{commit}}")]);
    let port: u16 = std::env::var("XTASK_RECIPE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(23044);
    let root = PathBuf::from(std::env::var("XTASK_RECIPE_DIR").unwrap_or_else(|_| {
        format!(
            "{}-recipe-test",
            std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "/tmp/xtask".into())
        )
    }));
    let _ = fs::remove_dir_all(&root);
    let out = root.join("out");
    let t0 = Instant::now();

    // R1 build
    let o = xtask(&[
        "release-recipe", "build", "--commit", &commit, "--cc", "gcc", "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "R1 build failed");
    let art = fs::read_dir(&out)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_none() && p.is_file())
        .expect("artefact");
    let recipe = fs::read_to_string(format!("{}.recipe", art.display())).unwrap();
    let sha = field(&recipe, "sha256");
    let size: u64 = field(&recipe, "size").parse().unwrap();
    let budget: u64 = field(&recipe, "budget").parse().unwrap();
    assert!(size <= budget, "R1 {size} > budget {budget}");
    assert_eq!(fs::metadata(&art).unwrap().len(), size);
    let sumfile = fs::read_to_string(format!("{}.sha256", art.display())).unwrap();
    assert!(sumfile.starts_with(&sha), "R1 .sha256 file");
    for k in ["rustc", "cargo", "cc_version", "lld", "cargo_argv", "env", "commit"] {
        println!("R1 recipe {k}={}", field(&recipe, k));
    }
    println!("R1 artefact {} {size} B sha256 {sha} ({:?})", art.display(), t0.elapsed());

    // R2 real binary
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let cc = gw_cmd(&art, &home, port).arg("--check-config").output().unwrap();
    assert_eq!(cc.status.code(), Some(0), "R2 --check-config: {}", text(&cc));
    let mut child = gw_cmd(&art, &home, port)
        .arg("--mock")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let up = Instant::now();
    let mut health = (0, String::new());
    while up.elapsed() < Duration::from_secs(20) {
        health = get(port, "/health");
        if health.0 == 200 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(health.0, 200, "R2 /health");
    assert!(health.1.contains("hermes-rs"), "R2 body {}", health.1);
    println!("R2 /health 200 in {:?}: {}", up.elapsed(), health.1.trim());
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(get(port, "/health").0, 0, "R2 port {port} still open");

    // R3 check, R4 verify (rebuild in another path)
    assert!(xtask(&["release-recipe", "check", art.to_str().unwrap()]).status.success());
    let v_work = root.join("verify-other-path");
    let o = xtask(&[
        "release-recipe", "verify", art.to_str().unwrap(), "--work", v_work.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "R4 verify failed");
    assert!(text(&o).contains("verify OK"));

    // R5 tampered sha
    let a5 = tampered(&art, &root.join("r5"), |r| {
        r.replace(&format!("sha256={sha}"), &format!("sha256={}", "0".repeat(64)))
    });
    let o = xtask(&["release-recipe", "check", a5.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("!= recipe sha256"), "R5");

    // R6 toolchain drift: refuse before building
    let rustc = field(&recipe, "rustc");
    let a6 = tampered(&art, &root.join("r6"), |r| {
        r.replace(&format!("rustc={rustc}\n"), "rustc=rustc 0.0.0\n")
    });
    let t6 = Instant::now();
    let o = xtask(&["release-recipe", "verify", a6.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("toolchain differs"), "R6");
    assert!(t6.elapsed() < Duration::from_secs(60), "R6 must not build");

    // R8 edited cargo_argv
    let a8 = tampered(&art, &root.join("r8"), |r| {
        let argv = field(r, "cargo_argv");
        r.replace(&argv, &format!("{argv} --features coding"))
    });
    let o = xtask(&["release-recipe", "check", a8.to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("edited"), "R8");

    // R7 the recorded recipe does not reproduce the sha -> verify fails
    let full = field(&recipe, "commit");
    assert_ne!(full, neg_full, "XTASK_RECIPE_NEG_COMMIT must differ from the commit");
    let a7 = tampered(&art, &root.join("r7"), |r| {
        r.replace(&format!("commit={full}"), &format!("commit={neg_full}"))
    });
    let o = xtask(&["release-recipe", "verify", a7.to_str().unwrap()]);
    assert!(!o.status.success(), "R7 verify must FAIL");
    assert!(text(&o).contains("does NOT reproduce"), "R7 reason: {}", text(&o));
    println!("R1-R8 pass in {:?}", t0.elapsed());
    let _ = fs::remove_dir_all(&root);
}
