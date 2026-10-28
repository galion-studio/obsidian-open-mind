// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Box parity probe for console-dist Rust serve (wave-4 slot 302).
//!
//! Starts `galion-console-serve` (`--dist` + `--local-api` + `--ui rust`) on
//! 127.0.0.1:21610 against a local copy of live `console-dist/static`, checks
//! status-code parity with the Node srvx shapes recorded from the live VPS
//! (and optionally a live SSH forward on 21611 when `CONSOLE_RUST_SERVE_LIVE=1`).
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_console_rust_serve -- --nocapture`
//! Ports: 21610 (binary), 21611 (optional live forward). No console.service change.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:21610";
const LIVE_FWD: &str = "127.0.0.1:21611";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct KillSsh(Option<Child>);
impl Drop for KillSsh {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn fetch(at: &str, path: &str, host: &str) -> (u16, String, Vec<u8>) {
    let Ok(mut s) = TcpStream::connect(at) else {
        return (0, String::new(), Vec::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let Some(p) = out.windows(4).position(|w| w == b"\r\n\r\n") else {
        return (0, String::new(), Vec::new());
    };
    let head = String::from_utf8_lossy(&out[..p]).to_string();
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, head, out[p + 4..].to_vec())
}

fn wait_up(at: &str, deadline: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if TcpStream::connect(at).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn dist_root() -> PathBuf {
    if let Ok(p) = std::env::var("CONSOLE_DIST_STATIC") {
        return PathBuf::from(p);
    }
    PathBuf::from("/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static")
}

fn bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_galion-console-serve") {
        return PathBuf::from(p);
    }
    // Fallback when invoked outside cargo test harness.
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.pop();
    d.pop();
    let td = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| "/workspace/swarm/target/console-rust-serve-impl".into());
    PathBuf::from(td).join("release/galion-console-serve")
}

#[test]
fn real_console_rust_serve_parity() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let root = dist_root();
    assert!(
        root.is_dir(),
        "missing console-dist static at {} (scp from VPS or set CONSOLE_DIST_STATIC)",
        root.display()
    );
    assert!(
        root.join("favicon.svg").is_file(),
        "favicon.svg missing under {}",
        root.display()
    );

    let exe = bin();
    assert!(exe.is_file(), "binary missing at {}", exe.display());

    let child = Command::new(&exe)
        .args([
            "--dist",
            root.to_str().unwrap(),
            "--bind",
            BIND,
            "--ui",
            "rust",
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn galion-console-serve");
    let _k = Kill(child);
    assert!(wait_up(BIND, Duration::from_secs(5)), "server did not bind");

    // --- status parity vs recorded live srvx shapes (2026-10-27 CET) ---
    let (c, _, _) = fetch(BIND, "/", "127.0.0.1");
    assert_eq!(c, 302, "/ -> /console");

    let (c, head, _) = fetch(BIND, "/health", "127.0.0.1");
    assert_eq!(c, 404, "bare /health stays 404 like live srvx");
    assert!(
        !head.to_ascii_lowercase().contains("application/json"),
        "/health must not invent a health JSON"
    );

    let (c, _, body) = fetch(BIND, "/api/v1/health", "127.0.0.1");
    assert_eq!(c, 200);
    let s = String::from_utf8_lossy(&body);
    assert!(s.contains(r#""status":"healthy""#), "{s}");
    assert!(s.contains("galion-console-static"), "{s}");
    assert!(s.contains(r#""implementation":"rust""#), "{s}");
    assert!(
        !s.contains("galion-control") && !s.contains("3.8.3-board"),
        "must not claim live Node board identity: {s}"
    );

    let (c, _, body) = fetch(BIND, "/api/v1/health", "evil.example");
    assert_eq!(c, 401);
    assert_eq!(
        String::from_utf8_lossy(&body),
        r#"{"error":"phone required"}"#
    );

    let (c, _, body) = fetch(BIND, "/api/v1/session", "127.0.0.1");
    assert_eq!(c, 401);
    assert_eq!(
        String::from_utf8_lossy(&body),
        r#"{"error":"phone required"}"#
    );

    let (c, _, body) = fetch(BIND, "/api/v1/session-pack", "127.0.0.1");
    assert_eq!(c, 401);
    assert_eq!(
        String::from_utf8_lossy(&body),
        r#"{"error":"phone required"}"#
    );

    let (c, _, _) = fetch(BIND, "/favicon.svg", "127.0.0.1");
    assert_eq!(c, 200);
    let (c, _, _) = fetch(BIND, "/manifest.webmanifest", "127.0.0.1");
    assert_eq!(c, 200);

    // Scanner / source leak paths: must stay 404 (plain), not Vite source.
    for p in [
        "/package.json",
        "/vite.config.ts",
        "/@vite/client",
        "/.env",
        "/src/lib/host.ts",
    ] {
        let (c, _, _) = fetch(BIND, p, "127.0.0.1");
        assert_eq!(c, 404, "{p}");
    }

    // Rust UI page (not Node SSR HTML — documented).
    let (c, head, body) = fetch(BIND, "/console", "127.0.0.1");
    assert_eq!(c, 200, "/console rust ui");
    let h = head.to_ascii_lowercase();
    assert!(h.contains("text/html"));
    let html = String::from_utf8_lossy(&body);
    assert!(!html.contains("<script"), "V2 zero-JS: {html:.80}");

    let (c, _, _) = fetch(BIND, "/api/v1/ping", "127.0.0.1");
    assert_eq!(c, 200);

    // Optional live compare (status codes only) over SSH forward.
    let mut ssh_guard = KillSsh(None);
    if std::env::var("CONSOLE_RUST_SERVE_LIVE").ok().as_deref() == Some("1") {
        let ssh = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ExitOnForwardFailure=yes",
                "-N",
                "-L",
                &format!("{LIVE_FWD}:127.0.0.1:8080"),
                "slavik-vps",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("ssh -N");
        ssh_guard.0 = Some(ssh);
        assert!(wait_up(LIVE_FWD, Duration::from_secs(10)), "live forward");

        let pairs = [
            ("/", 302u16),
            ("/health", 404),
            ("/api/v1/session", 401),
            ("/api/v1/session-pack", 401),
            ("/favicon.svg", 200),
            ("/manifest.webmanifest", 200),
            ("/package.json", 404),
            ("/@vite/client", 404),
        ];
        for (path, expect) in pairs {
            let (live, _, _) = fetch(LIVE_FWD, path, "127.0.0.1");
            let (rust, _, _) = fetch(BIND, path, "127.0.0.1");
            assert_eq!(live, expect, "live {path}");
            assert_eq!(rust, live, "parity {path}: rust={rust} live={live}");
        }
        // Health: both 200 on console host; bodies intentionally differ (honest Rust).
        let (live, _, _) = fetch(LIVE_FWD, "/api/v1/health", "127.0.0.1");
        let (rust, _, _) = fetch(BIND, "/api/v1/health", "127.0.0.1");
        assert_eq!(live, 200);
        assert_eq!(rust, 200);
        eprintln!("real: live vs rust /api/v1/health both 200 (bodies differ by design)");
    }

    eprintln!(
        "real: console-rust-serve parity ok on {BIND} dist={}",
        root.display()
    );
    let _ = Path::new("."); // silence unused in some cfgs
}
