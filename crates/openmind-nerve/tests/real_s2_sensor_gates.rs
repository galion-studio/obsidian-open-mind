// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 404 (D-385-2) real test, gated `GALION_REAL=1`, box only.
//!
//! The seven dependency-free S2 sensor features of `openmind-nerve`
//! (`sensor-sshd`, `sensor-connrate`, `sensor-proc-owner`, `sensor-surface`,
//! `sensor-hades`, `sensor-event-feed`, `sensor-edgerate`; all default on),
//! end to end on real release binaries:
//! 1. builds x86_64 musl release `-p openmind-nerve` alone as `default`,
//!    `s2-off` (slot 385's four features only: the seven compiled out) and
//!    `lean` (`--no-default-features`); with `NERVE_S2G_TABLE=1` also one
//!    `minus-<feature>` build per sensor for the size table;
//! 2. sizes: all under 2 MiB, `s2-off` saves at least [`MIN_S2_SAVING`]
//!    over `default`, `lean` < `s2-off`, every `minus-*` < `default`;
//! 3. serves each on a slot port with every gated sensor switched on
//!    (box-safe values: closed surface target, empty event file, own port
//!    for conn-rate, own user for proc-owner, `NERVE_SSHD_AUTH_LOG=0`) and
//!    reads `/state` after the first cycle:
//!    - `default`: all seven run (plus 385's three), no compiled-out line,
//!      `edge_hosts` is an object;
//!    - `s2-off`: none of the seven, 385's three still run, each of the
//!      seven named once on stderr, `edge_hosts` null;
//!    - `lean`: none of the ten, each of the ten named once;
//!    - `lean` with no S2 env at all: exactly one line (edge-rate is on by
//!      default); with only `NERVE_EDGE_HOSTS=0`: no line.
//!
//! Knobs: `NERVE_S2G_DIR` (nested target dir, default `<workspace>/target/s2-gates`),
//! `NERVE_S2G_JOBS` (default 2), `NERVE_S2G_PORT_BASE` (10-port block, default 22740),
//! `NERVE_S2G_TABLE=1` (seven more builds for the per-sensor size table).

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BUDGET: u64 = 2 * 1024 * 1024;
/// Slot 385 measured the seven together at 184 320 B (x86_64 musl). Floor
/// well under that so layout noise cannot flake it.
const MIN_S2_SAVING: u64 = 128 * 1024;
const BUILTINS: [&str; 5] = ["listeners", "scanners", "edge", "components", "ddos"];
/// (sensor name, cargo feature) for the seven gated here.
const S2: [(&str, &str); 7] = [
    ("sshd", "sensor-sshd"),
    ("conn-rate", "sensor-connrate"),
    ("proc-owner", "sensor-proc-owner"),
    ("surface", "sensor-surface"),
    ("hades", "sensor-hades"),
    ("event-feed", "sensor-event-feed"),
    ("edge-rate", "sensor-edgerate"),
];
/// Slot 385's three (all on in `s2-off`).
const S385: [(&str, &str); 3] = [
    ("rustsec", "sensor-rustsec"),
    ("drift", "sensor-drift"),
    ("ingress-lint", "sensor-ingress-lint"),
];
const ALL_FEATURES: [&str; 12] = [
    "sensor-rustsec",
    "rustsec-snapshot",
    "sensor-drift",
    "sensor-ingress-lint",
    "sensor-sshd",
    "sensor-connrate",
    "sensor-proc-owner",
    "sensor-surface",
    "sensor-hades",
    "sensor-event-feed",
    "sensor-edgerate",
    // Slot 424: staged memory freshness (slot 405), off unless NERVE_MEMFRESH_BIN.
    "sensor-memory-freshness",
];

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn build(label: &str, extra: &[String], dir: &Path) -> PathBuf {
    let jobs = std::env::var("NERVE_S2G_JOBS").unwrap_or_else(|_| "2".into());
    let t = Instant::now();
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            "x86_64-unknown-linux-musl",
            "-p",
            "openmind-nerve",
            "-j",
            &jobs,
            "--target-dir",
        ])
        .arg(dir)
        .args(extra)
        .current_dir(root())
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn cargo");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{label} build failed: {err}");
    let src = dir.join("x86_64-unknown-linux-musl/release/openmind-nerve");
    let dst = dir.join(format!("openmind-nerve-{label}"));
    std::fs::copy(&src, &dst).unwrap();
    eprintln!("{label}: built in {:.1} s", t.elapsed().as_secs_f64());
    dst
}

