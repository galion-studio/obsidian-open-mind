// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Wave-4 slot 312 (release-artifacts-refresh): probe the exact release
//! artifact bytes of every live Rust service, not a fresh cargo build.
//!
//! Gated: needs `GALION_REAL=1` and `GALION_RELEASE_DIR=<dir>` where
//! `<dir>/<triple>/<bin>-<tag>` are the stripped musl artifacts and
//! `GALION_RELEASE_TAG=<tag>` (for example `38a0770`). Only artifacts for the
//! host triple are started. Each service is started alone on a port in the
//! slot range 21820-21829, probed with real HTTP GETs, then killed, so ports
//! are reused and never overlap. Optional extra artifacts (gaia-cortex,
//! galion-console-serve) are probed when `GALION_RELEASE_CORTEX` /
//! `GALION_RELEASE_CONSOLE_SERVE` name them. No sandbox code is run.
//!
//! Run: `GALION_REAL=1 GALION_RELEASE_DIR=... GALION_RELEASE_TAG=...
//!   cargo test -p galion-realtest --test real_release_artifacts -- --nocapture`

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const P0: u16 = 21820;
const P1: u16 = 21821;

fn enabled() -> Option<(PathBuf, String)> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: GALION_REAL!=1");
        return None;
    }
    let dir = std::env::var("GALION_RELEASE_DIR").ok()?;
    let tag = std::env::var("GALION_RELEASE_TAG").ok()?;
    let triple = format!("{}-unknown-linux-musl", std::env::consts::ARCH);
    Some((PathBuf::from(dir).join(triple), tag))
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let code = text.split(' ').nth(1)?.parse().ok()?;
    let body = text.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    Some((code, body.to_string()))
}

