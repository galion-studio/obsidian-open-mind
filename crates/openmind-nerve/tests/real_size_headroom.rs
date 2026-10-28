// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 348 nerve-size-headroom (D-337-3) real test on release bytes
//! (gated: `GALION_REAL=1`, box only, otherwise skipped).
//!
//! Takes already built musl release binaries (this test does not build):
//! - `NERVE_HEADROOM_X86`  x86_64 musl release openmind-nerve (required),
//! - `NERVE_HEADROOM_A64`  aarch64 musl release openmind-nerve (optional,
//!   run under qemu user mode; `NERVE_HEADROOM_QEMU` or qemu-aarch64[-static]
//!   on PATH),
//! - `NERVE_HEADROOM_BASE` x86_64 build of the same tree with
//!   `NERVE_NO_SIZE_LINK_ARGS=1` (optional): the trimmed binary must answer
//!   exactly like it (same keys, statuses, verdict).
//!
//! Checks: static ELF of the right machine, stripped, x86_64 at least
//! `MIN_HEADROOM` under the 2 MiB budget of scripts/size-budget.sh, x86_64
//! has DT_RELR and only a handful of RELA entries left; then each binary is
//! started on the slot 348 ports (`NERVE_HEADROOM_PORT_BASE`, default 22180:
//! +0 x86_64, +1 aarch64, +2 base, +8 live stub, +9 dead port) with systemd,
//! journald, reflex and rollback off, and must serve `/health` (cycle >= 1),
//! `POST /signal` 202 / 400, `/state` (stub healthy, dead port failing) and
//! `/api/v1/signals` with the posted signal. No VPS, no live system.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 2 MiB: `LIMIT` of scripts/size-budget.sh.
const BUDGET: u64 = 2 * 1024 * 1024;
/// D-337-3 asked for real headroom; 128 KiB is ~6 % of the budget (the base
/// tree had 76,928 B).
const MIN_HEADROOM: u64 = 128 * 1024;

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

struct ElfInfo {
    machine: u16,
    has_interp: bool,
    has_symtab: bool,
    has_relr: bool,
    rela_entries: u64,
}

fn elf_info(b: &[u8]) -> ElfInfo {
    assert!(b.len() > 64 && &b[..4] == b"\x7fELF", "not an ELF");
    assert_eq!(b[4], 2, "not ELF64");
    assert_eq!(b[5], 1, "not little-endian");
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let phoff = u64_at(32) as usize;
    let phentsize = u16_at(54) as usize;
    let phnum = u16_at(56) as usize;
    let shoff = u64_at(40) as usize;
    let shentsize = u16_at(58) as usize;
    let shnum = u16_at(60) as usize;
    let mut info = ElfInfo {
        machine: u16_at(18),
        has_interp: false,
        has_symtab: false,
        has_relr: false,
        rela_entries: 0,
    };
    for i in 0..phnum {
        let p = phoff + i * phentsize;
        match u32_at(p) {
            3 => info.has_interp = true, // PT_INTERP
            2 => {
                // PT_DYNAMIC: walk (d_tag, d_val) pairs.
                let (off, size) = (u64_at(p + 8) as usize, u64_at(p + 32) as usize);
                let mut o = off;
                while o + 16 <= off + size {
                    let (tag, val) = (u64_at(o), u64_at(o + 8));
                    match tag {
                        0 => break,
                        0x24 => info.has_relr = true,      // DT_RELR
                        8 => info.rela_entries = val / 24, // DT_RELASZ
                        _ => {}
                    }
                    o += 16;
                }
            }
            _ => {}
        }
    }
    for i in 0..shnum {
        if u32_at(shoff + i * shentsize + 4) == 2 {
            info.has_symtab = true; // SHT_SYMTAB
        }
    }
    info
}

fn find_qemu() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NERVE_HEADROOM_QEMU") {
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

struct Smoke {
    health: Value,
    signal_code: u16,
    bad_signal_code: u16,
    state: Value,
    signal_seen: bool,
}

