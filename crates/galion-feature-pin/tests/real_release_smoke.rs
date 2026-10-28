// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Smoke on the exact release bytes of the two binaries added to RELEASE_BINS
//! in slot 337 (D-325-2): gaia-cortex (`llm-advisor`) and galion-console-serve.
//!
//! Gated: `GALION_REAL=1` and `GALION_RELEASE_BINS_DIR=<dir>` holding the
//! full-set binaries kept by `real_release_repro` (`<work>/out/<triple>/full`).
//! Each service is started alone on the slot 337 ports 22070-22079, probed
//! with real HTTP GETs, then killed. The cortex runs dry-run, with no nerve
//! (nothing listens on the nerve port) and LLM off; the console runs the live
//! `--dist --ui rust --local-api` argv on a throwaway dist dir. No network,
//! no LLM call, no live system touched.

mod common;

use common::RELEASE_BINS;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CORTEX_PORT: u16 = 22070;
const CONSOLE_PORT: u16 = 22071;
const CONSOLE_PULSE: u16 = 22078;
const NO_NERVE_PORT: u16 = 22079;

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

fn free(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_err()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("r337-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn spawn(bin: &Path, args: &[&str], env: &[(&str, String)]) -> Kill {
    let mut c = Command::new(bin);
    c.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
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

fn show(bin: &str, port: u16, path: &str, r: &(u16, String)) {
    let body: String = r.1.chars().take(160).collect();
    eprintln!("{bin} :{port}{path} -> {} {}", r.0, body.replace('\n', " "));
}

#[test]
fn new_release_bins_answer_on_their_release_bytes() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("real_release_smoke: skipped (set GALION_REAL=1)");
        return;
    }
    let Ok(dir) = std::env::var("GALION_RELEASE_BINS_DIR") else {
        eprintln!("real_release_smoke: skipped (set GALION_RELEASE_BINS_DIR)");
        return;
    };
    let dir = PathBuf::from(dir);
    for p in [CORTEX_PORT, CONSOLE_PORT, CONSOLE_PULSE, NO_NERVE_PORT] {
        assert!(free(p), "port {p} busy before the test");
    }
    let mut n = 0usize;

    // gaia-cortex: the llm-advisor transport is compiled in (the curl
    // `--proto =https,http` literal only exists with the feature).
    let cortex = dir.join("gaia-cortex");
    let bytes = std::fs::read(&cortex).expect("read gaia-cortex");
    assert!(
        bytes.windows(11).any(|w| w == b"=https,http"),
        "gaia-cortex release bytes lack the llm-advisor transport"
    );
    eprintln!(
        "gaia-cortex {} B: llm-advisor transport present",
        bytes.len()
    );
    n += 1;
    let out = Command::new(&cortex).arg("--version").output().unwrap();
    assert!(out.status.success());
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    eprintln!("gaia-cortex --version -> {v}");
    assert!(v.starts_with("gaia-cortex "));
    n += 1;
    let d = tmp("cortex");
    {
        let mut k = spawn(
            &cortex,
            &["serve"],
            &[
                ("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}")),
                ("CORTEX_DATA_DIR", d.display().to_string()),
                (
                    "CORTEX_NERVE_URL",
                    format!("http://127.0.0.1:{NO_NERVE_PORT}"),
                ),
                ("CORTEX_ACTIONS", "dry-run".into()),
                ("CORTEX_PULL_EVERY_S", "0".into()),
                ("CORTEX_LLM_ENABLED", "0".into()),
            ],
        );
        let h = wait_get(CORTEX_PORT, "/health", &mut k.0);
        show("gaia-cortex", CORTEX_PORT, "/health", &h);
        assert_eq!(h.0, 200);
        assert!(h.1.contains("\"service\":\"gaia-cortex\""), "{}", h.1);
        assert!(h.1.contains("\"mode\":\"dry-run\""), "{}", h.1);
        assert!(h.1.contains("\"llm_enabled\":false"), "{}", h.1);
        n += 1;
        for path in [
            "/stats",
            "/incidents",
            "/decisions",
            "/alerts",
            "/escalations",
        ] {
            let r = wait_get(CORTEX_PORT, path, &mut k.0);
            show("gaia-cortex", CORTEX_PORT, path, &r);
            assert_eq!(r.0, 200, "{path}");
            n += 1;
        }
        let r = wait_get(CORTEX_PORT, "/nope", &mut k.0);
        show("gaia-cortex", CORTEX_PORT, "/nope", &r);
        assert_eq!(r.0, 404);
        n += 1;
    }
    let _ = std::fs::remove_dir_all(&d);

    // galion-console-serve: the live :8080 argv shape on a throwaway dist.
    let serve = dir.join("galion-console-serve");
    let d = tmp("serve");
    std::fs::write(
        d.join("index.html"),
        "<!doctype html><title>r337 console-dist</title>",
    )
    .unwrap();
    {
        let bind = format!("127.0.0.1:{CONSOLE_PORT}");
        let pulse = format!("127.0.0.1:{CONSOLE_PULSE}");
        let mut k = spawn(
            &serve,
            &[
                "--bind",
                &bind,
                "--dist",
                d.to_str().unwrap(),
                "--ui",
                "rust",
                "--local-api",
                "--pulse",
                &pulse,
                "--quiet",
            ],
            &[],
        );
        let r = wait_get(CONSOLE_PORT, "/", &mut k.0);
        show("galion-console-serve", CONSOLE_PORT, "/", &r);
        assert!(r.0 == 200 || r.0 == 302, "{r:?}");
        n += 1;
        let h = wait_get(CONSOLE_PORT, "/api/v1/health", &mut k.0);
        show("galion-console-serve", CONSOLE_PORT, "/api/v1/health", &h);
        assert_eq!(h.0, 200);
        n += 1;
        let p = wait_get(CONSOLE_PORT, "/api/v1/ping", &mut k.0);
        show("galion-console-serve", CONSOLE_PORT, "/api/v1/ping", &p);
        assert_eq!(p.0, 200);
        n += 1;
        let s = wait_get(CONSOLE_PORT, "/api/v1/session", &mut k.0);
        show("galion-console-serve", CONSOLE_PORT, "/api/v1/session", &s);
        assert_eq!(s.0, 401, "unpaired session must be 401");
        n += 1;
        let t = wait_get(CONSOLE_PORT, "/../etc/passwd", &mut k.0);
        show("galion-console-serve", CONSOLE_PORT, "/../etc/passwd", &t);
        assert!(t.0 >= 400, "path traversal must be refused: {t:?}");
        assert!(!t.1.contains("root:"), "leaked a host file");
        n += 1;
    }
    let _ = std::fs::remove_dir_all(&d);

    for p in [CORTEX_PORT, CONSOLE_PORT, CONSOLE_PULSE, NO_NERVE_PORT] {
        assert!(free(p), "port {p} still busy after the test");
    }
    assert!(RELEASE_BINS.iter().any(|r| r.bin == "gaia-cortex"));
    eprintln!("real_release_smoke: {n} checks passed ({})", dir.display());
}