fn wait_get(port: u16, path: &str, child: &mut Child) -> (u16, String) {
    let t0 = Instant::now();
    loop {
        if let Some(r) = get(port, path) {
            return r;
        }
        if let Ok(Some(st)) = child.try_wait() {
            panic!("process exited early on :{port}: {st}");
        }
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "no answer on :{port}{path}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("r312-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// ELF checks on the artifact itself: static (no PT_INTERP, no PT_DYNAMIC
/// with DT_NEEDED is implied by no PT_INTERP for static-pie), right machine,
/// and stripped (no .symtab).
fn elf_static(path: &Path) {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"\x7fELF", "{}", path.display());
    let machine = u16::from_le_bytes([b[18], b[19]]);
    let want = if cfg!(target_arch = "x86_64") {
        62
    } else {
        183
    };
    assert_eq!(machine, want, "{}", path.display());
    let phoff = u64::from_le_bytes(b[32..40].try_into().unwrap()) as usize;
    let phentsize = u16::from_le_bytes([b[54], b[55]]) as usize;
    let phnum = u16::from_le_bytes([b[56], b[57]]) as usize;
    for i in 0..phnum {
        let o = phoff + i * phentsize;
        let ptype = u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        assert_ne!(ptype, 3, "PT_INTERP in {}", path.display());
    }
    let shoff = u64::from_le_bytes(b[40..48].try_into().unwrap()) as usize;
    let shentsize = u16::from_le_bytes([b[58], b[59]]) as usize;
    let shnum = u16::from_le_bytes([b[60], b[61]]) as usize;
    for i in 0..shnum {
        let o = shoff + i * shentsize;
        let stype = u32::from_le_bytes(b[o + 4..o + 8].try_into().unwrap());
        assert_ne!(stype, 2, "SHT_SYMTAB (not stripped) in {}", path.display());
    }
}

fn spawn(bin: &Path, args: &[&str], env: &[(&str, String)]) -> Kill {
    let mut c = Command::new(bin);
    c.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    Kill(
        c.spawn()
            .unwrap_or_else(|e| panic!("spawn {}: {e}", bin.display())),
    )
}

fn report(name: &str, port: u16, path: &str, r: &(u16, String)) {
    let body: String = r.1.chars().take(120).collect();
    println!("{name} :{port} GET {path} -> {} {}", r.0, body.trim_end());
}

#[test]
fn release_artifacts_health() {
    let Some((dir, tag)) = enabled() else { return };
    let bin = |n: &str| dir.join(format!("{n}-{tag}"));
    let mut n = 0;

    // version smoke + ELF checks for every artifact of this tag
    for name in [
        "openmind-nerve",
        "hermes-gateway",
        "openmind-memory",
        "openmind-queue",
        "galion-one",
        "galion-health",
        "galion-desk",
        "link-watch",
        "tuner",
        "galion-console-static",
        "galion-backup",
        "desk-hermes",
    ] {
        let p = bin(name);
        elf_static(&p);
        if matches!(
            name,
            "hermes-gateway" | "galion-console-static" | "galion-backup"
        ) {
            // hermes-gateway: --version enters the start path (D-228-4);
            // console-static / galion-backup have no --version (D-312-3);
            // all three are exercised for real below.
            continue;
        }
        let out = Command::new(&p)
            .arg("--version")
            .env_clear()
            .output()
            .unwrap();
        let s = String::from_utf8_lossy(&out.stdout);
        println!("{name} --version -> {} {}", out.status, s.trim());
        n += 1;
    }

    // openmind-nerve
    {
        let mut k = spawn(
            &bin("openmind-nerve"),
            &[
                "serve",
                "--listen",
                &format!("127.0.0.1:{P0}"),
                "--mode",
                "event",
            ],
            &[],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report("openmind-nerve", P0, "/health", &r);
        assert_eq!(r.0, 200);
        assert!(r.1.contains("\"service\":\"openmind-nerve\""));
        n += 1;
    }
    // tuner
    {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tuner/www");
        let mut k = spawn(
            &bin("tuner"),
            &[],
            &[
                ("TUNER_ADDR", format!("127.0.0.1:{P0}")),
                ("TUNER_ROOT", root.display().to_string()),
            ],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report("tuner", P0, "/health", &r);
        assert_eq!(r.0, 200);
        assert!(r.1.contains("\"ok\":true"));
        n += 1;
    }
    // galion-health checks a live tuner on P1
    {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tuner/www");
        let _t = spawn(
            &bin("tuner"),
            &[],
            &[
                ("TUNER_ADDR", format!("127.0.0.1:{P1}")),
                ("TUNER_ROOT", root.display().to_string()),
            ],
        );
        let mut k = spawn(
            &bin("galion-health"),
            &[],
            &[
                ("GALION_HEALTH_ADDR", format!("127.0.0.1:{P0}")),
                ("GALION_HEALTH_CHECKS", format!("{P1}=/health")),
            ],
        );
        let r = wait_get(P0, "/", &mut k.0);
        report("galion-health", P0, "/", &r);
        assert_eq!(r.0, 200);
        assert!(r.1.contains("galion-health"));
        n += 1;
    }
    // openmind-queue
    {
        let d = tmp("queue");
        let mut k = spawn(
            &bin("openmind-queue"),
            &[
                "serve",
                "--bind",
                &format!("127.0.0.1:{P0}"),
                "--data",
                d.to_str().unwrap(),
            ],
            &[],
        );
        let r = wait_get(P0, "/healthz", &mut k.0);
        report("openmind-queue", P0, "/healthz", &r);
        assert_eq!(r.0, 200);
        assert!(r.1.contains("\"service\":\"openmind-queue\""));
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // openmind-memory (SQLite)
    {
        let d = tmp("memory");
        let mut k = spawn(
            &bin("openmind-memory"),
            &[],
            &[
                ("OPENMIND_MEMORY_HOST", "127.0.0.1".into()),
                ("OPENMIND_MEMORY_PORT", P0.to_string()),
                ("OPENMIND_MEMORY_DATA_DIR", d.display().to_string()),
            ],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report("openmind-memory", P0, "/health", &r);
        assert_eq!(r.0, 200);
        drop(k);
        // galion-backup on the real SQLite file the memory service just wrote
        let db = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "db" || x == "sqlite"))
            .expect("memory sqlite file");
        let snap = d.join("snap.db");
        for args in [
            vec![
                "snapshot".to_string(),
                db.display().to_string(),
                snap.display().to_string(),
            ],
            vec!["verify".to_string(), snap.display().to_string()],
        ] {
            let out = Command::new(bin("galion-backup"))
                .args(&args)
                .env_clear()
                .output()
                .unwrap();
            let s = String::from_utf8_lossy(&out.stdout);
            println!("galion-backup {} -> {} {}", args[0], out.status, s.trim());
            assert!(out.status.success());
        }
        n += 1;
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // hermes-gateway (SQLite, --mock, throwaway key; no network model)
    {
        let d = tmp("hermes");
        let mut k = spawn(
            &bin("hermes-gateway"),
            &["--mock"],
            &[
                ("HOME", d.display().to_string()),
                ("HERMES_HOME", d.display().to_string()),
                ("HERMES_RS_HOST", "127.0.0.1".into()),
                ("HERMES_RS_PORT", P0.to_string()),
                ("HERMES_RS_MCP", "0".into()),
                ("API_SERVER_KEY", "r312-throwaway-test-key-0000".into()),
            ],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report("hermes-gateway", P0, "/health", &r);
        assert_eq!(r.0, 200);
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // galion-one
    {
        let d = tmp("one");
        std::fs::create_dir_all(d.join("home")).unwrap();
        std::fs::write(d.join("hermes.env"), "").unwrap();
        let dead = "127.0.0.1:21829".to_string();
        let mut k = spawn(
            &bin("galion-one"),
            &[],
            &[
                ("HOME", d.join("home").display().to_string()),
                ("GALION_HOME", d.join("home").display().to_string()),
                ("ONE_NO_WARM", "1".into()),
                ("ONE_BIND", format!("127.0.0.1:{P0}")),
                ("ONE_MOUTH_BIND", format!("127.0.0.1:{P1}")),
                ("ONE_HERMES_DIR", d.join("hermes").display().to_string()),
                ("ONE_HERMES_ENV", d.join("hermes.env").display().to_string()),
                (
                    "ONE_DEVICE_STORE",
                    d.join("missing.json").display().to_string(),
                ),
                ("GALION_MIND", dead.clone()),
                ("GALION_EAR", dead.clone()),
                ("GALION_EYE", dead.clone()),
                ("GALION_MEMORY", dead),
            ],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report("galion-one", P0, "/health", &r);
        assert_eq!(r, (200, "ok\n".to_string()));
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // galion-desk
    {
        let d = tmp("desk");
        let mut k = spawn(
            &bin("galion-desk"),
            &[],
            &[
                (
                    "OPENMIND_CONFIG",
                    d.join("missing.toml").display().to_string(),
                ),
                ("DESK_BIND", format!("127.0.0.1:{P0}")),
                ("DESK_STATE_DIR", d.display().to_string()),
                ("DESK_OLLAMA", "127.0.0.1:21829".into()),
            ],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report("galion-desk", P0, "/health", &r);
        assert_eq!(r.0, 200);
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // galion-console-static
    {
        let d = tmp("console");
        std::fs::write(d.join("_shell.html"), "<!doctype html><title>r312</title>").unwrap();
        let mut k = spawn(
            &bin("galion-console-static"),
            &[
                "--bind",
                &format!("127.0.0.1:{P0}"),
                "--quiet",
                "--root",
                d.to_str().unwrap(),
            ],
            &[],
        );
        let r = wait_get(P0, "/", &mut k.0);
        report("galion-console-static", P0, "/", &r);
        assert_eq!(r.0, 200);
        assert!(r.1.contains("r312"));
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // gaia-cortex (built from cortex-staging-fold; not on main-staging)
    if let Ok(name) = std::env::var("GALION_RELEASE_CORTEX") {
        let d = tmp("cortex");
        let mut k = spawn(
            &dir.join(&name),
            &["serve"],
            &[
                ("CORTEX_LISTEN", format!("127.0.0.1:{P0}")),
                ("CORTEX_DATA_DIR", d.display().to_string()),
                ("CORTEX_NERVE_URL", "http://127.0.0.1:21829".into()),
                ("CORTEX_ACTIONS", "dry-run".into()),
                ("CORTEX_PULL_EVERY_S", "0".into()),
            ],
        );
        let r = wait_get(P0, "/health", &mut k.0);
        report(&name, P0, "/health", &r);
        assert_eq!(r.0, 200);
        assert!(r.1.contains("\"service\":\"gaia-cortex\""));
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    // galion-console-serve (live console build, branch console-rust-serve-impl)
    if let Ok(name) = std::env::var("GALION_RELEASE_CONSOLE_SERVE") {
        let d = tmp("serve");
        std::fs::write(d.join("index.html"), "<!doctype html><title>r312s</title>").unwrap();
        let mut k = spawn(
            &dir.join(&name),
            &[
                "--bind",
                &format!("127.0.0.1:{P0}"),
                "--dist",
                d.to_str().unwrap(),
            ],
            &[],
        );
        let r = wait_get(P0, "/", &mut k.0);
        report(&name, P0, "/", &r);
        // --dist mode answers / with a 302 to the board entry page (as live :8080).
        assert!(r.0 == 200 || r.0 == 302, "{r:?}");
        let h = wait_get(P0, "/api/v1/health", &mut k.0);
        report(&name, P0, "/api/v1/health", &h);
        assert_eq!(h.0, 200);
        drop(k);
        let _ = std::fs::remove_dir_all(d);
        n += 1;
    }
    println!(
        "release_artifacts_health: {n} checks passed ({tag}, {})",
        dir.display()
    );
}
