// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Integration S5-T3 (slot 228): end-to-end rehearsal of the packaged
//! release on the box, beyond `real_release.rs`:
//! stamps against the build provenance, cross-target parity, a non-binding
//! `--help` run of every binary, headroom, and the SQLite-linking
//! openmind-memory started from each package (zig-cc C code under qemu too).
//!
//! Gated: `GALION_REAL=1`. Needs the release.yml-style static musl builds in
//! `$GALION_RELEASE_TARGET_DIR` (default `$CARGO_TARGET_DIR`), built with
//! `GALION_BUILD_COMMIT` / `GALION_BUILD_TIME` set; the same two variables
//! must be set for this test (it expects them in every stamped binary).
//! Ports: `GALION_REHEARSAL_PORT_BASE` (default 20980), uses base..base+1.
//! Evidence: `$GALION_REHEARSAL_EVIDENCE` (directory) gets rehearse.tsv and
//! summary.txt. Tag `v<version>-rehearsal`: nothing is tagged or pushed.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const TARGETS: [&str; 2] = ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"];
const BIN: &str = env!("CARGO_BIN_EXE_galion-release");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn target_dir() -> PathBuf {
    std::env::var_os("GALION_RELEASE_TARGET_DIR")
        .or_else(|| std::env::var_os("CARGO_TARGET_DIR"))
        .map(PathBuf::from)
        .expect("set GALION_RELEASE_TARGET_DIR or CARGO_TARGET_DIR")
}

