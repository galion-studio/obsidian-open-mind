// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Real-life tests for slot 380 edge-closed-ports-map (ports 22500-22509 only).
//!
//! 1. `real_20261028_map_on_box` (GALION_REAL=1): takes the secrets-free
//!    2026-10-28 VPS ingress copy, moves the 4 live origins (8671, 8091, 8080,
//!    8650) to 22500-22503 with real release `tuner` processes, the 19 retired
//!    ports round-robin onto 22504-22508 (left closed) and forge's 18800 alone
//!    onto 22509. Lints live (31 closed / 39), stages the cleanup proposal and
//!    re-lints it clean, then "brings forge back" (origin on 22509) to show the
//!    broken class turns OPEN while the retired class stays closed.
//! 2. `real_vps_live_map_matches_fixture` (GALION_REAL=1 GALION_REAL_VPS=1):
//!    read-only capture over `ssh slavik-vps` (config minus tunnel/credentials
//!    lines, `ss -ltnp`), linted on the box with `--ss`; asserts the live map
//!    still equals the committed fixture (drift alarm). Nothing is written on
//!    the VPS; the credentials file is never read.
//!
//! `cargo build --release -p tuner -p tunnel-lint`, then
//! `GALION_REAL=1 cargo test --release -p tunnel-lint --test real_edge_closed_ports_map -- --nocapture --test-threads 1`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LIVE: [(u16, u16); 4] = [(8671, 22500), (8091, 22501), (8080, 22502), (8650, 22503)];
const RETIRED: [u16; 19] = [
    3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444,
    5530, 8000, 8889,
];
const FORGE: (u16, u16) = (18800, 22509);

fn bin() -> String {
    std::env::var("TUNNEL_LINT_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_tunnel-lint").into())
}

fn gated(vps: bool) -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return false;
    }
    if vps && std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL_VPS=1");
        return false;
    }
    true
}

