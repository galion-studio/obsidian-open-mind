// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 377 (F-357-1): the build stamp in `gaia-cortex --version` and
//! `/health`, proven on real release builds, and the reproducible-build check
//! kept: the stamp never reads the wall clock.
//!
//! Gated: `GALION_REAL=1`. Builds the committed HEAD four times
//! (x86_64 musl release, features `GAIA_CORTEX_REAL_FEATURES`, default
//! `llm-advisor`, like the live `-llm` binary):
//!
//!   A  `git archive HEAD` in dir 1 + GALION_BUILD_COMMIT/SOURCE_DATE_EPOCH
//!   B  `git archive HEAD` in a different dir + same env
//!   C  `git clone` checkout of HEAD, no env (stamp from git)
//!   D  `git archive HEAD`, no env (no git -> `unknown`)
//!
//! Asserts sha256(A) = sha256(B) = sha256(C) (byte-identical: same commit ->
//! same binary, whether stamped from env or git), D differs only by stamp,
//! then serves A, C, D on 127.0.0.1:22470-22472 (REAL_CORTEX_PORT_BASE moves
//! it) and checks `/health` `commit` / `build`.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TARGET: &str = "x86_64-unknown-linux-musl";

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn log(m: impl AsRef<str>) {
    eprintln!(
        "[real_build_stamp {}] {}",
        zagreb_time::now_rfc3339(),
        m.as_ref()
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn git(args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn archive_into(commit: &str, dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let mut a = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["archive", commit])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let t = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dir)
        .stdin(a.stdout.take().unwrap())
        .status()
        .unwrap();
    assert!(
        a.wait().unwrap().success() && t.success(),
        "archive {commit}"
    );
}

/// Release build in `src` with its own target dir; returns the binary path.
fn build(src: &Path, target_dir: &Path, stamp: Option<(&str, &str)>) -> PathBuf {
    let feats = std::env::var("GAIA_CORTEX_REAL_FEATURES").unwrap_or("llm-advisor".into());
    let mut c = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()));
    c.current_dir(src)
        .args([
            "build",
            "-j",
            "2",
            "--release",
            "--locked",
            "--offline",
            "-p",
            "gaia-cortex",
        ])
        .args(["--target", TARGET])
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("GALION_BUILD_COMMIT")
        .env_remove("SOURCE_DATE_EPOCH")
        .env_remove("RUSTFLAGS");
    if !feats.is_empty() {
        c.args(["--features", &feats]);
    }
    if let Some((commit, epoch)) = stamp {
        c.env("GALION_BUILD_COMMIT", commit)
            .env("SOURCE_DATE_EPOCH", epoch);
    }
    let t = Instant::now();
    let st = c.status().unwrap();
    assert!(st.success(), "cargo build in {}", src.display());
    log(format!(
        "built {} in {:.1} s",
        src.display(),
        t.elapsed().as_secs_f64()
    ));
    target_dir.join(TARGET).join("release/gaia-cortex")
}

fn sha(p: &Path) -> String {
    galion_common::sha256::hex(&std::fs::read(p).unwrap())
}

fn version(p: &Path) -> String {
    let o = Command::new(p).arg("--version").output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap().trim_end().to_string()
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn health(port: u16) -> Option<Value> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    serde_json::from_str(out.split_once("\r\n\r\n")?.1).ok()
}

