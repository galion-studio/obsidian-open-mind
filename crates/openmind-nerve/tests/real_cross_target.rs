// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.5 cross-target real test (gated: `GALION_REAL=1`, box only).
//!
//! Repeats slot 217's measurement end to end:
//! 1. builds `openmind-nerve` release for x86_64 and aarch64 musl
//!    (`-p openmind-nerve` alone), plus the live recipe
//!    (`-p openmind-nerve -p openmind-memory`, x86_64 musl) that produced
//!    the VPS binary `openmind-nerve-aa77864`;
//! 2. checks every binary is a static ELF of the right machine, under the
//!    2 MiB budget, and records its size and SHA-256 (in-test SHA-256, no
//!    extra tool or crate);
//! 3. compares with the live binary (`NERVE_XT_LIVE_SHA`/`NERVE_XT_LIVE_SIZE`,
//!    default = aa77864 as measured on the VPS 2026-10-21 21:02 +02:00);
//! 4. runs each binary (`aarch64` under qemu user mode) on slot ports
//!    20870-20872 against one live service thread (:20873) and one dead
//!    port (:20874): `GET /health`, `POST /signal`, `GET /state`,
//!    `GET /api/v1/signals`, and compares the answers across targets;
//! 5. `cargo check` for aarch64-linux-android and armv7-unknown-linux-musleabihf.
//!
//! Knobs: `NERVE_XT_DIR` (nested target dir, default `<workspace>/target/xt`),
//! `NERVE_XT_JOBS` (default 2), `NERVE_XT_QEMU` (qemu-aarch64 path),
//! `NERVE_XT_REPORT` (JSON report path, default `<NERVE_XT_DIR>/report.json`).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// VPS `/opt/galion/rust/bin/openmind-nerve-aa77864`, read-only sha256sum/stat
/// 2026-10-21 21:02:28 +02:00 (docs/real-tests/nerve-cross-target.md).
const LIVE_SHA: &str = "f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68";
const LIVE_SIZE: u64 = 1_213_216;
const BUDGET: u64 = 2 * 1024 * 1024;
const PORT_SVC: u16 = 20873;
const PORT_DEAD: u16 = 20874;

