// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real rehearsal of release.yml's packaging on the box (S4-T7 / S5-T3).
//!
//! Gated: `GALION_REAL=1`. Needs the static musl release builds that
//! release.yml's `ucpu-gate.sh build` step makes, for both tier-1 targets, in
//! `$GALION_RELEASE_TARGET_DIR` (default `$CARGO_TARGET_DIR`):
//! `<t>/release` (workspace) and `omnode/<t>/release`. The test does not
//! build them (the box runs builds through the shared build queue).
//! Needs `qemu-aarch64-static`, `sha256sum` and `tar` (read-only cross-checks).
//! Ports: `GALION_RELEASE_PORT_BASE` (default 20860), uses base..base+3.
//! Writes a TSV evidence table to `$GALION_RELEASE_EVIDENCE` when set.
//! Tag `v<version>-rehearsal`: no tag is created or pushed.

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
    let d = std::env::temp_dir().join(format!("galion-release-real-{}-{name}", std::process::id()));
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

fn package_all(out: &Path, mtime: &str) {
    let td = target_dir();
    for t in TARGETS {
        let ws = td.join(t).join("release");
        let om = td.join("omnode").join(t).join("release");
        assert!(
            ws.is_dir() && om.is_dir(),
            "missing release builds for {t} under {}",
            td.display()
        );
        let o = rel(&[
            "package",
            "--target",
            t,
            "--tag",
            &tag(),
            "--out",
            out.to_str().unwrap(),
            "--bin-dir",
            ws.to_str().unwrap(),
            "--bin-dir",
            om.to_str().unwrap(),
            "--extra",
            repo().join("LICENSE").to_str().unwrap(),
            "--extra",
            repo().join("README.md").to_str().unwrap(),
            "--mtime",
            mtime,
        ]);
        assert!(o.status.success(), "package {t} failed");
    }
    assert!(rel(&["sums", out.to_str().unwrap()]).status.success());
}

fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
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