fn serve(bin: &Path, port: u16, data: &Path) -> (Kill, Value) {
    let k = Kill(
        Command::new(bin)
            .arg("serve")
            .env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
            .env("CORTEX_DATA_DIR", data)
            .env_remove("CORTEX_CONFIG")
            .env_remove("CORTEX_NERVE_URL")
            .env_remove("CORTEX_LLM_ENABLED")
            .env_remove("CORTEX_HERMES_HANDOFF")
            .env_remove("CORTEX_ACTIONS")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if let Some(h) = health(port) {
            log(format!(
                "{port} /health after {} ms",
                t.elapsed().as_millis()
            ));
            return (k, h);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("no /health on {port}");
}

#[test]
fn real_build_stamp_reproducible() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let base: u16 = std::env::var("REAL_CORTEX_PORT_BASE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(22470);
    let head = git(&["rev-parse", "HEAD"]);
    let ct = git(&["show", "-s", "--format=%ct", "HEAD"]);
    let date = zagreb_time::from_unix(ct.parse().unwrap());
    let tmp = std::env::var("GAIA_CORTEX_REAL_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::temp_dir().join(format!("cortex-stamp-{}", std::process::id()))
        });
    let _ = std::fs::remove_dir_all(&tmp);
    log(format!(
        "HEAD {head} committed {date}; tmp {}",
        tmp.display()
    ));

    let a_src = tmp.join("a/src");
    let b_src = tmp.join("b/elsewhere/deeper/src");
    let c_src = tmp.join("c/checkout");
    let d_src = tmp.join("d/src");
    archive_into(&head, &a_src);
    archive_into(&head, &b_src);
    archive_into(&head, &d_src);
    let st = Command::new("git")
        .args(["clone", "-q", "--no-hardlinks"])
        .arg(root())
        .arg(&c_src)
        .status()
        .unwrap();
    assert!(st.success());
    let st = Command::new("git")
        .arg("-C")
        .arg(&c_src)
        .args(["checkout", "-q", "--detach", &head])
        .status()
        .unwrap();
    assert!(st.success());

    let a = build(&a_src, &tmp.join("a/t"), Some((&head, &ct)));
    let b = build(&b_src, &tmp.join("b/t"), Some((&head, &ct)));
    let c = build(&c_src, &tmp.join("c/t"), None);
    let d = build(&d_src, &tmp.join("d/t"), None);
    let (sa, sb, sc, sd) = (sha(&a), sha(&b), sha(&c), sha(&d));
    for (n, p, s) in [
        ("A", &a, &sa),
        ("B", &b, &sb),
        ("C", &c, &sc),
        ("D", &d, &sd),
    ] {
        log(format!(
            "{n} {} B sha256 {s} --version {:?}",
            std::fs::metadata(p).unwrap().len(),
            version(p)
        ));
    }
    assert_eq!(sa, sb, "archive builds in two dirs must be byte-identical");
    assert_eq!(sa, sc, "env-stamped archive build = git checkout build");
    assert_ne!(sa, sd, "unstamped build differs only by the stamp");

    let va = version(&a);
    assert!(
        va.contains(&format!("(commit {}, built {date},", &head[..8])),
        "{va}"
    );
    assert_eq!(va, version(&c));
    let vd = version(&d);
    assert!(vd.contains("(commit unknown, built unknown,"), "{vd}");

    let (_ka, ha) = serve(&a, base, &tmp.join("a/data"));
    let (_kc, hc) = serve(&c, base + 1, &tmp.join("c/data"));
    let (_kd, hd) = serve(&d, base + 2, &tmp.join("d/data"));
    log(format!(
        "A /health commit {} build {}",
        ha["commit"], ha["build"]
    ));
    log(format!(
        "C /health commit {} build {}",
        hc["commit"], hc["build"]
    ));
    log(format!(
        "D /health commit {} build {}",
        hd["commit"], hd["build"]
    ));
    assert_eq!(ha["ok"], true);
    assert_eq!(ha["commit"], head.as_str());
    assert_eq!(ha["build"]["date"], date.as_str());
    assert_eq!(hc["commit"], head.as_str());
    assert_eq!(hc["build"]["date"], date.as_str());
    assert_eq!(hd["commit"], "unknown");
    assert_eq!(hd["build"]["date"], "unknown");
    drop((_ka, _kc, _kd));
    if std::env::var("GAIA_CORTEX_REAL_KEEP").is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
