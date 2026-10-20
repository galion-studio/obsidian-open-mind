// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Real-life tests for `tunnel-lint` (wave-4 slot 103, ports 19730-19739 only).
//! Gated: both run only with GALION_REAL=1. They share the port range, so a
//! mutex serialises them.
//!
//! 1. `real_binary_flags_closed_slot_ports`: small config, plain TCP listeners
//!    on 19735/19736, 19737 left closed.
//! 2. `real_vps_copy_with_tuner_origins`: Rust port of the former
//!    `scripts/real/tunnel-audit.sh`. Takes the secrets-free VPS ingress copy,
//!    moves the 4 live VPS origins (8671, 8091, 8080, 8650) to 19730-19733 and
//!    the 21 dead VPS ports round-robin onto 19734-19739, starts 4 real release
//!    `tuner` origins, lints live, stages, kills one origin, revives a dead port,
//!    replays the VPS `ss` capture offline, checks the credentials guard, and
//!    tears everything down. Nothing touches the VPS.
//!
//! Build the real origin first (same CARGO_TARGET_DIR and profile):
//! `cargo build --release -p tuner -p tunnel-lint`, then
//! `GALION_REAL=1 cargo test --release -p tunnel-lint --test real_tunnel_lint -- --nocapture`.
//! `TUNER_BIN` overrides the tuner path.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static PORTS: Mutex<()> = Mutex::new(());

fn bin() -> String {
    std::env::var("TUNNEL_LINT_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_tunnel-lint").into())
}

fn gated() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return false;
    }
    true
}