fn comp_status(st: &Value, name: &str) -> String {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == name))
        .map(|c| c["status"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    k.sort();
    k
}

#[allow(clippy::too_many_arguments)]
fn smoke(
    label: &str,
    prog: &Path,
    args: &[&Path],
    port: u16,
    stub: u16,
    dead: u16,
    scope: &str,
    dir: &Path,
) -> Smoke {
    let data = dir.join(format!("data-{label}"));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let comps = dir.join(format!("components-{label}.json"));
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"hr-live","port":{stub}}},{{"name":"hr-dead","port":{dead},"gaia":"APOLLO"}}]"#
        ),
    )
    .unwrap();
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "{label}: port {port} busy before start"
    );
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .arg("serve")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
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
        .env("NERVE_LISTEN_SCOPE", scope)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let t = Instant::now();
    let nerve = Kill(cmd.spawn().expect("spawn nerve"));
    let health = loop {
        if let Some((200, b)) = http(port, "GET", "/health", "") {
            let v: Value = serde_json::from_str(&b).unwrap_or_default();
            if v["cycle"].as_u64().unwrap_or(0) >= 1 {
                break v;
            }
        }
        assert!(
            t.elapsed() < Duration::from_secs(90),
            "{label}: no /health with cycle>=1 within 90 s"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let src = format!("hr-{label}");
    let sig = format!(r#"{{"source":"{src}","kind":"error","name":"headroom","msg":"slot 348"}}"#);
    let signal_code = http(port, "POST", "/signal", &sig).map_or(0, |r| r.0);
    let bad_signal_code = http(port, "POST", "/signal", r#"{"source":"x"}"#).map_or(0, |r| r.0);
    let state: Value = http(port, "GET", "/state", "")
        .filter(|r| r.0 == 200)
        .and_then(|r| serde_json::from_str(&r.1).ok())
        .unwrap_or_default();
    let signal_seen = http(port, "GET", "/api/v1/signals", "")
        .filter(|r| r.0 == 200)
        .is_some_and(|r| r.1.contains(&src));
    drop(nerve);
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_file(&comps);
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "{label}: port {port} still open after kill"
    );
    eprintln!(
        "[headroom] smoke {label:<8} :{port} first cycle {:.2} s system {}",
        t.elapsed().as_secs_f64(),
        state["system"]["status"]
    );
    Smoke {
        health,
        signal_code,
        bad_signal_code,
        state,
        signal_seen,
    }
}

#[test]
fn real_size_headroom() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("real_size_headroom: skipped (set GALION_REAL=1)");
        return;
    }
    let x86 = PathBuf::from(std::env::var("NERVE_HEADROOM_X86").expect(
        "set NERVE_HEADROOM_X86 to the x86_64 musl release openmind-nerve (this test does not build)",
    ));
    let a64 = std::env::var("NERVE_HEADROOM_A64").ok().map(PathBuf::from);
    let base = std::env::var("NERVE_HEADROOM_BASE").ok().map(PathBuf::from);
    let pb: u16 = std::env::var("NERVE_HEADROOM_PORT_BASE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(22180);
    let (stub_port, dead_port) = (pb + 8, pb + 9);
    let scope = format!("{pb}-{}", pb + 9);

    // 1. bytes ------------------------------------------------------------
    let check = |label: &str, p: &Path, machine: u16, trimmed: bool| -> u64 {
        let b = std::fs::read(p).unwrap_or_else(|e| panic!("{label}: read {}: {e}", p.display()));
        let e = elf_info(&b);
        let size = b.len() as u64;
        eprintln!(
            "[headroom] {label:<8} {size:>9} B headroom {:>7} B relr {} rela {} {}",
            BUDGET as i64 - size as i64,
            e.has_relr,
            e.rela_entries,
            p.display()
        );
        assert_eq!(e.machine, machine, "{label}: wrong e_machine");
        assert!(!e.has_interp, "{label}: PT_INTERP (not static)");
        assert!(!e.has_symtab, "{label}: not stripped (.symtab)");
        assert!(size <= BUDGET, "{label}: {size} B over the 2 MiB budget");
        if trimmed && machine == 0x3E {
            assert!(
                size + MIN_HEADROOM <= BUDGET,
                "{label}: {size} B leaves less than {MIN_HEADROOM} B headroom"
            );
            assert!(
                e.has_relr,
                "{label}: no DT_RELR (build.rs flags not applied?)"
            );
            assert!(
                e.rela_entries < 64,
                "{label}: {} RELA entries left (RELR should pack them)",
                e.rela_entries
            );
        }
        size
    };
    let x_size = check("x86_64", &x86, 0x3E, true);
    if let Some(p) = &a64 {
        check("aarch64", p, 0xB7, true);
    }
    if let Some(p) = &base {
        let b_size = check("base", p, 0x3E, false);
        assert!(
            x_size < b_size,
            "trimmed {x_size} B not smaller than base {b_size} B"
        );
        eprintln!("[headroom] saved {} B vs base", b_size - x_size);
    }

    // 2. smoke on the release bytes ---------------------------------------
    let dir = std::env::temp_dir().join(format!("nerve-headroom-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let svc = TcpListener::bind(("127.0.0.1", stub_port)).expect("stub port free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    assert!(
        TcpStream::connect(("127.0.0.1", dead_port)).is_err(),
        "dead port {dead_port} must be free"
    );
    let mut runs: Vec<(String, PathBuf, Vec<PathBuf>, u16)> =
        vec![("x86_64".into(), x86.clone(), vec![], pb)];
    if let Some(p) = &a64 {
        let q = find_qemu().expect("qemu-aarch64 not found (set NERVE_HEADROOM_QEMU)");
        let v = Command::new(&q)
            .arg(p)
            .arg("--version")
            .output()
            .expect("qemu --version");
        let ver = String::from_utf8_lossy(&v.stdout).trim().to_string();
        assert!(
            ver.starts_with("openmind-nerve "),
            "qemu --version: {ver:?}"
        );
        runs.push(("aarch64".into(), q, vec![p.clone()], pb + 1));
    }
    if let Some(p) = &base {
        runs.push(("base".into(), p.clone(), vec![], pb + 2));
    }
    let mut smokes = Vec::new();
    for (label, prog, args, port) in &runs {
        let a: Vec<&Path> = args.iter().map(PathBuf::as_path).collect();
        let s = smoke(label, prog, &a, *port, stub_port, dead_port, &scope, &dir);
        assert_eq!(
            s.health["service"], "openmind-nerve",
            "{label}: {}",
            s.health
        );
        assert_eq!(s.health["status"], "ok", "{label}");
        assert_eq!(s.signal_code, 202, "{label}: POST /signal");
        assert_eq!(s.bad_signal_code, 400, "{label}: POST /signal without kind");
        assert_eq!(comp_status(&s.state, "hr-live"), "healthy", "{label}");
        assert_eq!(comp_status(&s.state, "hr-dead"), "failing", "{label}");
        assert!(
            s.signal_seen,
            "{label}: posted signal not in /api/v1/signals"
        );
        smokes.push((label.clone(), s));
    }
    let x = &smokes[0].1;
    for (label, s) in &smokes[1..] {
        assert_eq!(
            keys(&x.health),
            keys(&s.health),
            "health keys x86_64 vs {label}"
        );
        assert_eq!(
            keys(&x.state),
            keys(&s.state),
            "state keys x86_64 vs {label}"
        );
        assert_eq!(
            x.state["system"]["status"], s.state["system"]["status"],
            "system verdict x86_64 vs {label}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
    for p in pb..=pb + 9 {
        assert!(
            TcpStream::connect(("127.0.0.1", p)).is_err() || p == stub_port,
            "port {p} still busy after the test"
        );
    }
    eprintln!(
        "[headroom] PASS {} binaries, ports {pb}-{}",
        smokes.len(),
        pb + 9
    );
}