fn features_without(off: &[&str]) -> Vec<String> {
    let keep: Vec<&str> = ALL_FEATURES
        .iter()
        .copied()
        .filter(|f| !off.contains(f))
        .collect();
    let mut v = vec!["--no-default-features".to_string()];
    if !keep.is_empty() {
        v.push("--features".into());
        v.push(keep.join(","));
    }
    v
}

fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn port_closed(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}").parse().unwrap();
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err()
}

struct Run {
    sensors: Vec<String>,
    edge_hosts: Value,
    stderr: String,
    cycle: u64,
}

impl Run {
    fn has(&self, n: &str) -> bool {
        self.sensors.iter().any(|s| s == n)
    }
    fn warned(&self, feature: &str) -> usize {
        self.stderr.matches(&format!("`{feature}` feature")).count()
    }
    fn warn_lines(&self) -> usize {
        self.stderr
            .lines()
            .filter(|l| l.contains("but this build has no"))
            .count()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Env {
    /// Every gated sensor (seven + 385's three) switched on.
    All,
    /// No S2 variable at all.
    Bare,
    /// Only `NERVE_EDGE_HOSTS=0`.
    EdgeOff,
}

/// Serve `bin` on `port`; return the `/state` after the first cycle that
/// ran every registered sensor.
fn run(label: &str, bin: &Path, port: u16, closed: u16, dir: &Path, mode: Env) -> Run {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let data = dir.join(format!("data-{label}"));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let comps = dir.join(format!("components-{label}.json"));
    std::fs::write(&comps, "[]").unwrap();
    let events = dir.join(format!("events-{label}.jsonl"));
    std::fs::write(&events, "").unwrap();
    let err_path = dir.join(format!("stderr-{label}.txt"));
    let err_file = std::fs::File::create(&err_path).unwrap();
    assert!(port_closed(port), "port {port} already in use");
    let user = std::env::var("USER").unwrap_or_else(|_| "root".into());
    let mut cmd = Command::new(bin);
    cmd.arg("serve")
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
        .env("NERVE_LISTEN_SCOPE", format!("{port}-{port}"));
    for k in [
        "NERVE_SENSORS_OFF",
        "NERVE_RUSTSEC_DB",
        "NERVE_RUSTSEC_LOCK",
        "NERVE_DRIFT_MANIFEST",
        "NERVE_INGRESS_LINT_CONFIG",
        "NERVE_SSHD",
        "NERVE_CONNRATE_PORTS",
        "NERVE_PROC_OWNER_USERS",
        "NERVE_SURFACE_TARGETS",
        "NERVE_HADES",
        "NERVE_HADES_LEDGER",
        "NERVE_HADES_AUDIT",
        "NERVE_HADES_DISABLE_FILE",
        "NERVE_EVENT_FEED_FILE",
        "NERVE_EDGE_HOSTS",
    ] {
        cmd.env_remove(k);
    }
    match mode {
        Env::All => {
            cmd.env("NERVE_RUSTSEC_LOCK", fx.join("rustsec/Cargo.lock"))
                .env("NERVE_RUSTSEC_COMPONENT", "s2g-svc")
                .env("NERVE_DRIFT_MANIFEST", fx.join("drift/SHA256SUMS"))
                .env("NERVE_DRIFT_ROOT", fx.join("drift/deployed"))
                .env(
                    "NERVE_INGRESS_LINT_CONFIG",
                    fx.join("ingress-lint/vps-config-20261021.yml"),
                )
                .env("NERVE_SSHD", "1")
                .env("NERVE_SSHD_AUTH_LOG", "0")
                .env("NERVE_CONNRATE_PORTS", format!("{port}=s2g-svc"))
                .env("NERVE_PROC_OWNER_USERS", &user)
                .env(
                    "NERVE_SURFACE_TARGETS",
                    format!("http://127.0.0.1:{closed}/@200"),
                )
                .env("NERVE_SURFACE_TIMEOUT_MS", "300")
                .env("NERVE_HADES", "1")
                .env("NERVE_EVENT_FEED_FILE", &events)
                .env("NERVE_EDGE_HOSTS", "1");
        }
        Env::Bare => {}
        Env::EdgeOff => {
            cmd.env("NERVE_EDGE_HOSTS", "0");
        }
    }
    let nerve = Kill(
        cmd.stdout(Stdio::null())
            .stderr(err_file)
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    let st = loop {
        if let Some((200, b)) = http_get(port, "/state") {
            let v: Value = serde_json::from_str(&b).unwrap_or_default();
            if v["cycle"].as_u64().unwrap_or(0) >= 1
                && v["cortex_sensors"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
            {
                break v;
            }
        }
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "{label}: no /state with cycle>=1 in 60 s"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    drop(nerve);
    assert!(port_closed(port), "{label}: :{port} still open after kill");
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_file(&comps);
    let _ = std::fs::remove_file(&events);
    let _ = std::fs::remove_file(&err_path);
    Run {
        sensors: st["cortex_sensors"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["name"].as_str().map(str::to_string))
            .collect(),
        edge_hosts: st["edge_hosts"].clone(),
        stderr,
        cycle: st["cycle"].as_u64().unwrap_or(0),
    }
}

fn sha_short(p: &Path) -> String {
    Command::new("sha256sum")
        .arg(p)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.chars().take(8).collect())
        .unwrap_or_default()
}

#[test]
fn real_s2_sensor_gates() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1 (builds 3+ release nerves, box only)");
        return;
    }
    let dir = std::env::var("NERVE_S2G_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("target/s2-gates"));
    std::fs::create_dir_all(&dir).unwrap();
    let base = galion_realtest::ports::base_from_env("NERVE_S2G_PORT_BASE", 22740, 10);
    let closed = base + 9;
    assert!(
        port_closed(closed),
        "surface target :{closed} must be closed"
    );

    let s2_feats: Vec<&str> = S2.iter().map(|x| x.1).collect();
    let default = build("default", &[], &dir);
    let s2_off = build("s2-off", &features_without(&s2_feats), &dir);
    let lean = build("lean", &features_without(&ALL_FEATURES), &dir);
    let size = |p: &Path| std::fs::metadata(p).unwrap().len();
    let (d, s, l) = (size(&default), size(&s2_off), size(&lean));
    for (label, b, n) in [
        ("default", &default, d),
        ("s2-off", &s2_off, s),
        ("lean", &lean, l),
    ] {
        assert!(n < BUDGET, "{label}: {n} B over the 2 MiB budget");
        eprintln!(
            "size {label}: {n} B (headroom {} B) sha {}",
            BUDGET - n,
            sha_short(b)
        );
    }
    assert!(d - s >= MIN_S2_SAVING, "the seven save only {} B", d - s);
    assert!(l < s, "lean {l} < s2-off {s}");
    eprintln!(
        "sizes: default {d} B, s2-off {s} B (-{} B), lean {l} B (-{} B)",
        d - s,
        d - l
    );

    if std::env::var("NERVE_S2G_TABLE").as_deref() == Ok("1") {
        let mut sum = 0;
        for (name, f) in S2 {
            let b = build(&format!("minus-{f}"), &features_without(&[f]), &dir);
            let n = size(&b);
            assert!(n < d, "minus {f}: {n} B not below default {d} B");
            sum += d - n;
            eprintln!("table {name} ({f}): {n} B, saves {} B", d - n);
            let _ = std::fs::remove_file(&b);
        }
        eprintln!(
            "table: singles add to {sum} B, the seven together save {} B",
            d - s
        );
        // Slot 424: the staged memory freshness sensor (slot 405) is gated too.
        let f = "sensor-memory-freshness";
        let b = build(&format!("minus-{f}"), &features_without(&[f]), &dir);
        let n = size(&b);
        assert!(n <= d, "minus {f}: {n} B above default {d} B");
        eprintln!("table memory-freshness ({f}): {n} B, saves {} B", d - n);
        let _ = std::fs::remove_file(&b);
    }

    // All gated sensors switched on.
    let r = run("default", &default, base, closed, &dir, Env::All);
    eprintln!(
        "default :{base} cycle {} sensors {:?} edge_hosts {}",
        r.cycle, r.sensors, r.edge_hosts
    );
    for b in BUILTINS {
        assert!(r.has(b), "default: built-in {b} missing");
    }
    for (n, _) in S2.iter().chain(S385.iter()) {
        assert!(r.has(n), "default: {n} missing from {:?}", r.sensors);
    }
    assert_eq!(r.warn_lines(), 0, "default stderr: {}", r.stderr);
    assert!(
        r.edge_hosts.is_object(),
        "default edge_hosts {}",
        r.edge_hosts
    );

    let r = run("s2-off", &s2_off, base + 1, closed, &dir, Env::All);
    eprintln!(
        "s2-off :{} cycle {} sensors {:?} edge_hosts {} warnings {}",
        base + 1,
        r.cycle,
        r.sensors,
        r.edge_hosts,
        r.warn_lines()
    );
    for b in BUILTINS {
        assert!(r.has(b), "s2-off: built-in {b} missing");
    }
    for (n, f) in S2 {
        assert!(!r.has(n), "s2-off: {n} still runs");
        assert_eq!(r.warned(f), 1, "{f} once; stderr: {}", r.stderr);
    }
    for (n, f) in S385 {
        assert!(r.has(n), "s2-off: {n} missing");
        assert_eq!(r.warned(f), 0, "{f}");
    }
    assert_eq!(r.warn_lines(), 7, "{}", r.stderr);
    assert!(r.edge_hosts.is_null(), "s2-off edge_hosts {}", r.edge_hosts);

    let r = run("lean", &lean, base + 2, closed, &dir, Env::All);
    eprintln!(
        "lean :{} cycle {} sensors {:?} warnings {}",
        base + 2,
        r.cycle,
        r.sensors,
        r.warn_lines()
    );
    assert_eq!(r.sensors, BUILTINS, "lean runs only the built-ins");
    for (_, f) in S2.iter().chain(S385.iter()) {
        assert_eq!(r.warned(f), 1, "{f} once; stderr: {}", r.stderr);
    }
    assert_eq!(r.warn_lines(), 10, "{}", r.stderr);
    for line in r.stderr.lines().filter(|l| l.contains("but this build")) {
        assert!(line.starts_with("<4>openmind-nerve: "), "{line}");
        eprintln!("lean stderr: {line}");
    }

    let r = run("lean-bare", &lean, base + 3, closed, &dir, Env::Bare);
    assert_eq!(r.sensors, BUILTINS);
    assert_eq!(r.warn_lines(), 1, "edge-rate default only: {}", r.stderr);
    assert_eq!(r.warned("sensor-edgerate"), 1, "{}", r.stderr);
    assert!(r.stderr.contains("NERVE_EDGE_HOSTS unset"), "{}", r.stderr);
    eprintln!("lean-bare :{} warnings 1 (edge-rate default)", base + 3);

    let r = run("lean-edge-off", &lean, base + 4, closed, &dir, Env::EdgeOff);
    assert_eq!(r.sensors, BUILTINS);
    assert_eq!(r.warn_lines(), 0, "{}", r.stderr);
    eprintln!("lean-edge-off :{} warnings 0", base + 4);

    let r = run("default-bare", &default, base + 5, closed, &dir, Env::Bare);
    let mut want: Vec<&str> = BUILTINS.to_vec();
    want.push("edge-rate");
    assert_eq!(r.sensors, want, "default with no env");
    assert_eq!(r.warn_lines(), 0, "{}", r.stderr);
    eprintln!("default-bare :{} sensors {:?}", base + 5, r.sensors);

    for p in base..base + 10 {
        assert!(port_closed(p), ":{p} left open");
    }
}
