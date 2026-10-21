// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! UCPU-1 real-life test: the static aarch64-musl `tuner` binary runs under
//! qemu-user on an x86_64 host and serves real HTTP.
//!
//! Gated: runs only with `GALION_REAL=1`. Needs `qemu-aarch64-static` and
//! the release binary, built by `scripts/ucpu-gate.sh build` or
//! `scripts/real/ucpu.sh`:
//!
//!   GALION_REAL=1 cargo test -p tuner --test real_ucpu -- --nocapture
//!
//! Binary: `$UCPU_TUNER_BIN`, else
//! `$CARGO_TARGET_DIR/aarch64-unknown-linux-musl/release/tuner`.
//! Port 19132 (swarm slot 13; do not run while scripts/real/ucpu.sh runs).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:19132";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(path: &str) -> Option<String> {
    let mut s = TcpStream::connect(ADDR).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {ADDR}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

fn tuner_bin() -> PathBuf {
    if let Ok(p) = std::env::var("UCPU_TUNER_BIN") {
        return PathBuf::from(p);
    }
    let td = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../target").to_string());
    PathBuf::from(td).join("aarch64-unknown-linux-musl/release/tuner")
}

#[test]
fn aarch64_tuner_serves_http_under_qemu() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1 to run the qemu real-life test");
        return;
    }
    let bin = tuner_bin();
    assert!(bin.is_file(), "missing {}", bin.display());
    // e_machine at offset 18 (little endian): 0xB7 = AArch64.
    let head = std::fs::read(&bin).expect("read binary");
    assert_eq!(&head[..4], b"\x7fELF");
    assert_eq!(
        u16::from_le_bytes([head[18], head[19]]),
        0xB7,
        "not aarch64"
    );

    let www = std::env::temp_dir().join(format!("ucpu-tuner-www-{}", std::process::id()));
    std::fs::create_dir_all(&www).unwrap();
    std::fs::write(
        www.join("index.html"),
        "<title>ucpu rust real test</title>\n",
    )
    .unwrap();

    let child = Command::new("qemu-aarch64-static")
        .arg(&bin)
        .env("TUNER_ADDR", ADDR)
        .env("TUNER_ROOT", &www)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn qemu-aarch64-static (apt install qemu-user-static)");
    let _guard = Kill(child);

    let start = Instant::now();
    let health = loop {
        if let Some(r) = get("/health") {
            break r;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "no listener on {ADDR}"
        );
        std::thread::sleep(Duration::from_millis(300));
    };
    eprintln!("emulated start-up + first reply: {:?}", start.elapsed());
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.contains("\"ok\":true"), "{health}");

    let index = get("/").expect("GET /");
    assert!(index.starts_with("HTTP/1.1 200"), "{index}");
    assert!(index.contains("ucpu rust real test"), "{index}");

    let missing = get("/no-such-file").expect("GET /no-such-file");
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
    let _ = std::fs::remove_dir_all(&www);
}
