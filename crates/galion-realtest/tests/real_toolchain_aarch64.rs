// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 145 (toolchain-aarch64): the repo-level `.cargo/config.toml` must make
//! `cargo build --target aarch64-unknown-linux-musl` link with rust-lld with
//! no env var set (before, every agent had to export
//! `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` by hand).
//!
//! * `cargo_config_cross_linkers` always runs: it reads the config file.
//! * `real_aarch64_build_and_run` runs only with `GALION_REAL=1`. It builds
//!   tuner and galion-one (pure Rust) for aarch64 musl with every
//!   `CARGO_TARGET_*_LINKER` and `RUSTFLAGS` removed from the environment,
//!   checks the ELF (AArch64, static: no PT_INTERP, no PT_DYNAMIC), then runs
//!   both under `qemu-aarch64-static` on 127.0.0.1:20150-20152 and probes
//!   their real HTTP endpoints. When `CC_aarch64_unknown_linux_musl` is set
//!   (a C cross compiler, e.g. the zig cc wrapper in docs/toolchain-aarch64.md)
//!   it also builds openmind-memory (bundled SQLite) and probes it on 20153.
//!   It uses its own target dir (`<target>/toolchain-aarch64-real`) so the
//!   nested cargo never waits on the outer `cargo test` lock.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::http;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `[section]` -> list of `key = value` lines (comments stripped). Enough
/// TOML for this flat file; no toml dependency needed.
fn sections(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            out.push((line[1..line.len() - 1].trim().to_string(), Vec::new()));
        } else if let (Some((k, v)), Some(last)) = (line.split_once('='), out.last_mut()) {
            last.1.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn get<'a>(s: &'a [(String, Vec<(String, String)>)], sec: &str, key: &str) -> Option<&'a str> {
    s.iter()
        .find(|(n, _)| n == sec)
        .and_then(|(_, kv)| kv.iter().find(|(k, _)| k == key))
        .map(|(_, v)| v.as_str())
}

#[test]
fn cargo_config_cross_linkers() {
    let text = std::fs::read_to_string(root().join(".cargo/config.toml")).expect("config.toml");
    let s = sections(&text);
    for t in [
        "aarch64-unknown-linux-musl",
        "armv7-unknown-linux-musleabihf",
    ] {
        assert_eq!(
            get(&s, &format!("target.{t}"), "linker"),
            Some("\"rust-lld\""),
            "{t}: linker must be rust-lld"
        );
    }
    // target.<triple>.rustflags would REPLACE [build] rustflags and drop the
    // serial curve25519 backend (no-SIMD rule); keep targets linker-only.
    for (name, kv) in &s {
        if name.starts_with("target.") {
            assert!(
                kv.iter().all(|(k, _)| k == "linker"),
                "[{name}] may only set linker, found {kv:?}"
            );
        }
    }
    assert!(get(&s, "build", "rustflags").is_some_and(|v| v.contains("serial")));
    // No CPU-pinning codegen flags anywhere (UNIVERSAL-CPU.md).
    for bad in ["target-cpu", "target-feature"] {
        assert!(!text.contains(bad), "{bad} in .cargo/config.toml");
    }
}

