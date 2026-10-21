// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the tuner binary on loopback ports 19071-19073 (swarm
//! slot 07). Runs only with `GALION_REAL=1`. `TUNER_REAL_BIN` names the binary
//! (the release musl build in `scripts/real/tuner-cutover.sh`); without it the
//! test-profile binary is used. Every route of the matrix is sent over real TCP
//! and diffed byte-for-byte against the live VPS fixtures. Never binds 8650.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PORT_SERVE: u16 = 19071;
const PORT_BUSY: u16 = 19072;
const PORT_FREE: u16 = 19073;

fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin() -> String {
    std::env::var("TUNER_REAL_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_tuner").to_string())
}

struct Kid(Option<Child>);

impl Drop for Kid {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn start(port: u16) -> Kid {
    let www = manifest().join("www");
    let kid = Kid(Some(
        Command::new(bin())
            .env_clear()
            .env("TUNER_ADDR", format!("127.0.0.1:{port}"))
            .env("TUNER_ROOT", &www)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn tuner"),
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return kid;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("tuner did not listen on {port}");
}

fn raw(port: u16, req: &[u8]) -> Vec<u8> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(req).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    out
}

fn unescape(fmt: &str) -> Vec<u8> {
    fmt.replace("\\r", "\r").replace("\\n", "\n").into_bytes()
}

#[test]
fn real_binary_matches_live_on_every_route() {
    if !enabled() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let _kid = start(PORT_SERVE);
    let text =
        fs::read_to_string(manifest().join("../../scripts/real/tuner-requests.txt")).unwrap();
    let mut n = 0;
    for line in text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let (name, fmt) = line.split_once(char::is_whitespace).unwrap();
        let want = fs::read(manifest().join(format!("tests/fixtures/live/{name}.http"))).unwrap();
        let got = raw(PORT_SERVE, &unescape(fmt.trim()));
        assert!(
            got == want,
            "{name}: {} vs live {} bytes",
            got.len(),
            want.len()
        );
        n += 1;
    }
    assert!(n >= 20);

    let head = raw(PORT_SERVE, b"HEAD /health HTTP/1.1\r\n\r\n");
    assert!(head.ends_with(b"Connection: close\r\n\r\n"));
    let post = raw(
        PORT_SERVE,
        b"POST /health HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
    );
    assert!(post.starts_with(b"HTTP/1.1 405 Method Not Allowed\r\n"));

    // 16 parallel clients, every one gets the full page.
    let page = fs::read(manifest().join("tests/fixtures/live/root.http")).unwrap();
    let handles: Vec<_> = (0..16)
        .map(|_| thread::spawn(|| raw(PORT_SERVE, b"GET / HTTP/1.1\r\n\r\n")))
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), page);
    }

    // A client that connects and sends nothing is dropped after the 5 s read
    // timeout without blocking others.
    let idle = TcpStream::connect(("127.0.0.1", PORT_SERVE)).unwrap();
    let t0 = Instant::now();
    let health = raw(PORT_SERVE, b"GET /health HTTP/1.1\r\n\r\n");
    assert!(health.ends_with(b"{\"ok\":true}\n"));
    assert!(t0.elapsed() < Duration::from_secs(2));
    drop(idle);
}

#[test]
fn real_flags_exit_without_binding() {
    if !enabled() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    // galion-deploy runs `<new> --version`; it must exit even with the port free.
    let out = Command::new(bin())
        .arg("--version")
        .env("TUNER_ADDR", format!("127.0.0.1:{PORT_FREE}"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("tuner "));
    assert!(
        TcpListener::bind(("127.0.0.1", PORT_FREE)).is_ok(),
        "port stayed free"
    );

    let check = Command::new(bin())
        .arg("--check")
        .env("TUNER_ADDR", format!("127.0.0.1:{PORT_FREE}"))
        .env("TUNER_ROOT", manifest().join("www"))
        .output()
        .unwrap();
    assert!(check.status.success(), "{check:?}");
    let bad = Command::new(bin())
        .arg("--check")
        .env("TUNER_ROOT", "/nonexistent-tuner-root")
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));

    let typo = Command::new(bin()).arg("--serve").output().unwrap();
    assert_eq!(typo.status.code(), Some(2));
}

#[test]
fn real_busy_port_exits_1_cleanly() {
    if !enabled() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let _hold = TcpListener::bind(("127.0.0.1", PORT_BUSY)).unwrap();
    let out = Command::new(bin())
        .env("TUNER_ADDR", format!("127.0.0.1:{PORT_BUSY}"))
        .env("TUNER_ROOT", manifest().join("www"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("tuner: bind 127.0.0.1:19072"));
}