fn wait_health(port: u16, path: &str, secs: u64) -> (u16, String, u128) {
    let t0 = Instant::now();
    loop {
        if let Some((c, b)) = http_get(port, path) {
            if c == 200 {
                return (c, b, t0.elapsed().as_millis());
            }
        }
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "{port}{path}: no 200 in {secs}s"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn real_release_rehearsal() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1 (see file header)");
        return;
    }
    let a = tmp("a");
    let b = tmp("b");
    let work = tmp("work");
    let mtime = "1792000000";
    package_all(&a, mtime);

    // 1. verify (ours) and sha256sum -c / tar -tzf (independent tools).
    let v = rel(&["verify", a.to_str().unwrap()]);
    assert!(v.status.success());
    let c = Command::new("sha256sum")
        .arg("-c")
        .arg("SHA256SUMS")
        .current_dir(&a)
        .output()
        .unwrap();
    eprintln!("sha256sum -c:\n{}", String::from_utf8_lossy(&c.stdout));
    assert!(c.status.success(), "sha256sum -c failed");
    let mut names: Vec<Vec<String>> = Vec::new();
    for t in TARGETS {
        let f = a.join(format!("obsidian-open-mind-{}-{t}.tar.gz", tag()));
        let l = Command::new("tar").arg("-tzvf").arg(&f).output().unwrap();
        assert!(l.status.success(), "GNU tar cannot read {}", f.display());
        let listing = String::from_utf8_lossy(&l.stdout).to_string();
        eprintln!("tar -tzvf {t}:\n{listing}");
        let mut n: Vec<String> = listing
            .lines()
            .filter_map(|x| x.split('/').nth(1).map(str::to_string))
            .filter(|x| !x.is_empty())
            .collect();
        n.sort();
        assert!(n.iter().any(|x| x == "MANIFEST") && n.iter().any(|x| x == "LICENSE"));
        assert!(
            !n.iter().any(|x| x == "gaia-dummy" || x.contains("mock")),
            "test helper shipped"
        );
        names.push(n);
    }
    assert_eq!(
        names[0], names[1],
        "x86_64 and aarch64 ship different binary sets"
    );
    assert!(names[0].len() >= 30, "only {} members", names[0].len());

    // 2. Determinism: same inputs + mtime -> identical bytes and sums.
    package_all(&b, mtime);
    for f in fs::read_dir(&a).unwrap() {
        let f = f.unwrap().file_name();
        assert_eq!(
            fs::read(a.join(&f)).unwrap(),
            fs::read(b.join(&f)).unwrap(),
            "{f:?} differs"
        );
    }

    // 3. Rehearse: every binary starts and prints `<name> <version>`.
    let r = rel(&[
        "rehearse",
        a.to_str().unwrap(),
        "--work",
        work.to_str().unwrap(),
        "--timeout",
        "60",
    ]);
    let table = String::from_utf8_lossy(&r.stdout).to_string();
    if let Some(p) = std::env::var_os("GALION_RELEASE_EVIDENCE") {
        fs::write(p, &table).unwrap();
    }
    assert!(r.status.success(), "rehearse failed");
    assert!(
        table.contains("\tqemu-aarch64-static\t"),
        "aarch64 not run under qemu"
    );
    assert!(!table.lines().any(|l| l.starts_with("FAIL")));

    // 4. Started from the packages: tuner + nerve serve /health, both arches.
    let base: u16 = std::env::var("GALION_RELEASE_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20860);
    let qemu = Command::new("sh")
        .args(["-c", "command -v qemu-aarch64-static"])
        .output()
        .unwrap();
    let qemu = String::from_utf8_lossy(&qemu.stdout).trim().to_string();
    assert!(!qemu.is_empty());
    let www = tmp("www");
    let state = tmp("nerve-state");
    for (i, t) in TARGETS.iter().enumerate() {
        let dir = work.join(format!("obsidian-open-mind-{}-{t}", tag()));
        let spawn = |exe: &str| {
            let p = dir.join(exe);
            let mut c = if t.starts_with("aarch64") {
                let mut c = Command::new(&qemu);
                c.arg(p);
                c
            } else {
                Command::new(p)
            };
            c.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            c
        };
        let tp = base + i as u16 * 2;
        let np = tp + 1;
        let _tuner = Kill(
            spawn("tuner")
                .env("TUNER_ADDR", format!("127.0.0.1:{tp}"))
                .env("TUNER_ROOT", &www)
                .spawn()
                .unwrap(),
        );
        let _nerve = Kill(
            spawn("openmind-nerve")
                .env("NERVE_ADDR", format!("127.0.0.1:{np}"))
                .env("NERVE_STATE_DIR", &state)
                .spawn()
                .unwrap(),
        );
        let (c1, b1, ms1) = wait_health(tp, "/health", 60);
        let (c2, b2, ms2) = wait_health(np, "/health", 90);
        eprintln!("{t} tuner :{tp} /health {c1} {ms1} ms {}", b1.trim());
        eprintln!("{t} nerve :{np} /health {c2} {ms2} ms {}", b2.trim());
        assert!(b1.contains("\"ok\":true"));
        assert!(b2.contains("\"service\":\"openmind-nerve\""));
        assert!(b2.contains(&format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"))));
    }

    for d in [&a, &b, &work, &www, &state] {
        let _ = fs::remove_dir_all(d);
    }
}

#[test]
fn real_release_negative_controls() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let d = tmp("neg");
    package_all(&d, "0");
    let ds = d.to_str().unwrap();
    let x86 = d.join(format!(
        "obsidian-open-mind-{}-{}.tar.gz",
        tag(),
        TARGETS[0]
    ));

    // Wrong expected version -> rehearse fails.
    let w = tmp("neg-work");
    let r = rel(&[
        "rehearse",
        ds,
        "--work",
        w.to_str().unwrap(),
        "--expect-version",
        "9.9.9",
    ]);
    assert_eq!(r.status.code(), Some(1));

    // Unlisted archive -> verify fails.
    fs::copy(&x86, d.join("stray.tar.gz")).unwrap();
    let r = rel(&["verify", ds]);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("stray.tar.gz: not listed"));
    fs::remove_file(d.join("stray.tar.gz")).unwrap();

    // One flipped byte -> sha256 mismatch; re-summed -> archive check fails.
    let mut bytes = fs::read(&x86).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    fs::write(&x86, &bytes).unwrap();
    let r = rel(&["verify", ds]);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("sha256 mismatch"));
    let o = Command::new("sha256sum")
        .arg("-c")
        .arg("SHA256SUMS")
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(!o.status.success(), "sha256sum -c must agree");
    assert!(rel(&["sums", ds]).status.success());
    let r = rel(&["verify", ds]);
    assert_eq!(
        r.status.code(),
        Some(1),
        "re-summed tampered archive must still fail"
    );

    // The gate refuses a dynamic (host gnu) binary and an over-budget one.
    let host = tmp("host-bin");
    fs::copy(BIN, host.join("galion-release")).unwrap();
    let r = rel(&[
        "package",
        "--target",
        "x86_64-unknown-linux-musl",
        "--tag",
        "v0.0.0",
        "--out",
        host.join("o").to_str().unwrap(),
        "--bin-dir",
        host.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(1));
    let e = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(e.contains("PT_INTERP") || e.contains("DT_NEEDED"), "{e}");
    let td = target_dir();
    let ws = td.join(TARGETS[1]).join("release");
    let r = rel(&[
        "package",
        "--target",
        TARGETS[1],
        "--tag",
        "v0.0.0",
        "--out",
        host.join("o2").to_str().unwrap(),
        "--bin-dir",
        ws.to_str().unwrap(),
        "--budget",
        "1000000",
    ]);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("> budget 1000000"));
    // Wrong arch dir for the target -> machine mismatch.
    let r = rel(&[
        "package",
        "--target",
        TARGETS[0],
        "--tag",
        "v0.0.0",
        "--out",
        host.join("o3").to_str().unwrap(),
        "--bin-dir",
        ws.to_str().unwrap(),
    ]);
    assert_eq!(r.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&r.stderr).contains("machine AArch64"));
    for p in [&d, &w, &host] {
        let _ = fs::remove_dir_all(p);
    }
}
