// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary test (slot 34, port 19342). Gated: runs only with
//! `GALION_REAL=1` and a console build at `CONSOLE_STATIC_OUT` (default
//! /workspace/swarm/artifacts/console-static/out:
//! a pre-built asset set: client/ + routes.txt). Scanner paths are also replayed from
//! the nerve capture journal when `/workspace/swarm/nerve-capture` exists.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:19342";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn status(path: &str) -> u16 {
    let mut s = TcpStream::connect(ADDR).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out[9..12]).parse().unwrap()
}

/// Paths the Vite dev server logged for scanner traffic in the capture.
fn captured_probes(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return found;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        for (marker, end) in [("Instead of ", ','), ("request id \\\"", '\\')] {
            let mut rest = text.as_str();
            while let Some(i) = rest.find(marker) {
                rest = &rest[i + marker.len()..];
                let path: String = rest
                    .chars()
                    .take_while(|c| *c != end && *c != ' ' && *c != '"')
                    .collect();
                if path.starts_with('/') && !found.contains(&path) {
                    found.push(path);
                }
            }
        }
    }
    found
}

#[test]
fn real_binary_serves_console_and_404s_probes() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let out = PathBuf::from(
        std::env::var("CONSOLE_STATIC_OUT")
            .unwrap_or_else(|_| "/workspace/swarm/artifacts/console-static/out".into()),
    );
    assert!(
        out.join("client/_shell.html").is_file(),
        "no console build at {}",
        out.display()
    );
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--bind", ADDR, "--quiet", "--root"])
        .arg(out.join("client"))
        .arg("--routes")
        .arg(out.join("routes.txt"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let _guard = Kill(child);
    let t0 = Instant::now();
    while TcpStream::connect(ADDR).is_err() {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let routes = std::fs::read_to_string(out.join("routes.txt")).unwrap();
    let pages: Vec<&str> = routes
        .lines()
        .filter_map(|l| l.strip_prefix("page "))
        .collect();
    assert!(pages.len() >= 10);
    for p in &pages {
        assert_eq!(status(p), 200, "{p}");
    }
    for p in ["/favicon.svg", "/manifest.webmanifest", "/og.jpg"] {
        assert_eq!(status(p), 200, "{p}");
    }
    let mut probes: Vec<String> = [
        "/.env",
        "/public/.env",
        "/phpinfo.php",
        "/public/phpinfo.php",
        "/.git/config",
        "/@fs/opt/galion/console-ide/.git/config",
        "/opt/galion/console-ide/.git/config",
        "/package.json",
        "/vite.config.ts",
        "/src/lib/host.ts",
        "/@vite/client",
        "/_shell.html",
        "/health",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let captured = captured_probes(Path::new("/workspace/swarm/nerve-capture"));
    eprintln!("captured scanner paths: {captured:?}");
    probes.extend(captured);
    for p in &probes {
        assert_eq!(status(p), 404, "{p}");
    }
}