/// e_machine and static-ness of an ELF64 little-endian file.
fn elf_info(p: &Path) -> (u16, bool, bool) {
    let b = std::fs::read(p).expect("read elf");
    assert_eq!(&b[..4], b"\x7fELF", "{} is not ELF", p.display());
    assert_eq!(b[4], 2, "ELF64");
    assert_eq!(b[5], 1, "little endian");
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let machine = u16_at(18);
    let phoff = u64_at(32) as usize;
    let phentsize = u16_at(54) as usize;
    let phnum = u16_at(56) as usize;
    let (mut interp, mut dynamic) = (false, false);
    for i in 0..phnum {
        let t = u32::from_le_bytes(b[phoff + i * phentsize..][..4].try_into().unwrap());
        interp |= t == 3;
        dynamic |= t == 2;
    }
    (machine, interp, dynamic)
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

#[test]
fn real_aarch64_build_and_run() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (cross build + qemu-aarch64-static run)");
        return;
    }
    const T: &str = "aarch64-unknown-linux-musl";
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let td = base.join("toolchain-aarch64-real");
    let with_c = std::env::var_os("CC_aarch64_unknown_linux_musl").is_some();
    let mut pkgs = vec!["tuner", "galion-one"];
    if with_c {
        pkgs.push("openmind-memory");
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new("nice");
    cmd.args([
        "-n",
        "10",
        &cargo,
        "build",
        "--release",
        "--locked",
        "-j",
        "2",
    ]);
    for p in &pkgs {
        cmd.args(["-p", p]);
    }
    cmd.args(["--target", T]).current_dir(root());
    cmd.env("CARGO_TARGET_DIR", &td).env_remove("RUSTFLAGS");
    for (k, _) in std::env::vars() {
        if k.starts_with("CARGO_TARGET_") && k.ends_with("_LINKER") {
            cmd.env_remove(k);
        }
    }
    let t0 = Instant::now();
    let st = cmd.status().expect("run cargo");
    assert!(st.success(), "aarch64 build without linker env failed");
    eprintln!("build {pkgs:?} for {T}: {:.1}s", t0.elapsed().as_secs_f64());

    let bin = td.join(T).join("release");
    for p in &pkgs {
        let (m, interp, dynamic) = elf_info(&bin.join(p));
        eprintln!("{p}: e_machine={m} PT_INTERP={interp} PT_DYNAMIC={dynamic}");
        assert_eq!(m, 183, "{p}: not AArch64 (EM_AARCH64 = 183)");
        assert!(!interp && !dynamic, "{p}: not a static binary");
    }

    let Some(qemu) = which("qemu-aarch64-static").or_else(|| which("qemu-aarch64")) else {
        eprintln!("qemu-aarch64 missing: ELF checks only");
        return;
    };
    let work = td.join("run");
    let _ = std::fs::remove_dir_all(&work);
    for d in ["one", "www", "mem"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(
        work.join("www/index.html"),
        "<!doctype html><title>slot145 aarch64</title>\n",
    )
    .unwrap();
    let spawn = |b: &str, env: &[(&str, PathBuf)]| -> Child {
        let mut c = Command::new(&qemu);
        c.arg(bin.join(b))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        c.spawn().expect("spawn under qemu")
    };
    let p = PathBuf::from;
    let mut kids = Kill(vec![
        spawn(
            "tuner",
            &[
                ("TUNER_ADDR", p("127.0.0.1:20150")),
                ("TUNER_ROOT", work.join("www")),
            ],
        ),
        spawn(
            "galion-one",
            &[
                ("GALION_HOME", work.join("one")),
                ("ONE_BIND", p("127.0.0.1:20151")),
                ("ONE_MOUTH_BIND", p("127.0.0.1:20152")),
                ("ONE_NO_WARM", p("1")),
            ],
        ),
    ]);
    if with_c {
        kids.0.push(spawn(
            "openmind-memory",
            &[
                ("OPENMIND_MEMORY_HOST", p("127.0.0.1")),
                ("OPENMIND_MEMORY_PORT", p("20153")),
                ("OPENMIND_MEMORY_DATA_DIR", work.join("mem")),
            ],
        ));
    }
    let mut checks = vec![
        ("127.0.0.1:20150", "/health", "\"ok\":true"),
        ("127.0.0.1:20150", "/", "slot145 aarch64"),
        ("127.0.0.1:20151", "/health", "ok"),
    ];
    if with_c {
        checks.push(("127.0.0.1:20153", "/health", "status"));
    }
    for (addr, path, want) in checks {
        assert!(wait_up(addr, 60), "{addr}: nothing listening");
        let r = http::get(addr, path).unwrap_or_else(|e| panic!("GET {addr}{path}: {e}"));
        let body = r.text();
        eprintln!(
            "GET {addr}{path} -> {} {}",
            r.status,
            body.chars()
                .take(120)
                .collect::<String>()
                .replace('\n', " ")
        );
        assert_eq!(r.status, 200, "{addr}{path}");
        assert!(body.contains(want), "{addr}{path}: wanted {want}");
    }
    if with_c {
        let r = http::post_json(
            "127.0.0.1:20153",
            "/capture",
            &[],
            r#"{"session_key":"slot145","user_content":"aarch64 rust-lld write","assistant_content":"stored under qemu"}"#,
        )
        .expect("capture");
        eprintln!("POST /capture -> {}", r.status);
        assert!(r.status < 300, "capture status {}", r.status);
        let r = http::post_json(
            "127.0.0.1:20153",
            "/search/conversations",
            &[],
            r#"{"query":"rust-lld","session_key":"slot145"}"#,
        )
        .expect("search");
        eprintln!(
            "POST /search/conversations -> {} hit={}",
            r.status,
            r.text().contains("rust-lld")
        );
        assert!(r.text().contains("rust-lld"), "SQLite read-back under qemu");
    }
    drop(kids);
    let _ = std::fs::remove_dir_all(&work);
}