fn ts() -> String {
    Command::new("date")
        .arg("-Iseconds")
        .env("TZ", "Europe/Zagreb")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn step(s: &str) {
    eprintln!("\n== {} {s}", ts());
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("s380-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let o = Command::new(bin()).args(args).output().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn replace_port(text: &str, from: u16, to: u16) -> String {
    let pat = format!(":{from}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(&pat) {
        let end = i + pat.len();
        out.push_str(&rest[..i]);
        if rest[end..].starts_with(|c: char| c.is_ascii_digit()) {
            out.push_str(&pat);
        } else {
            out.push_str(&format!(":{to}"));
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn get_status(port: u16) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.write_all(b"GET / HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn tuner_bin() -> PathBuf {
    if let Ok(p) = std::env::var("TUNER_BIN") {
        return PathBuf::from(p);
    }
    Path::new(&bin()).parent().unwrap().join("tuner")
}

struct Origins(Vec<Child>);

impl Origins {
    fn start(&mut self, tuner: &Path, port: u16, www: &Path) {
        let c = Command::new(tuner)
            .env("TUNER_ADDR", format!("127.0.0.1:{port}"))
            .env("TUNER_ROOT", www)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        self.0.push(c);
        let t = Instant::now();
        while !listening(port) {
            assert!(t.elapsed() < Duration::from_secs(10), "origin {port} down");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Origins {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn summary(out: &str) -> &str {
    out.lines()
        .find(|l| l.starts_with("summary:"))
        .unwrap_or("")
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn real_20261028_map_on_box() {
    if !gated(false) {
        return;
    }
    let tuner = tuner_bin();
    assert!(tuner.is_file(), "build tuner first: {}", tuner.display());
    step("ports 22500-22509 must be free");
    for p in 22500..=22509 {
        assert!(!listening(p), "slot port {p} busy");
    }

    step("remap 2026-10-28 VPS copy onto slot ports");
    let dir = tmpdir("map");
    let mut text = std::fs::read_to_string(fixtures().join("vps-config-20261028.yml")).unwrap();
    for (from, to) in LIVE {
        text = replace_port(&text, from, to);
    }
    for (i, from) in RETIRED.iter().enumerate() {
        text = replace_port(&text, *from, 22504 + (i as u16 % 5));
    }
    text = replace_port(&text, FORGE.0, FORGE.1);
    for p in LIVE.iter().map(|l| l.0).chain(RETIRED).chain([FORGE.0]) {
        assert!(!text.contains(&format!(":{p}\n")), "port {p} not remapped");
    }
    let cfg = dir.join("config.yml");
    std::fs::write(&cfg, &text).unwrap();
    let c = cfg.to_str().unwrap();

    step("start 4 real tuner origins on 22500-22503");
    let www = dir.join("www");
    std::fs::create_dir_all(&www).unwrap();
    std::fs::write(www.join("index.html"), "<!doctype html>slot380 ok\n").unwrap();
    let mut origins = Origins(Vec::new());
    for (_, p) in LIVE {
        origins.start(&tuner, p, &www);
        let st = get_status(p);
        eprintln!("GET :{p} -> {st}");
        assert!(st.contains(" 200"), "{st}");
    }

    step("lint live: expect exit 1, 31 closed routes on 6 ports, 7 open");
    let (code, out, _) = run(&[c]);
    eprintln!("{out}exit={code:?}");
    assert_eq!(code, Some(1));
    assert_eq!(
        summary(&out),
        "summary: 39 rules, 31 closed routes on 6 ports \
         [22504, 22505, 22506, 22507, 22508, 22509], 31 findings"
    );
    let count = |k: &str| {
        out.lines()
            .filter(|l| l.split_whitespace().nth(1) == Some(k))
            .count()
    };
    assert_eq!((count("OPEN"), count("CLOSED"), count("SKIP")), (7, 31, 1));
    assert!(out.contains("CLOSED forge.galion.studio"));

    step("stage cleanup proposal, re-lint staged: expect exit 0, 8 rules");
    let staged = dir.join("staged.yml");
    let s = staged.to_str().unwrap();
    let (code, _, err) = run(&["--stage", s, "--no-fail", c]);
    eprint!("{err}");
    assert_eq!(code, Some(0));
    let (code, out, _) = run(&[s]);
    eprintln!("{}", summary(&out));
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(
        summary(&out),
        "summary: 8 rules, 0 closed routes on 0 ports [], 0 findings"
    );

    step("broken class: bring forge origin back on 22509 -> forge OPEN, retired stay closed");
    origins.start(&tuner, FORGE.1, &www);
    let (code, out, _) = run(&[c]);
    eprintln!("{}\nexit={code:?}", summary(&out));
    assert_eq!(code, Some(1));
    assert_eq!(
        summary(&out),
        "summary: 39 rules, 30 closed routes on 5 ports \
         [22504, 22505, 22506, 22507, 22508], 30 findings"
    );
    assert!(out
        .lines()
        .any(|l| l.contains("OPEN   forge.galion.studio")));

    step("teardown");
    drop(origins);
    for p in 22500..=22509 {
        assert!(!listening(p), "port {p} still busy after teardown");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn real_vps_live_map_matches_fixture() {
    if !gated(true) {
        return;
    }
    let host = std::env::var("GALION_VPS_SSH").unwrap_or_else(|_| "slavik-vps".into());
    let dir = tmpdir("vps");
    step("read-only capture: ingress config minus tunnel/credentials lines");
    let ssh = |cmd: &str| {
        let o = Command::new("ssh")
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=15", &host, cmd])
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "ssh failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let cfg_path = "/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml";
    let mut cfg = ssh(&format!(
        "grep -vE '^\\s*(tunnel|credentials-file|origin-cert)\\s*:' {cfg_path}"
    ));
    assert!(!cfg.contains("credentials-file:") && !cfg.contains("\ntunnel:"));
    cfg.insert_str(
        0,
        "credentials-file: /nonexistent/REDACTED-NOT-COPIED.json\n",
    );
    let ss = ssh("ss -ltnp");
    let c = dir.join("config.yml");
    let sf = dir.join("ss.txt");
    std::fs::write(&c, &cfg).unwrap();
    std::fs::write(&sf, &ss).unwrap();

    step("lint the live capture on the box (--ss)");
    let (code, live, _) = run(&["--ss", sf.to_str().unwrap(), c.to_str().unwrap()]);
    eprintln!("{}\nexit={code:?}", summary(&live));
    let fx = fixtures();
    let (_, fixed, _) = run(&[
        "--ss",
        fx.join("vps-ss-20261028.txt").to_str().unwrap(),
        fx.join("vps-config-20261028.yml").to_str().unwrap(),
    ]);
    let rows = |o: &str| -> Vec<String> {
        // rule, status, hostname, service (owner pids change across restarts).
        o.lines()
            .filter(|l| {
                l.split_whitespace()
                    .nth(1)
                    .is_some_and(|k| ["OPEN", "CLOSED", "SKIP"].contains(&k))
            })
            .map(|l| l.split_whitespace().take(4).collect::<Vec<_>>().join(" "))
            .collect()
    };
    let (a, b) = (rows(&live), rows(&fixed));
    for (x, y) in a.iter().zip(&b) {
        if x != y {
            eprintln!("DRIFT live `{x}` vs fixture `{y}`");
        }
    }
    assert_eq!(
        summary(&live),
        summary(&fixed),
        "live map drifted from fixture"
    );
    assert_eq!(a, b, "live rule table drifted from fixture");
    std::fs::remove_dir_all(&dir).ok();
}