/// Zagreb wall-clock stamp with explicit offset, for the log only.
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
    let d = std::env::temp_dir().join(format!("tunnel-lint-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let o: Output = Command::new(bin()).args(args).output().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// Replace `:FROM` with `:TO` wherever FROM is not followed by another digit.
fn replace_port(text: &str, from: u16, to: u16) -> String {
    let pat = format!(":{from}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(&pat) {
        let end = i + pat.len();
        let boundary = !rest[end..].starts_with(|c: char| c.is_ascii_digit());
        out.push_str(&rest[..i]);
        if boundary {
            out.push_str(&format!(":{to}"));
        } else {
            out.push_str(&pat);
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

fn wait_listen(port: u16) {
    let t = Instant::now();
    while !listening(port) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "origin on {port} did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Minimal HTTP/1.0 GET, returns the status line.
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
    Path::new(&bin())
        .parent()
        .unwrap()
        .join(format!("tuner{}", std::env::consts::EXE_SUFFIX))
}

/// Kills every started origin on drop, also when an assertion fails.
struct Origins(Vec<(u16, Child)>);

impl Origins {
    fn start(&mut self, tuner: &Path, port: u16, www: &Path) {
        let c = Command::new(tuner)
            .env("TUNER_ADDR", format!("127.0.0.1:{port}"))
            .env("TUNER_ROOT", www)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        self.0.push((port, c));
        wait_listen(port);
    }
    fn kill(&mut self, port: u16) {
        if let Some(i) = self.0.iter().position(|(p, _)| *p == port) {
            let (_, mut c) = self.0.remove(i);
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Origins {
    fn drop(&mut self) {
        for (_, c) in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn chmod(_: &Path, _: u32) {}

#[test]
fn real_binary_flags_closed_slot_ports() {
    if !gated() {
        return;
    }
    let _g = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmpdir("real-small");
    let cfg = dir.join("config.yml");
    std::fs::write(
        &cfg,
        concat!(
            "tunnel: 00000000-0000-0000-0000-000000000000\n",
            "ingress:\n",
            "  - hostname: up.example\n",
            "    service: http://localhost:19735\n",
            "  - hostname: up4.example\n",
            "    service: http://127.0.0.1:19736\n",
            "  - hostname: down.example\n",
            "    service: http://localhost:19737\n",
            "  - service: http_status:404\n",
        ),
    )
    .unwrap();
    let a = TcpListener::bind("127.0.0.1:19735").expect("port 19735 free");
    let b = TcpListener::bind("127.0.0.1:19736").expect("port 19736 free");
    let c = cfg.to_str().unwrap();

    let (code, text, _) = run(&["--tsv", c]);
    assert_eq!(code, Some(1), "{text}");
    assert!(
        text.contains("up.example\t\thttp://localhost:19735\t19735\tOPEN"),
        "{text}"
    );
    assert!(
        text.contains("up4.example\t\thttp://127.0.0.1:19736\t19736\tOPEN"),
        "{text}"
    );
    assert!(
        text.contains("down.example\t\thttp://localhost:19737\t19737\tCLOSED"),
        "{text}"
    );

    // Close one listener: the route must flip to CLOSED on the next run.
    drop(b);
    let (_, text, _) = run(&["--tsv", c]);
    assert!(text.contains("19736\tCLOSED"), "{text}");

    // Stage, then lint the staged copy: only the live route is left, exit 0.
    let staged = dir.join("staged.yml");
    let s = staged.to_str().unwrap();
    let (code, _, _) = run(&["--stage", s, c]);
    assert_eq!(code, Some(1));
    let (code, text, _) = run(&[s]);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("summary: 2 rules, 0 closed routes"), "{text}");
    drop(a);

    // Credentials guard.
    let (code, _, _) = run(&[dir.join("x.json").to_str().unwrap()]);
    assert_eq!(code, Some(2));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn real_vps_copy_with_tuner_origins() {
    if !gated() {
        return;
    }
    let _g = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let fix = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let tuner = tuner_bin();
    assert!(
        tuner.is_file(),
        "real origin missing: {} (build it with `cargo build --release -p tuner` \
         in the same CARGO_TARGET_DIR, or set TUNER_BIN)",
        tuner.display()
    );
    eprintln!("tunnel-lint: {}\ntuner: {}", bin(), tuner.display());

    step("ports 19730-19739 must be free");
    for p in 19730..=19739 {
        assert!(!listening(p), "slot port {p} busy");
    }

    step("box config from the VPS copy (live 8671/8091/8080/8650 -> 19730-19733, dead -> 19734-19739)");
    let dir = tmpdir("real-vps");
    let mut text = std::fs::read_to_string(fix.join("vps-config-20261020.yml")).unwrap();
    for (from, to) in [(8671, 19730), (8091, 19731), (8080, 19732), (8650, 19733)] {
        text = replace_port(&text, from, to);
    }
    let dead = [
        3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4000, 4001, 4003, 4004, 4020, 4042, 4050,
        4060, 4444, 5530, 7681, 8000, 8889,
    ];
    for (i, from) in dead.iter().enumerate() {
        text = replace_port(&text, *from, 19734 + (i as u16 % 6));
    }
    for p in [8671, 8091, 8080, 8650].iter().chain(dead.iter()) {
        assert!(!text.contains(&format!(":{p}\n")), "port {p} not remapped");
    }
    let cfg = dir.join("config.yml");
    std::fs::write(&cfg, &text).unwrap();
    chmod(&cfg, 0o666); // same mode as the VPS file, exercises the warning
    let c = cfg.to_str().unwrap();

    step("start real tuner origins on 19730-19733");
    let www = dir.join("www");
    std::fs::create_dir_all(&www).unwrap();
    std::fs::write(
        www.join("index.html"),
        "<!doctype html><title>slot103</title>ok\n",
    )
    .unwrap();
    let mut origins = Origins(Vec::new());
    for p in 19730..=19733 {
        origins.start(&tuner, p, &www);
        let st = get_status(p);
        eprintln!("GET :{p} -> {st}");
        assert!(st.contains(" 200"), "GET :{p} -> {st}");
    }

    step("lint live (expect exit 1: 33 closed routes on 6 ports, 5 open)");
    let (code, out, _) = run(&[c]);
    eprintln!("{out}exit={code:?}");
    assert_eq!(code, Some(1));
    assert!(
        out.contains(
            "summary: 39 rules, 33 closed routes on 6 ports \
             [19734, 19735, 19736, 19737, 19738, 19739]"
        ),
        "{out}"
    );
    let open = out
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some("OPEN"))
        .count();
    assert_eq!(open, 5, "{out}");
    assert!(out.contains("world-writable (mode 666)"), "{out}");
    assert!(out.contains("note: credentials-file is referenced; tunnel-lint did not read it"));

    step("stage, re-lint staged (expect exit 0)");
    let staged = dir.join("staged.yml");
    let s = staged.to_str().unwrap();
    let (code, _, err) = run(&["--stage", s, "--no-fail", c]);
    assert_eq!(code, Some(0), "{err}");
    eprint!("{err}");
    chmod(&staged, 0o640);
    let (code, out, _) = run(&[s]);
    eprintln!("{out}exit={code:?}");
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("summary: 6 rules, 0 closed routes on 0 ports [], 0 findings"),
        "{out}"
    );

    step("kill the tuner on 19733 (tuner.galion.app) and re-lint");
    origins.kill(19733);
    let (code, out, _) = run(&["--tsv", c]);
    let line = out
        .lines()
        .find(|l| l.contains("tuner.galion.app"))
        .unwrap_or("")
        .to_string();
    eprintln!("{line}\nexit={code:?}");
    assert_eq!(code, Some(1));
    assert!(line.ends_with("19733\tCLOSED\t"), "{line:?}");

    step("revive dead port 19734 with a real tuner and re-lint");
    origins.start(&tuner, 19734, &www);
    let (_, out, _) = run(&["--tsv", c]);
    let revived: Vec<&str> = out.lines().filter(|l| l.contains("\t19734\t")).collect();
    eprintln!("{}", revived.join("\n"));
    assert!(!revived.is_empty());
    assert!(
        revived.iter().all(|l| l.contains("\t19734\tOPEN")),
        "{revived:?}"
    );

    step("offline replay of the VPS capture (ss -ltnp 2026-10-20 20:35 +02:00)");
    let ss = fix.join("vps-ss-20261020.txt");
    let vps = fix.join("vps-config-20261020.yml");
    let (code, out, _) = run(&["--ss", ss.to_str().unwrap(), vps.to_str().unwrap()]);
    let summary = out
        .lines()
        .find(|l| l.starts_with("summary:"))
        .unwrap_or("")
        .to_string();
    eprintln!("{summary}\nexit={code:?}");
    assert_eq!(code, Some(1));
    assert_eq!(
        summary,
        "summary: 39 rules, 33 closed routes on 21 ports [3004, 3005, 3010, 3011, 3015, \
         3020, 3080, 3333, 4000, 4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444, 5530, \
         7681, 8000, 8889], 33 findings"
    );

    step("credentials guard (must refuse *.json, exit 2)");
    let (code, _, err) = run(&["/nonexistent/tunnel.json"]);
    eprint!("{err}");
    assert_eq!(code, Some(2));
    assert!(err.contains("credentials are never read"), "{err}");

    step("teardown");
    drop(origins);
    std::thread::sleep(Duration::from_millis(100));
    for p in 19730..=19739 {
        assert!(!listening(p), "listener left behind on {p}");
    }
    std::fs::remove_dir_all(&dir).ok();
    eprintln!("PASS {}", ts());
}
