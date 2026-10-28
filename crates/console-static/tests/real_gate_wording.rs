// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 410, console-gate-wording; F-396-3 / D-396-3):
//! the RELEASE `galion-console-serve` started with real argv on the box.
//!
//! A1. The pre-17:35 live argv (`deploy/console-rust-serve/live/console.service`:
//!    `--dist --ui rust --local-api`, no `--admin-store`) on THIS build: the
//!    API answers `503 {"error":"admin store not configured"}`; every remote
//!    room 503 "Admin sign-in is not set up on this console"; loopback 200;
//!    pairing POST 503 "not set up", not 401 "code refused".
//! A2. The real 404 case: the byte-identical live binary 2d846fb8 (live
//!    2026-10-27 19:28 CET - 2026-10-28 17:35 CET, `--local-api`) answers
//!    `/api/v1/admin` 404 and itself shows "Console API is not answering"
//!    (negative control = the F-396-3 bug); this build in front of it with
//!    `--upstream` shows "not set up" on every room.
//! B. `--upstream` to a dead port: 503 "Console API is not answering".
//! C. `--upstream` to a real TCP upstream answering 502 -> "not answering";
//!    answering 405 -> "not set up".
//! D. The candidate argv (live since 17:35 CET, `--admin-store` drop-in):
//!    `/api/v1/admin` 200, `/console` 200 pairing page, neither gate text.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_gate_wording -- --nocapture`
//! Ports: 22800-22809 (box only). No live change, NO deploy.

mod units;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LIVE: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const DROP: &str = include_str!(
    "../../../deploy/console-rust-serve/candidate/console.service.d/60-admin-store.conf"
);
const REMOTE: &str = "console.galion.app";
const MISSING: &str = "Admin sign-in is not set up on this console";
const DOWN: &str = "Console API is not answering";
const ROOMS: &[&str] = &["/console", "/node", "/hermes", "/status", "/agents"];

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn send(at: &str, raw: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(at) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = s.write_all(raw.as_bytes());
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let t = String::from_utf8_lossy(&out).to_string();
    let code = t
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, t)
}

fn get(at: &str, path: &str, host: &str) -> (u16, String) {
    send(
        at,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"),
    )
}

fn pair_post(at: &str) -> (u16, String) {
    let body = "code=SLOT410-WRONG";
    send(
        at,
        &format!(
            "POST /_ui/pair?back=/console HTTP/1.1\r\nHost: {REMOTE}\r\nOrigin: https://{REMOTE}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn wait_up(at: &str) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if TcpStream::connect(at).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn spawn(argv: &[String], at: &str) -> Kill {
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn galion-console-serve");
    let k = Kill(child);
    assert!(wait_up(at), "server did not bind {at}");
    k
}

fn remap(argv: &[String], exe: &Path, tmp: &Path, bind: &str) -> Vec<String> {
    let t = tmp.to_str().unwrap();
    argv.iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 {
                return exe.to_str().unwrap().to_string();
            }
            a.replace("127.0.0.1:8080", bind)
                .replace("127.0.0.1:8091", "127.0.0.1:22808")
                .replace("127.0.0.1:8646", "127.0.0.1:22809")
                .replace("/opt/galion/console-dist", &format!("{t}/opt/console-dist"))
                .replace("/root/galion-secrets", &format!("{t}/root/galion-secrets"))
                .replace(
                    "/workspace/galion-light/secrets/persist",
                    &format!("{t}/persist"),
                )
                .replace("/workspace/arena", &format!("{t}/arena"))
        })
        .collect()
}

/// Real TCP upstream on `at` answering every request with `status`.
fn upstream(at: &str, status: u16) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(at).unwrap();
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(20) {
            match l.accept() {
                Ok((mut s, _)) => {
                    s.set_nonblocking(false).ok();
                    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
                    let mut b = [0u8; 8192];
                    let _ = s.read(&mut b);
                    let body = "{\"error\":\"slot410\"}";
                    let _ = s.write_all(
                        format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    );
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    })
}

fn rooms_say(at: &str, want: &str, tag: &str) {
    let other = if want == MISSING { DOWN } else { MISSING };
    for r in ROOMS {
        let (c, b) = get(at, r, REMOTE);
        assert_eq!(c, 503, "{tag} {r}: fail-closed 503");
        assert!(b.contains(want), "{tag} {r}: want {want:?}");
        assert!(!b.contains(other), "{tag} {r}: not {other:?}");
        assert!(!b.contains("/_ui/pair"), "{tag} {r}: no pair form");
    }
    eprintln!("{tag}: {} rooms 503 \"{want}\"", ROOMS.len());
}

