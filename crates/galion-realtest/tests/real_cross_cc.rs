// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Cross C compiler regression test (slot 338 cross-cc-restore, D-318-3 /
//! D-325-3). GALION_REAL=1 only, otherwise skipped.
//!
//! hermes-gateway links bundled SQLite (libsqlite3-sys, C). For the
//! aarch64 and armv7 musl targets cc-rs needs a C cross compiler: either
//! `CC_<triple>` or the default tool name on PATH (`aarch64-linux-musl-gcc`,
//! `arm-linux-musleabihf-gcc`). On the swarm box these are zig 0.15.2 cc
//! wrappers in /workspace/swarm/bin (docs/cross-cc-restore.md). The test
//! - builds `-p hermes-gateway` (default features: sqlite + coding) for both
//!   targets in its own target dir (`<target>/cross-cc-real`), no CC env
//!   needed when the wrappers are on PATH;
//! - checks the ELF (machine, static: no PT_INTERP / PT_DYNAMIC) and that
//!   the SQLite amalgamation is linked in;
//! - when `qemu-aarch64[-static]` / `qemu-arm[-static]` is on PATH, runs the
//!   real binary with `--mock` (offline provider, no network, no spend) on
//!   127.0.0.1:22080 (aarch64) / 22081 (armv7), writes a chat session, kills
//!   it, restarts it on the same state.db and reads the session back. That
//!   is a real SQLite write + read executed as target code.
//!
//! Ports 22080-22089 (wave-4 slot 338). Loopback only, no live system.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::http;

const KEY: &str = "slot338-cross-cc-test-key";

struct Target {
    triple: &'static str,
    cc_tool: &'static str,
    qemu: [&'static str; 2],
    em: u16,
    addr: &'static str,
}

const TARGETS: [Target; 2] = [
    Target {
        triple: "aarch64-unknown-linux-musl",
        cc_tool: "aarch64-linux-musl-gcc",
        qemu: ["qemu-aarch64-static", "qemu-aarch64"],
        em: 183, // EM_AARCH64
        addr: "127.0.0.1:22080",
    },
    Target {
        triple: "armv7-unknown-linux-musleabihf",
        cc_tool: "arm-linux-musleabihf-gcc",
        qemu: ["qemu-arm-static", "qemu-arm"],
        em: 40, // EM_ARM
        addr: "127.0.0.1:22081",
    },
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(bin))
            .find(|f| f.is_file())
    })
}

