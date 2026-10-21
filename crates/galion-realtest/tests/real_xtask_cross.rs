// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 156 (sqlite-aarch64-cc): `cargo xtask cross <triple>` must build the
//! bundled-SQLite crates for aarch64 and armv7 musl from a CLEAN shell: no
//! `CC_*`, `AR_*`, `CARGO_TARGET_*_LINKER`, `RUSTFLAGS` or zig variables, and
//! no shell wrapper script. Before, only a per-shell zig wrapper worked
//! (docs/toolchain-aarch64.md, slot 145).
//!
//! * `cargo_alias_and_member` always runs (reads config files).
//! * `real_xtask_cross` runs with `GALION_REAL=1`:
//!   1. negative control: plain `cargo build --target aarch64-unknown-linux-musl
//!      -p openmind-memory` in the clean env must FAIL in libsqlite3-sys
//!      (the gap exists);
//!   2. `cargo xtask cross aarch64-unknown-linux-musl build --release -p
//!      openmind-memory -p hermes-gateway` and the armv7 equivalent for
//!      openmind-memory, same clean env; ELF machine + static checked;
//!   3. under qemu-aarch64-static: openmind-memory on 127.0.0.1:20260
//!      (`/health`, `POST /capture`, `POST /search/conversations` read-back:
//!      SQLite C code really runs), hermes-gateway `--mock` (offline mock
//!      provider, no network, no spend) on 20262 (`/health`, state DB file
//!      created by SQLite); under qemu-arm-static: openmind-memory on 20261.
//!
//!   With `GALION_REAL_FULL=1` step 2 builds `--workspace` for both targets
//!   and checks every workspace bin target (xtask excluded; 27 today).
//!   Own target dir `<target>/xtask-cross-real`, so the nested cargo never
//!   waits on the outer `cargo test` lock.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::http;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn cargo_alias_and_member() {
    let cfg = std::fs::read_to_string(root().join(".cargo/config.toml")).unwrap();
    assert!(
        cfg.contains("[alias]")
            && cfg.contains("xtask = \"run --quiet --package xtask --bin xtask --\""),
        ".cargo/config.toml must define the xtask alias"
    );
    // The C compiler must not be hard-coded per machine in repo config.
    for bad in ["CC_aarch64", "AR_aarch64", "zigw", "cc-aarch64"] {
        let live = cfg
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| l.contains(bad));
        assert!(!live, "{bad} set in .cargo/config.toml");
    }
    let ws = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    assert!(ws.contains(r#"members = ["crates/*", "xtask"]"#));
    // xtask stays dependency-free (std only).
    let x = std::fs::read_to_string(root().join("xtask/Cargo.toml")).unwrap();
    let deps = x.split("[dependencies]").nth(1).unwrap_or("");
    assert!(
        deps.lines()
            .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#')),
        "xtask must have no dependencies"
    );
}

/// (e_machine, PT_INTERP present, PT_DYNAMIC present) for ELF32/ELF64 LE.
fn elf_info(p: &Path) -> (u16, bool, bool) {
    let b = std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
    assert_eq!(&b[..4], b"\x7fELF", "{} is not ELF", p.display());
    assert_eq!(b[5], 1, "little endian");
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let (phoff, phentsize, phnum) = match b[4] {
        1 => (
            u32_at(28) as usize,
            u16_at(42) as usize,
            u16_at(44) as usize,
        ),
        2 => (
            u64_at(32) as usize,
            u16_at(54) as usize,
            u16_at(56) as usize,
        ),
        c => panic!("bad ELF class {c}"),
    };
    let (mut interp, mut dynamic) = (false, false);
    for i in 0..phnum {
        let t = u32_at(phoff + i * phentsize);
        interp |= t == 3;
        dynamic |= t == 2;
    }
    (u16_at(18), interp, dynamic)
}

/// A clean environment: only what finds cargo/rustup. Everything else is
/// dropped, in particular CC_*, AR_*, *_LINKER, RUSTFLAGS, ZIG, GALION_ZIG.
fn clean(cmd: &mut Command, td: &Path) {
    let cargo = PathBuf::from(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    let mut path = String::new();
    if let Some(d) = cargo.parent().filter(|d| !d.as_os_str().is_empty()) {
        path.push_str(&format!("{}:", d.display()));
    }
    path.push_str("/usr/bin:/bin");
    cmd.env_clear()
        .env("PATH", path)
        .env("CARGO_TARGET_DIR", td);
    for k in ["HOME", "RUSTUP_HOME", "CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
}

fn run(td: &Path, args: &[&str]) -> (Output, f64) {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new("nice");
    cmd.arg("-n").arg("10").arg(&cargo).args(args);
    cmd.current_dir(root());
    clean(&mut cmd, td);
    eprintln!("$ (clean env) cargo {}", args.join(" "));
    let t0 = Instant::now();
    let out = cmd.output().expect("run cargo");
    (out, t0.elapsed().as_secs_f64())
}

fn tail(b: &[u8], n: usize) -> String {
    let s = String::from_utf8_lossy(b);
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Workspace bin target names (xtask excluded), from `cargo metadata`.
fn workspace_bins(td: &Path) -> Vec<String> {
    let (out, _) = run(td, &["metadata", "--no-deps", "--format-version", "1"]);
    assert!(out.status.success(), "cargo metadata");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut bins = Vec::new();
    for p in v["packages"].as_array().unwrap() {
        if p["name"] == "xtask" {
            continue;
        }
        for t in p["targets"].as_array().unwrap() {
            if t["kind"].as_array().unwrap().iter().any(|k| k == "bin") {
                bins.push(t["name"].as_str().unwrap().to_string());
            }
        }
    }
    bins.sort();
    bins
}

struct Kill(Vec<Child>);
impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn wait_up(addr: &str, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if http::port_open(addr) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|f| f.is_file())
    })
}

fn get_ok(addr: &str, path: &str, want: &str) {
    assert!(wait_up(addr, 60), "{addr}: nothing listening");
    let r = http::get(addr, path).unwrap_or_else(|e| panic!("GET {addr}{path}: {e}"));
    let body = r.text();
    eprintln!(
        "GET {addr}{path} -> {} {}",
        r.status,
        body.chars()
            .take(100)
            .collect::<String>()
            .replace('\n', " ")
    );
    assert_eq!(r.status, 200, "{addr}{path}");
    assert!(body.contains(want), "{addr}{path}: wanted {want}");
}

fn memory_roundtrip(addr: &str, tag: &str) {
    get_ok(addr, "/health", "status");
    let body = format!(
        r#"{{"session_key":"{tag}","user_content":"{tag} zigcc sqlite write","assistant_content":"stored under qemu"}}"#
    );
    let r = http::post_json(addr, "/capture", &[], &body).expect("capture");
    eprintln!("POST {addr}/capture -> {}", r.status);
    assert!(r.status < 300, "capture status {}", r.status);
    let q = format!(r#"{{"query":"zigcc","session_key":"{tag}"}}"#);
    let r = http::post_json(addr, "/search/conversations", &[], &q).expect("search");
    let hit = r.text().contains(&format!("{tag} zigcc"));
    eprintln!("POST {addr}/search/conversations -> {} hit={hit}", r.status);
    assert!(hit, "SQLite read-back under qemu ({tag})");
}

#[test]
fn real_xtask_cross() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (clean-env cargo xtask cross + qemu run)");
        return;
    }
    const A64: &str = "aarch64-unknown-linux-musl";
    const ARM: &str = "armv7-unknown-linux-musleabihf";
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let td = base.join("xtask-cross-real");
    let full = std::env::var("GALION_REAL_FULL").as_deref() == Ok("1");

    // 1. negative control: the gap without xtask.
    let (out, secs) = run(
        &td,
        &[
            "build",
            "--release",
            "-j",
            "2",
            "-p",
            "openmind-memory",
            "--target",
            A64,
        ],
    );
    let err = String::from_utf8_lossy(&out.stderr);
    eprintln!(
        "negative control: exit={:?} {secs:.1}s, libsqlite3-sys: no C cross compiler={}",
        out.status.code(),
        err.contains("libsqlite3-sys")
    );
    assert!(
        !out.status.success(),
        "plain cargo build must fail without a C cross compiler"
    );
    assert!(
        err.contains("libsqlite3-sys") && err.contains("failed to find tool"),
        "{}",
        tail(&out.stderr, 15)
    );

    // 2. the fix: cargo xtask cross, same clean env.
    let builds: Vec<(&str, Vec<&str>)> = if full {
        vec![
            (A64, vec!["build", "--release", "-j", "2", "--workspace"]),
            (ARM, vec!["build", "--release", "-j", "2", "--workspace"]),
        ]
    } else {
        vec![
            (
                A64,
                vec![
                    "build",
                    "--release",
                    "-j",
                    "2",
                    "-p",
                    "openmind-memory",
                    "-p",
                    "hermes-gateway",
                ],
            ),
            (
                ARM,
                vec!["build", "--release", "-j", "2", "-p", "openmind-memory"],
            ),
        ]
    };
    for (t, args) in &builds {
        let mut a = vec!["xtask", "cross", *t];
        a.extend(args.iter().copied());
        let (out, secs) = run(&td, &a);
        eprintln!("{}", tail(&out.stderr, 3));
        assert!(
            out.status.success(),
            "cargo xtask cross {t} failed:\n{}",
            tail(&out.stderr, 30)
        );
        eprintln!("cargo xtask cross {t}: ok {secs:.1}s");
    }
    let bins = if full {
        workspace_bins(&td)
    } else {
        vec!["openmind-memory".to_string(), "hermes-gateway".to_string()]
    };
    for (t, machine) in [(A64, 183u16), (ARM, 40u16)] {
        let dir = td.join(t).join("release");
        let mut n = 0;
        for b in &bins {
            if !full && t == ARM && b == "hermes-gateway" {
                continue;
            }
            let (m, interp, dynamic) = elf_info(&dir.join(b));
            assert_eq!(m, machine, "{t}/{b}: e_machine");
            assert!(!interp && !dynamic, "{t}/{b}: not static");
            n += 1;
        }
        eprintln!("{t}: {n} binaries, e_machine={machine}, static (no PT_INTERP/PT_DYNAMIC)");
    }
    if full {
        eprintln!("workspace bins checked per target: {}", bins.len());
    }

    // 3. run them.
    let qa = which("qemu-aarch64-static").or_else(|| which("qemu-aarch64"));
    let qr = which("qemu-arm-static").or_else(|| which("qemu-arm"));
    let (Some(qa), Some(qr)) = (qa, qr) else {
        panic!("qemu-aarch64-static and qemu-arm-static are required for the run step");
    };
    let work = td.join("run");
    let _ = std::fs::remove_dir_all(&work);
    for d in ["mem-a64", "mem-arm", "hermes"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    let spawn = |q: &Path, bin: PathBuf, args: &[&str], env: &[(&str, String)]| -> Child {
        let mut c = Command::new(q);
        c.arg(bin)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c.env_remove("API_SERVER_KEY")
            .env_remove("OPENROUTER_API_KEY");
        for (k, v) in env {
            c.env(k, v);
        }
        c.spawn().expect("spawn under qemu")
    };
    let mem_env = |port: &str, dir: &str| {
        vec![
            ("OPENMIND_MEMORY_HOST", "127.0.0.1".to_string()),
            ("OPENMIND_MEMORY_PORT", port.to_string()),
            (
                "OPENMIND_MEMORY_DATA_DIR",
                work.join(dir).display().to_string(),
            ),
        ]
    };
    let hdir = work.join("hermes");
    let state_db = hdir.join("rs-state.db");
    let kids = Kill(vec![
        spawn(
            &qa,
            td.join(A64).join("release/openmind-memory"),
            &[],
            &mem_env("20260", "mem-a64"),
        ),
        spawn(
            &qr,
            td.join(ARM).join("release/openmind-memory"),
            &[],
            &mem_env("20261", "mem-arm"),
        ),
        spawn(
            &qa,
            td.join(A64).join("release/hermes-gateway"),
            &["--mock"],
            &[
                ("HERMES_HOME", hdir.display().to_string()),
                ("HERMES_RS_HOST", "127.0.0.1".into()),
                ("HERMES_RS_PORT", "20262".into()),
                ("HERMES_RS_STATE_DB", state_db.display().to_string()),
                // Fixed loopback-only test value (gateway needs >= 16 chars); not a real key.
                ("API_SERVER_KEY", "s156-loopback-test-key".into()),
            ],
        ),
    ]);
    memory_roundtrip("127.0.0.1:20260", "s156a64");
    memory_roundtrip("127.0.0.1:20261", "s156armv7");
    get_ok("127.0.0.1:20262", "/health", "ok");
    let db = std::fs::read(&state_db).expect("hermes-gateway state db");
    assert!(db.starts_with(b"SQLite format 3\0"), "state db is SQLite");
    eprintln!("hermes-gateway state db: {} B, SQLite header ok", db.len());
    drop(kids);
    for p in ["20260", "20261", "20262"] {
        assert!(
            !http::port_open(&format!("127.0.0.1:{p}")),
            "{p} still open"
        );
    }
    let _ = std::fs::remove_dir_all(&work);
}