fn tag() -> String {
    format!("v{}-rehearsal", env!("CARGO_PKG_VERSION"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("galion-rehearsal-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn rel(args: &[&str]) -> Output {
    let o = Command::new(BIN)
        .args(args)
        .output()
        .expect("run galion-release");
    eprintln!(
        "$ galion-release {}\n-> {:?}\n{}{}",
        args.join(" "),
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    o
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn package(out: &Path, t: &str, extra_args: &[&str]) {
    let td = target_dir();
    let ws = td.join(t).join("release");
    let om = td.join("omnode").join(t).join("release");
    assert!(ws.is_dir() && om.is_dir(), "missing release builds for {t}");
    let mut a = vec![
        "package",
        "--target",
        t,
        "--out",
        out.to_str().unwrap(),
        "--bin-dir",
        ws.to_str().unwrap(),
        "--bin-dir",
        om.to_str().unwrap(),
        "--extra",
    ];
    let lic = repo().join("LICENSE");
    let readme = repo().join("README.md");
    let tg = tag();
    a.push(lic.to_str().unwrap());
    a.extend(["--extra", readme.to_str().unwrap(), "--tag", &tg]);
    a.extend(extra_args);
    assert!(rel(&a).status.success(), "package {t} failed");
}

fn env_req(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("set {k} (the build provenance)"))
}

fn http(port: u16, req: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.write_all(req.as_bytes()).ok()?;
    let mut b = String::new();
    s.read_to_string(&mut b).ok()?;
    let code = b.split_whitespace().nth(1)?.parse().ok()?;
    let body = b
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    Some((code, body))
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port_free(p: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", p)).is_ok()
}

#[test]
fn real_s5t3_rehearsal() {
    if !real() {
        eprintln!("skip: GALION_REAL!=1");
        return;
    }
    let commit = env_req("GALION_BUILD_COMMIT");
    let built = env_req("GALION_BUILD_TIME");
    let ev = std::env::var_os("GALION_REHEARSAL_EVIDENCE").map(PathBuf::from);
    let mut summary = String::new();

    // 1. Package both targets, SHA256SUMS, verify, sha256sum -c.
    let dist = tmp("dist");
    for t in TARGETS {
        package(&dist, t, &[]);
    }
    assert!(rel(&["sums", dist.to_str().unwrap()]).status.success());
    let v = rel(&["verify", dist.to_str().unwrap()]);
    assert!(v.status.success());
    summary.push_str(&String::from_utf8_lossy(&v.stdout));
    let c = Command::new("sha256sum")
        .args(["-c", "SHA256SUMS"])
        .current_dir(&dist)
        .output()
        .unwrap();
    assert!(c.status.success(), "sha256sum -c failed");
    summary.push_str(&String::from_utf8_lossy(&c.stdout));

    // 2. Full rehearsal with provenance + --help.
    let work = tmp("work");
    let tsv = tmp("ev").join("rehearse.tsv");
    let r = rel(&[
        "rehearse",
        dist.to_str().unwrap(),
        "--work",
        work.to_str().unwrap(),
        "--timeout",
        "60",
        "--expect-commit",
        &commit,
        "--expect-built",
        &built,
        "--help-timeout",
        "5",
        "--tsv",
        tsv.to_str().unwrap(),
    ]);
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let table = fs::read_to_string(&tsv).unwrap();
    for l in out.lines().filter(|l| !l.contains('\t')) {
        summary.push_str(l);
        summary.push('\n');
    }
    if let Some(d) = &ev {
        fs::create_dir_all(d).unwrap();
        fs::write(d.join("rehearse.tsv"), &table).unwrap();
    }
    assert!(r.status.success(), "rehearse failed");
    let rows: Vec<Vec<&str>> = table
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect();
    assert!(rows.iter().all(|r| r[0] == "PASS"));
    assert!(rows.iter().all(|r| r.len() == 12));
    let per: Vec<usize> = TARGETS
        .iter()
        .map(|t| rows.iter().filter(|r| r[1] == *t).count())
        .collect();
    assert_eq!(per[0], per[1], "binary count differs per target");
    assert!(per[0] >= 30, "too few binaries: {per:?}");
    assert!(rows
        .iter()
        .filter(|r| r[1].starts_with("aarch64"))
        .all(|r| r[5] == "qemu-aarch64-static"));
    // every stamped row carries exactly this build's provenance
    let stamp = format!("(commit {commit}, built {built})");
    for r in rows.iter().filter(|r| r[8] == "stamped") {
        assert!(r[7].ends_with(&stamp), "{r:?}");
    }
    assert!(out.contains("parity: ok"));
    assert!(
        rows.iter().all(|r| r[9] != "-"),
        "--help not run everywhere"
    );

    // 3. Negative controls for the new checks.
    // 3a wrong commit: only stamped rows fail.
    let r = rel(&[
        "rehearse",
        dist.to_str().unwrap(),
        "--work",
        tmp("work-bad").to_str().unwrap(),
        "--expect-commit",
        "deadbeef0000",
    ]);
    assert_eq!(r.status.code(), Some(1));
    let o = String::from_utf8_lossy(&r.stdout).to_string();
    let fails = o.lines().filter(|l| l.starts_with("FAIL\t")).count();
    let stamped = rows.iter().filter(|r| r[8] == "stamped").count();
    assert_eq!(fails, stamped, "wrong-commit: only stamped binaries fail");
    summary.push_str(&format!(
        "negative wrong commit: exit 1, {fails} FAIL rows = {stamped} stamped binaries\n"
    ));
    // 3b wrong build time
    let r = rel(&[
        "rehearse",
        dist.to_str().unwrap(),
        "--work",
        tmp("work-bad2").to_str().unwrap(),
        "--expect-built",
        "2000-01-01T00:00:00+01:00",
    ]);
    assert_eq!(r.status.code(), Some(1));
    summary.push_str("negative wrong build time: exit 1\n");
    // 3c parity: aarch64 package without tuner
    let skew = tmp("dist-skew");
    package(&skew, TARGETS[0], &[]);
    package(&skew, TARGETS[1], &["--exclude", "tuner"]);
    assert!(rel(&["sums", skew.to_str().unwrap()]).status.success());
    assert!(rel(&["verify", skew.to_str().unwrap()]).status.success());
    let r = rel(&[
        "rehearse",
        skew.to_str().unwrap(),
        "--work",
        tmp("work-skew").to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(1));
    let o = String::from_utf8_lossy(&r.stdout).to_string();
    assert!(
        o.contains("parity: FAIL tuner: in x86_64-unknown-linux-musl but not in aarch64-unknown-linux-musl"),
        "{o}"
    );
    summary.push_str("negative arch skew (aarch64 without tuner): exit 1, parity FAIL tuner\n");

    // 4. openmind-memory (rusqlite bundled, zig cc C) from each package.
    let base: u16 = std::env::var("GALION_REHEARSAL_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20980);
    let qemu = Command::new("sh")
        .args(["-c", "command -v qemu-aarch64-static"])
        .output()
        .unwrap();
    let qemu = String::from_utf8_lossy(&qemu.stdout).trim().to_string();
    for (i, t) in TARGETS.iter().enumerate() {
        let port = base + i as u16;
        assert!(port_free(port), "port {port} busy");
        let exe = work
            .join(format!("obsidian-open-mind-{}-{t}", tag()))
            .join("openmind-memory");
        let mut c = if t.starts_with("aarch64") {
            let mut c = Command::new(&qemu);
            c.arg(&exe);
            c
        } else {
            Command::new(&exe)
        };
        let data = tmp(&format!("mem-{i}"));
        let _m = Kill(
            c.env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", &data)
                .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
                .env("OPENMIND_MEMORY_PORT", port.to_string())
                .env("OPENMIND_MEMORY_DATA_DIR", &data)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let t0 = Instant::now();
        let (code, body) = loop {
            if let Some(x) = http(
                port,
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            ) {
                break x;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "{t}: memory no reply"
            );
            std::thread::sleep(Duration::from_millis(200));
        };
        let ms = t0.elapsed().as_millis();
        assert_eq!(code, 200, "{t}: {body}");
        let dbs: Vec<String> = fs::read_dir(&data)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".db"))
            .collect();
        assert!(!dbs.is_empty(), "{t}: no SQLite db created in data dir");
        let short: String = body.chars().take(160).collect();
        summary.push_str(&format!(
            "{t} openmind-memory :{port} /health {code} {ms} ms db={} {short}\n",
            dbs.join(",")
        ));
        drop(_m);
        let _ = fs::remove_dir_all(&data);
    }
    for i in 0..2 {
        assert!(port_free(base + i), "port {} still busy", base + i);
    }

    eprintln!("SUMMARY\n{summary}");
    if let Some(d) = &ev {
        fs::write(d.join("summary.txt"), &summary).unwrap();
    }
    let _ = (dist, skew, work);
    let pre = format!("galion-rehearsal-{}-", std::process::id());
    for e in fs::read_dir(std::env::temp_dir()).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with(&pre) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}