// ---------------------------------------------------------------- SHA-256
// FIPS 180-4, straight from the spec; used only to fingerprint the binaries.
fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, c) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_known_answers() {
    // FIPS 180-4 examples; always on (cheap), so the fingerprint code is trusted.
    assert_eq!(
        sha256(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

// ---------------------------------------------------------------- ELF
#[derive(Debug, PartialEq)]
struct Elf {
    machine: u16,
    has_interp: bool,
    pie: bool,
}

fn elf(b: &[u8]) -> Elf {
    assert!(b.len() > 64 && &b[..4] == b"\x7fELF", "not an ELF");
    assert_eq!(b[4], 2, "not ELF64");
    assert_eq!(b[5], 1, "not little-endian");
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let phoff = u64_at(32) as usize;
    let phentsize = u16_at(54) as usize;
    let phnum = u16_at(56) as usize;
    let has_interp = (0..phnum).any(|i| u32_at(phoff + i * phentsize) == 3); // PT_INTERP
    Elf {
        machine: u16_at(18),
        has_interp,
        pie: u16_at(16) == 3, // ET_DYN
    }
}

// ---------------------------------------------------------------- HTTP
struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    write!(
        s,
        "{method} {path} HTTP/1.0\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Key order as sent on the wire (the test's own serde_json may sort keys on
/// parse, so order is read from the raw body). Flat objects only.
fn wire_order(raw: &str, v: &Value) -> Vec<String> {
    let mut k: Vec<(usize, String)> = keys(v)
        .into_iter()
        .map(|k| (raw.find(&format!("\"{k}\":")).unwrap_or(usize::MAX), k))
        .collect();
    k.sort();
    k.into_iter().map(|x| x.1).collect()
}

fn sorted(mut k: Vec<String>) -> Vec<String> {
    k.sort();
    k
}

// ---------------------------------------------------------------- build
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

fn run_cargo(args: &[&str], dir: &Path) -> (bool, f64, String) {
    let jobs = std::env::var("NERVE_XT_JOBS").unwrap_or_else(|_| "2".into());
    let t = Instant::now();
    let out = Command::new(cargo())
        .args(args)
        .args(["-j", &jobs, "--target-dir"])
        .arg(dir)
        .current_dir(root())
        .env("CARGO_INCREMENTAL", "0")
        .env(
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER",
            std::env::var("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER")
                .unwrap_or_else(|_| "rust-lld".into()),
        )
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn cargo");
    let err = String::from_utf8_lossy(&out.stderr);
    let tail: String = err.lines().rev().take(8).collect::<Vec<_>>().join(" | ");
    (out.status.success(), t.elapsed().as_secs_f64(), tail)
}

fn find_qemu() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NERVE_XT_QEMU") {
        return Some(PathBuf::from(p));
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for name in ["qemu-aarch64-static", "qemu-aarch64"] {
        for d in path.split(':') {
            let p = Path::new(d).join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

// ---------------------------------------------------------------- smoke
struct Smoke {
    health: Value,
    health_raw: String,
    signal_code: u16,
    bad_signal_code: u16,
    state: Value,
    signals: Value,
    start_s: f64,
}

fn smoke(label: &str, prog: &Path, args: &[&Path], port: u16, dir: &Path) -> Smoke {
    let data = dir.join(format!("data-{label}"));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let comps = dir.join(format!("components-{label}.json"));
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"xt-live","port":{PORT_SVC}}},{{"name":"xt-dead","port":{PORT_DEAD},"gaia":"APOLLO"}}]"#
        ),
    )
    .unwrap();
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", &data)
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_ROLLBACK", "0")
        .env("NERVE_DISK_PATH", &data)
        .env("NERVE_LISTEN_SCOPE", "20870-20879")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let t = Instant::now();
    let _nerve = Kill(cmd.spawn().expect("spawn nerve"));
    // First cycle must be done (cycle >= 1) so /state has components.
    let (health, health_raw) = loop {
        if let Some((200, b)) = http(port, "GET", "/health", "") {
            let v: Value = serde_json::from_str(&b).unwrap_or_default();
            if v["cycle"].as_u64().unwrap_or(0) >= 1 {
                break (v, b);
            }
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "{label}: no /health with cycle>=1 within 60 s"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let start_s = t.elapsed().as_secs_f64();
    let sig =
        format!(r#"{{"source":"xt-{label}","kind":"error","name":"cross","msg":"slot 217"}}"#);
    let signal_code = http(port, "POST", "/signal", &sig)
        .map(|r| r.0)
        .unwrap_or(0);
    let bad_signal_code = http(port, "POST", "/signal", r#"{"source":"x"}"#)
        .map(|r| r.0)
        .unwrap_or(0);
    let state = http(port, "GET", "/state", "")
        .filter(|r| r.0 == 200)
        .and_then(|r| serde_json::from_str(&r.1).ok())
        .unwrap_or_default();
    let signals = http(port, "GET", "/api/v1/signals", "")
        .filter(|r| r.0 == 200)
        .and_then(|r| serde_json::from_str(&r.1).ok())
        .unwrap_or_default();
    drop(_nerve);
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_file(&comps);
    Smoke {
        health,
        health_raw,
        signal_code,
        bad_signal_code,
        state,
        signals,
        start_s,
    }
}

fn comp_status(st: &Value, name: &str) -> String {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == name))
        .map(|c| c["status"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

#[test]
fn real_cross_target() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let started = zagreb_time::now_rfc3339();
    let xt = std::env::var("NERVE_XT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("target/xt"));
    std::fs::create_dir_all(&xt).unwrap();
    let live_sha = std::env::var("NERVE_XT_LIVE_SHA").unwrap_or_else(|_| LIVE_SHA.into());
    let live_size: u64 = std::env::var("NERVE_XT_LIVE_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(LIVE_SIZE);

    // 1. builds -------------------------------------------------------
    let x86 = "x86_64-unknown-linux-musl";
    let a64 = "aarch64-unknown-linux-musl";
    let builds: [(&str, &str, &[&str]); 3] = [
        ("x86_64", x86, &["-p", "openmind-nerve"]),
        ("aarch64", a64, &["-p", "openmind-nerve"]),
        (
            "x86_64-live-recipe",
            x86,
            &["-p", "openmind-nerve", "-p", "openmind-memory"],
        ),
    ];
    let mut bins = Vec::new();
    let mut report_bins = Vec::new();
    for (label, target, pkgs) in builds {
        let dir = xt.join(label);
        let mut args = vec!["build", "--release", "--locked", "--target", target];
        args.extend_from_slice(pkgs);
        let (ok, secs, tail) = run_cargo(&args, &dir);
        assert!(ok, "{label}: cargo build failed: {tail}");
        let bin = dir.join(target).join("release/openmind-nerve");
        let bytes = std::fs::read(&bin).expect("binary");
        let e = elf(&bytes);
        let want_machine = if target == a64 { 0xB7 } else { 0x3E };
        assert_eq!(e.machine, want_machine, "{label}: wrong e_machine");
        assert!(!e.has_interp, "{label}: has PT_INTERP (not static)");
        let size = bytes.len() as u64;
        assert!(size < BUDGET, "{label}: {size} B over 2 MiB budget");
        let sha = sha256(&bytes);
        eprintln!(
            "[xt] {label:<20} {size:>9} B  sha256 {sha}  pie={} build {secs:.1} s",
            e.pie
        );
        report_bins.push(json!({
            "label": label, "target": target, "packages": pkgs, "size": size,
            "sha256": sha, "pie": e.pie, "static": !e.has_interp,
            "e_machine": format!("{:#x}", e.machine), "build_s": (secs * 10.0).round() / 10.0,
            "size_vs_live": size as i64 - live_size as i64, "equals_live": sha == live_sha,
        }));
        bins.push((label, bin, sha, size));
    }
    // The live recipe must reproduce the live binary bit for bit; the solo
    // build differs only by serde_json preserve_order (feature unification).
    let (_, _, recipe_sha, recipe_size) = &bins[2];
    assert_eq!(
        recipe_sha, &live_sha,
        "live recipe no longer reproduces the VPS binary"
    );
    assert_eq!(*recipe_size, live_size);
    assert_ne!(
        bins[0].2, live_sha,
        "solo -p openmind-nerve build now equals live (preserve_order no longer differs?)"
    );

    // 2. qemu smoke -----------------------------------------------------
    let qemu = find_qemu().expect("qemu-aarch64 not found (set NERVE_XT_QEMU)");
    let svc = TcpListener::bind(("127.0.0.1", PORT_SVC)).expect("20873 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    assert!(
        TcpStream::connect(("127.0.0.1", PORT_DEAD)).is_err(),
        "20874 must be free (dead component)"
    );
    // --version under qemu (sanity: the aarch64 binary really executes).
    let v = Command::new(&qemu)
        .arg(&bins[1].1)
        .arg("--version")
        .output()
        .expect("qemu --version");
    let ver = String::from_utf8_lossy(&v.stdout).trim().to_string();
    assert!(
        ver.starts_with("openmind-nerve "),
        "qemu --version: {ver:?}"
    );

    let runs = [
        ("x86_64", bins[0].1.clone(), None, 20870u16),
        ("aarch64", bins[1].1.clone(), Some(qemu.clone()), 20871),
        ("x86_64-live-recipe", bins[2].1.clone(), None, 20872),
    ];
    let mut smokes = Vec::new();
    let mut report_smoke = Vec::new();
    for (label, bin, emu, port) in runs {
        let s = match &emu {
            Some(q) => smoke(label, q, &[bin.as_path()], port, &xt),
            None => smoke(label, &bin, &[], port, &xt),
        };
        assert_eq!(
            s.health["service"], "openmind-nerve",
            "{label}: {}",
            s.health
        );
        assert_eq!(s.health["status"], "ok", "{label}");
        assert_eq!(s.signal_code, 202, "{label}: POST /signal");
        assert_eq!(s.bad_signal_code, 400, "{label}: POST /signal without kind");
        assert_eq!(comp_status(&s.state, "xt-live"), "healthy", "{label}");
        assert_eq!(comp_status(&s.state, "xt-dead"), "failing", "{label}");
        let src = format!("xt-{label}");
        let seen = s.signals.to_string().contains(&src);
        assert!(seen, "{label}: signal source {src} not in /api/v1/signals");
        assert!(
            TcpStream::connect(("127.0.0.1", port)).is_err(),
            "{label}: port {port} still open after kill"
        );
        eprintln!(
            "[xt] smoke {label:<20} :{port} first cycle {:.2} s  /health wire order {:?}  state keys {}  system {}",
            s.start_s,
            wire_order(&s.health_raw, &s.health),
            keys(&s.state).len(),
            s.state["system"]["status"]
        );
        report_smoke.push(json!({
            "label": label, "port": port, "emulated": emu.is_some(),
            "first_cycle_s": (s.start_s * 100.0).round() / 100.0,
            "health": s.health, "health_wire_order": wire_order(&s.health_raw, &s.health),
            "signal_post": s.signal_code, "bad_signal_post": s.bad_signal_code,
            "state_keys": keys(&s.state), "system": s.state["system"]["status"],
            "xt_live": comp_status(&s.state, "xt-live"), "xt_dead": comp_status(&s.state, "xt-dead"),
            "signal_seen": seen,
        }));
        smokes.push(s);
    }
    // Same answers on both architectures (shape, statuses, system verdict).
    let (x, a, r) = (&smokes[0], &smokes[1], &smokes[2]);
    assert_eq!(
        keys(&x.health),
        keys(&a.health),
        "health keys x86 vs aarch64"
    );
    assert_eq!(keys(&x.state), keys(&a.state), "state keys x86 vs aarch64");
    assert_eq!(
        x.state["system"]["status"], a.state["system"]["status"],
        "system verdict x86 vs aarch64"
    );
    // Live recipe (preserve_order on): same key set, source (insertion) order on
    // the wire; solo builds (BTreeMap) send keys sorted.
    assert_eq!(sorted(keys(&r.health)), sorted(keys(&x.health)));
    assert_eq!(sorted(keys(&r.state)), sorted(keys(&x.state)));
    let xo = wire_order(&x.health_raw, &x.health);
    let ao = wire_order(&a.health_raw, &a.health);
    let ro = wire_order(&r.health_raw, &r.health);
    assert_eq!(
        xo,
        sorted(xo.clone()),
        "solo x86_64 /health keys not sorted"
    );
    assert_eq!(ao, xo, "/health wire order x86_64 vs aarch64");
    assert_eq!(
        ro.first().map(String::as_str),
        Some("status"),
        "live recipe order {ro:?}"
    );

    // 3. cargo check for android / armv7 --------------------------------
    let mut report_check = Vec::new();
    for target in ["aarch64-linux-android", "armv7-unknown-linux-musleabihf"] {
        let (ok, secs, tail) = run_cargo(
            &[
                "check",
                "--locked",
                "--target",
                target,
                "-p",
                "openmind-nerve",
            ],
            &xt.join("check"),
        );
        assert!(ok, "{target}: cargo check failed: {tail}");
        eprintln!("[xt] check {target} ok {secs:.1} s");
        report_check
            .push(json!({"target": target, "ok": ok, "secs": (secs * 10.0).round() / 10.0}));
    }

    let report = json!({
        "slot": 217, "started": started, "finished": zagreb_time::now_rfc3339(),
        "live": {"sha256": live_sha, "size": live_size},
        "binaries": report_bins, "smoke": report_smoke, "check": report_check,
        "qemu": qemu.display().to_string(), "qemu_version_out": ver,
    });
    let out = std::env::var("NERVE_XT_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| xt.join("report.json"));
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    eprintln!("[xt] report {}", out.display());
}