#[test]
fn real_gate_wording() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let fixture = std::env::var("CONSOLE_DIST_STATIC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static")
        });
    assert!(
        fixture.join("favicon.svg").is_file(),
        "{}",
        fixture.display()
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_galion-console-serve"));
    let tmp = std::env::temp_dir().join(format!("real-410-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dist = tmp.join("opt/console-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::os::unix::fs::symlink(&fixture, dist.join("static")).unwrap();
    let t0 = Instant::now();

    // ---- A1: pre-17:35 live argv (local-api, no admin store), this build ----
    // 5aa31927+ answers 503 {"error":"admin store not configured"} there.
    let a = "127.0.0.1:22800";
    let argv = remap(&units::exec_argv(&[LIVE]), &exe, &tmp, a);
    assert!(argv.iter().any(|x| x == "--local-api"));
    assert!(!argv.iter().any(|x| x == "--admin-store"));
    eprintln!("A1 argv: {}", argv[1..].join(" "));
    {
        let _k = spawn(&argv, a);
        let (c, b) = get(a, "/api/v1/admin", REMOTE);
        assert_eq!(c, 503, "A1: no store");
        assert!(b.contains("admin store not configured"), "A1: {b}");
        let (c, _) = get(a, "/api/v1/health", "127.0.0.1");
        assert_eq!(c, 200, "A1: health");
        rooms_say(a, MISSING, "A1");
        let (c, b) = get(a, "/console", "127.0.0.1");
        assert_eq!(c, 200, "A1: loopback renders");
        assert!(!b.contains(MISSING) && !b.contains(DOWN));
        let (c, b) = pair_post(a);
        assert_eq!(c, 503, "A1: pair POST");
        assert!(b.contains(MISSING) && !b.contains("That code was refused"));
        eprintln!("A1: /api/v1/admin 503 not-configured, health 200, loopback /console 200, POST /_ui/pair 503 not-set-up");
    }

    // ---- A2: the byte-identical live 2d846fb8 binary (live 2026-10-27
    // 19:28 CET - 2026-10-28 17:35 CET) as the upstream: real 404 case ----
    let old = std::env::var("CONSOLE_OLD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/swarm/artifacts/t291u/galion-console-serve-2d846fb8")
        });
    if old.is_file() {
        let o_at = "127.0.0.1:22804";
        let oargv = remap(&units::exec_argv(&[LIVE]), &old, &tmp, o_at);
        let _o = spawn(&oargv, o_at);
        let (c, _) = get(o_at, "/api/v1/admin", REMOTE);
        assert_eq!(c, 404, "A2: old live binary has no admin route");
        // Negative control: the old binary itself shows the misleading text.
        let (c, b) = get(o_at, "/console", REMOTE);
        assert_eq!(c, 503);
        assert!(b.contains(DOWN) && !b.contains(MISSING), "A2 old text");
        let n_at = "127.0.0.1:22800";
        let nargv: Vec<String> = [
            exe.to_str().unwrap(),
            "--bind",
            n_at,
            "--ui",
            "rust",
            "--upstream",
            o_at,
            "--quiet",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let _k = spawn(&nargv, n_at);
        rooms_say(n_at, MISSING, "A2 (upstream 2d846fb8, 404)");
        let (c, b) = pair_post(n_at);
        assert_eq!(c, 503, "A2: pair POST");
        assert!(b.contains(MISSING) && !b.contains("That code was refused"));
        eprintln!("A2: old 2d846fb8 /api/v1/admin 404 + own /console 503 \"{DOWN}\" (old text); new gate in front: 503 \"{MISSING}\", POST /_ui/pair 503");
    } else {
        panic!("A2: old live binary missing at {}", old.display());
    }

    // ---- B: upstream dead ----
    let b_at = "127.0.0.1:22801";
    let argv: Vec<String> = [
        exe.to_str().unwrap(),
        "--bind",
        b_at,
        "--ui",
        "rust",
        "--upstream",
        "127.0.0.1:22807",
        "--quiet",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    {
        let _k = spawn(&argv, b_at);
        rooms_say(b_at, DOWN, "B dead upstream");
        let (c, b) = pair_post(b_at);
        assert_eq!(c, 503);
        assert!(b.contains(DOWN));
    }

    // ---- C: real upstream answering 502, then 405 ----
    for (port, status, want) in [(22805u16, 502u16, DOWN), (22806, 405, MISSING)] {
        let up = format!("127.0.0.1:{port}");
        let _u = upstream(&up, status);
        let c_at = "127.0.0.1:22802";
        let mut argv = argv.clone();
        argv[2] = c_at.into();
        argv[6] = up.clone();
        let _k = spawn(&argv, c_at);
        rooms_say(c_at, want, &format!("C upstream {status}"));
    }

    // ---- D: candidate argv (live since 17:35 CET) with an admin store ----
    let sdir = tmp.join("root/galion-secrets");
    let mdir = tmp.join("persist");
    for d in [&sdir, &mdir] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::create_dir_all(tmp.join("arena")).unwrap();
    let d_at = "127.0.0.1:22803";
    let argv = remap(&units::exec_argv(&[LIVE, DROP]), &exe, &tmp, d_at);
    assert!(argv.iter().any(|x| x == "--admin-store"));
    {
        let _k = spawn(&argv, d_at);
        let (c, _) = get(d_at, "/api/v1/admin", REMOTE);
        assert_eq!(c, 200, "D: admin route present");
        let (c, b) = get(d_at, "/console", REMOTE);
        assert_eq!(c, 200, "D: pairing page");
        assert!(b.contains("/_ui/pair"));
        assert!(!b.contains(MISSING) && !b.contains(DOWN));
        eprintln!("D: candidate argv /api/v1/admin 200, /console 200 pairing page");
    }

    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "real_gate_wording A-D pass in {} ms",
        t0.elapsed().as_millis()
    );
}
