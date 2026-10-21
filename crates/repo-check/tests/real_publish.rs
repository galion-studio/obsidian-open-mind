// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for slot 201 sec-deny-wildcards (gated: `GALION_REAL=1`).
//!
//! 1. Runs the real `repo-check` binary with `--only publish` on this
//!    workspace (must exit 0) and on a temp copy of every crate manifest with
//!    one `publish = false` removed (must exit 1 and name that file).
//! 2. When `cargo deny` is installed, runs the real `cargo deny check bans`
//!    with this repo's deny.toml on (a) this workspace (must pass) and (b) a
//!    throwaway two-crate workspace with a path dep, once without and once
//!    with `publish = false` (wildcard error / ok). Offline: no registry deps.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sdw201-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn repo_check(dir: &Path) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_repo-check"))
        .args(["--root", dir.to_str().unwrap(), "--only", "publish"])
        .output()
        .unwrap();
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.code().unwrap_or(-1), s)
}

#[test]
fn real_repo_check_publish() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let r = root();
    let (code, out) = repo_check(&r);
    eprintln!("live tree: exit {code}\n{out}");
    assert_eq!(code, 0, "workspace must pass the publish check: {out}");

    // Copy every crates/*, edge/*, sim/* manifest into a temp tree.
    let d = tmp("tree");
    let mut n = 0;
    for top in ["crates", "edge", "sim"] {
        let Ok(rd) = fs::read_dir(r.join(top)) else {
            continue;
        };
        for e in rd.flatten() {
            let m = e.path().join("Cargo.toml");
            if m.is_file() {
                let dst = d.join(top).join(e.file_name());
                fs::create_dir_all(&dst).unwrap();
                fs::copy(&m, dst.join("Cargo.toml")).unwrap();
                n += 1;
            }
        }
    }
    eprintln!("copied {n} manifests");
    assert!(n >= 65, "expected 63 crates + edge + sim, got {n}");
    let (code, out) = repo_check(&d);
    assert_eq!(code, 0, "copy must pass too: {out}");

    let victim = d.join("crates/zagreb-time/Cargo.toml");
    let text = fs::read_to_string(&victim).unwrap();
    assert!(text.contains("publish = false"));
    fs::write(&victim, text.replace("publish = false\n", "")).unwrap();
    let (code, out) = repo_check(&d);
    eprintln!("stripped zagreb-time: exit {code}\n{out}");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("crates/zagreb-time/Cargo.toml"), "{out}");
    fs::remove_dir_all(&d).unwrap();
}

fn cargo_deny_available() -> bool {
    Command::new("cargo")
        .args(["deny", "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn deny_bans(manifest: &Path, config: &Path) -> (bool, String) {
    let o = Command::new("cargo")
        .args(["deny", "--manifest-path", manifest.to_str().unwrap()])
        .args(["--config", config.to_str().unwrap(), "check", "bans"])
        .output()
        .unwrap();
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.success(), s)
}

fn mini(d: &Path, private: bool) -> PathBuf {
    let p = if private { "publish = false\n" } else { "" };
    let a = d.join("a");
    let b = d.join("b");
    fs::create_dir_all(a.join("src")).unwrap();
    fs::create_dir_all(b.join("src")).unwrap();
    fs::write(
        d.join("Cargo.toml"),
        "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        a.join("Cargo.toml"),
        format!("[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT\"\n{p}\n[dependencies]\nb = {{ path = \"../b\" }}\n"),
    )
    .unwrap();
    fs::write(
        b.join("Cargo.toml"),
        format!("[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"MIT\"\n{p}"),
    )
    .unwrap();
    fs::write(a.join("src/lib.rs"), "").unwrap();
    fs::write(b.join("src/lib.rs"), "").unwrap();
    let o = Command::new("cargo")
        .args(["generate-lockfile", "--offline", "--manifest-path"])
        .arg(d.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    d.join("Cargo.toml")
}

#[test]
fn real_cargo_deny_wildcards() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    assert!(
        cargo_deny_available(),
        "GALION_REAL=1 needs `cargo deny` on PATH (cargo install cargo-deny)"
    );
    let r = root();
    let cfg = r.join("deny.toml");
    assert!(fs::read_to_string(&cfg)
        .unwrap()
        .contains("wildcards = \"deny\""));

    for m in [
        "Cargo.toml",
        "edge/openmind-node/Cargo.toml",
        "sim/deploy-sim/Cargo.toml",
    ] {
        let (ok, out) = deny_bans(&r.join(m), &cfg);
        eprintln!("{m}: ok={ok} {}", out.lines().last().unwrap_or(""));
        assert!(ok, "{m}: {out}");
        assert!(!out.contains("[wildcard]"), "{m}: {out}");
    }

    let d = tmp("pub");
    let (ok, out) = deny_bans(&mini(&d, false), &cfg);
    eprintln!("mini public: ok={ok}");
    assert!(!ok, "public path dep must fail: {out}");
    assert!(out.contains("error[wildcard]"), "{out}");
    fs::remove_dir_all(&d).unwrap();

    let d = tmp("priv");
    let (ok, out) = deny_bans(&mini(&d, true), &cfg);
    eprintln!("mini private: ok={ok}");
    assert!(ok, "private path dep must pass: {out}");
    fs::remove_dir_all(&d).unwrap();
}