/// (e_machine, PT_INTERP present, PT_DYNAMIC present) for ELF32/ELF64 LE.
fn elf_info(b: &[u8]) -> (u16, bool, bool) {
    assert_eq!(&b[..4], b"\x7fELF", "not ELF");
    assert_eq!(b[5], 1, "little endian");
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let machine = u16_at(18);
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
    (machine, interp, dynamic)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

struct Kill(Option<Child>);
impl Drop for Kill {
    fn drop(&mut self) {
        if let Some(c) = &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn wait_up(addr: &str, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if http::get(addr, "/health").is_ok_and(|r| r.status == 200) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn spawn(qemu: &Path, bin: &Path, t: &Target, work: &Path) -> Kill {
    let port = t.addr.rsplit(':').next().unwrap();
    let child = Command::new(qemu)
        .arg(bin)
        .arg("--mock")
        .env_clear()
        .env("HERMES_HOME", work.join("home"))
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port)
        .env("HERMES_RS_STATE_DB", work.join("state.db"))
        .env("API_SERVER_KEY", KEY)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn under qemu");
    let k = Kill(Some(child));
    assert!(
        wait_up(t.addr, 60),
        "{}: gateway not up on {}",
        t.triple,
        t.addr
    );
    k
}

fn sessions(addr: &str) -> String {
    let auth = format!("Bearer {KEY}");
    let r = http::request(
        addr,
        "GET",
        "/api/sessions",
        &[("Authorization", &auth)],
        b"",
        Duration::from_secs(20),
    )
    .expect("GET /api/sessions");
    assert_eq!(r.status, 200, "/api/sessions: {}", r.text());
    r.text()
}

fn build(t: &Target, td: &Path) -> PathBuf {
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
    ])
    .args(["-p", "hermes-gateway", "--target", t.triple])
    .current_dir(root())
    .env("CARGO_TARGET_DIR", td)
    .env_remove("RUSTFLAGS");
    // Nested cargo: drop the outer test's per-crate env.
    for (k, _) in std::env::vars() {
        if (k.starts_with("CARGO_") && k != "CARGO_HOME") || k.starts_with("RUSTC") {
            cmd.env_remove(k);
        }
    }
    cmd.env("CARGO_TARGET_DIR", td);
    let t0 = Instant::now();
    let st = cmd.status().expect("run cargo");
    assert!(st.success(), "{}: hermes-gateway build failed", t.triple);
    eprintln!(
        "build hermes-gateway {}: {:.1}s",
        t.triple,
        t0.elapsed().as_secs_f64()
    );
    td.join(t.triple).join("release").join("hermes-gateway")
}

#[test]
fn real_cross_cc_sqlite_gateway() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (cross C build + qemu run)");
        return;
    }
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let td = base.join("cross-cc-real");
    for t in &TARGETS {
        let env_cc = format!("CC_{}", t.triple.replace('-', "_"));
        assert!(
            std::env::var_os(&env_cc).is_some() || which(t.cc_tool).is_some(),
            "{}: no C cross compiler: set {env_cc} or put {} on PATH \
             (docs/cross-cc-restore.md)",
            t.triple,
            t.cc_tool
        );
        let bin = build(t, &td);
        let bytes = std::fs::read(&bin).expect("read binary");
        let (m, interp, dynamic) = elf_info(&bytes);
        eprintln!(
            "{}: {} B e_machine={m} PT_INTERP={interp} PT_DYNAMIC={dynamic}",
            t.triple,
            bytes.len()
        );
        assert_eq!(m, t.em, "{}: wrong e_machine", t.triple);
        assert!(!interp && !dynamic, "{}: not static", t.triple);
        assert!(
            contains(&bytes, b"SQLite format 3"),
            "{}: SQLite amalgamation not linked",
            t.triple
        );

        let Some(qemu) = t.qemu.iter().find_map(|q| which(q)) else {
            eprintln!("{}: qemu missing: ELF checks only", t.triple);
            continue;
        };
        assert!(
            !http::port_open(t.addr),
            "{} already in use (slot 338 range)",
            t.addr
        );
        let work = td.join(format!("run-{}", t.triple));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(work.join("home")).unwrap();
        let sid = format!("s338-{}", t.triple);
        let text = format!("{} sqlite write", t.triple);
        {
            let _g = spawn(&qemu, &bin, t, &work);
            let auth = format!("Bearer {KEY}");
            let body = serde_json::json!({"model": "hermes-agent",
                "messages": [{"role": "user", "content": text}]})
            .to_string();
            let r = http::request(
                t.addr,
                "POST",
                "/v1/chat/completions",
                &[
                    ("Authorization", &auth),
                    ("Content-Type", "application/json"),
                    ("X-Hermes-Session-Id", &sid),
                ],
                body.as_bytes(),
                Duration::from_secs(30),
            )
            .expect("chat");
            assert_eq!(r.status, 200, "chat: {}", r.text());
            assert!(r.text().contains(&format!("echo: {text}")), "{}", r.text());
            let unauth = http::get(t.addr, "/v1/models").expect("models");
            assert_eq!(unauth.status, 401, "no-key /v1/models must be 401");
            assert!(sessions(t.addr).contains(&sid));
        }
        let db = std::fs::read(work.join("state.db")).expect("state.db written");
        assert!(db.starts_with(b"SQLite format 3\0"), "state.db not SQLite");
        {
            let _g = spawn(&qemu, &bin, t, &work);
            let s = sessions(t.addr);
            assert!(
                s.contains(&sid) && s.contains("\"message_count\":2"),
                "{}: session not read back after restart: {s}",
                t.triple
            );
            eprintln!(
                "{}: qemu run ok on {} (chat 200, no-key 401, session read back after restart, state.db {} B)",
                t.triple,
                t.addr,
                db.len()
            );
        }
        let _ = std::fs::remove_dir_all(&work);
    }
}
