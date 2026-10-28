// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 385 (F-376-1) real test, gated `GALION_REAL=1`, box only.
//!
//! The optional sensor features of `openmind-nerve` (`sensor-rustsec`,
//! `rustsec-snapshot`, `sensor-drift`, `sensor-ingress-lint`; all default on)
//! end to end:
//! 1. builds three x86_64 musl release binaries with `-p openmind-nerve`
//!    alone: `default`, `no-snapshot` (rustsec sensor without the embedded
//!    advisory snapshot) and `lean` (`--no-default-features`);
//! 2. every binary is under the 2 MiB budget, `lean` saves at least
//!    [`MIN_LEAN_SAVING`] over `default`, `no-snapshot` sits in between;
//! 3. starts each one on a slot port with the switch-on variables of all
//!    three gated sensors set (fixture Cargo.lock, drift manifest, ingress
//!    config copy) and reads `/state`:
//!    - `default`: `rustsec`, `drift`, `ingress-lint` run, and the rustsec
//!      fixture finding `rustsec-smallvec-1.6.0` is reported;
//!    - `no-snapshot`: `rustsec` runs and reports `rustsec-unreadable`
//!      naming `NERVE_RUSTSEC_DB` (never a silent clean bill);
//!    - `lean`: none of the three run, the five built-ins still do, and
//!      stderr names each compiled-out sensor once.
//!
//! Knobs: `NERVE_DG_DIR` (nested target dir, default `<workspace>/target/dep-growth`),
//! `NERVE_DG_JOBS` (default 2), `NERVE_DG_PORT_BASE` (3-port block, default 22550).

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BUDGET: u64 = 2 * 1024 * 1024;
/// Measured 2026-10-28 (slot 385): default 1 930 048 B, lean 1 745 728 B
/// (-184 320 B). Floor well under that so layout noise cannot flake it.
const MIN_LEAN_SAVING: u64 = 128 * 1024;
const BUILTINS: [&str; 5] = ["listeners", "scanners", "edge", "components", "ddos"];

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

fn build(label: &str, extra: &[&str], dir: &Path) -> PathBuf {
    let jobs = std::env::var("NERVE_DG_JOBS").unwrap_or_else(|_| "2".into());
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
    vulns: Vec<(String, String)>,
    stderr: String,
    cycle: u64,
}

/// Serve `bin` on `port` with all three gated sensors switched on; return
/// the `/state` after the first cycle that ran every registered sensor.
fn run(label: &str, bin: &Path, port: u16, dir: &Path) -> Run {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let data = dir.join(format!("data-{label}"));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let comps = dir.join(format!("components-{label}.json"));
    std::fs::write(&comps, "[]").unwrap();
    let err_path = dir.join(format!("stderr-{label}.txt"));
    let err_file = std::fs::File::create(&err_path).unwrap();
    assert!(port_closed(port), "port {port} already in use");
    let nerve = Kill(
        Command::new(bin)
            .arg("serve")
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
            .env("NERVE_LISTEN_SCOPE", format!("{port}-{port}"))
            .env_remove("NERVE_SENSORS_OFF")
            .env_remove("NERVE_RUSTSEC_DB")
            .env("NERVE_RUSTSEC_LOCK", fx.join("rustsec/Cargo.lock"))
            .env("NERVE_RUSTSEC_COMPONENT", "dg-svc")
            .env("NERVE_DRIFT_MANIFEST", fx.join("drift/SHA256SUMS"))
            .env("NERVE_DRIFT_ROOT", fx.join("drift/deployed"))
            .env(
                "NERVE_INGRESS_LINT_CONFIG",
                fx.join("ingress-lint/vps-config-20261021.yml"),
            )
            .stdout(Stdio::null())
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
    let _ = std::fs::remove_file(&err_path);
    Run {
        sensors: st["cortex_sensors"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["name"].as_str().map(str::to_string))
            .collect(),
        vulns: st["cortex"]["vulnerabilities"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|f| {
                        (
                            f["id"].as_str().unwrap_or("").to_string(),
                            f["title"].as_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        stderr,
        cycle: st["cycle"].as_u64().unwrap_or(0),
    }
}

#[test]
fn real_dep_growth() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1 (builds 3 release nerves, box only)");
        return;
    }
    let dir = std::env::var("NERVE_DG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("target/dep-growth"));
    std::fs::create_dir_all(&dir).unwrap();
    let base = galion_realtest::ports::base_from_env("NERVE_DG_PORT_BASE", 22550, 3);

    let variants: [(&str, &[&str]); 3] = [
        ("default", &[]),
        (
            "no-snapshot",
            &[
                "--no-default-features",
                "--features",
                "sensor-rustsec,sensor-drift,sensor-ingress-lint",
            ],
        ),
        ("lean", &["--no-default-features"]),
    ];
    let mut sizes = Vec::new();
    for (i, (label, extra)) in variants.iter().enumerate() {
        let bin = build(label, extra, &dir);
        let size = std::fs::metadata(&bin).unwrap().len();
        assert!(size < BUDGET, "{label}: {size} B over the 2 MiB budget");
        let r = run(label, &bin, base + i as u16, &dir);
        eprintln!(
            "{label}: {size} B (headroom {} B) :{} cycle {} sensors {:?} vulns {:?}",
            BUDGET - size,
            base + i as u16,
            r.cycle,
            r.sensors,
            r.vulns.iter().map(|v| &v.0).collect::<Vec<_>>()
        );
        for b in BUILTINS {
            assert!(
                r.sensors.iter().any(|s| s == b),
                "{label}: built-in {b} missing"
            );
        }
        let has = |n: &str| r.sensors.iter().any(|s| s == n);
        let warned = |f: &str| r.stderr.matches(&format!("`{f}` feature")).count();
        match *label {
            "default" => {
                assert!(
                    has("rustsec") && has("drift") && has("ingress-lint"),
                    "{:?}",
                    r.sensors
                );
                assert!(
                    r.vulns.iter().any(|v| v.0 == "rustsec-smallvec-1.6.0"),
                    "embedded snapshot judges the fixture lock: {:?}",
                    r.vulns
                );
                assert_eq!(
                    warned("sensor-rustsec")
                        + warned("sensor-drift")
                        + warned("sensor-ingress-lint"),
                    0
                );
            }
            "no-snapshot" => {
                assert!(
                    has("rustsec") && has("drift") && has("ingress-lint"),
                    "{:?}",
                    r.sensors
                );
                let u = r.vulns.iter().find(|v| v.0 == "rustsec-unreadable");
                assert!(
                    u.is_some_and(|v| v.1.contains("NERVE_RUSTSEC_DB")),
                    "{:?}",
                    r.vulns
                );
                assert!(!r.vulns.iter().any(|v| v.0.starts_with("rustsec-smallvec")));
            }
            _ => {
                assert!(
                    !has("rustsec") && !has("drift") && !has("ingress-lint"),
                    "{:?}",
                    r.sensors
                );
                for f in ["sensor-rustsec", "sensor-drift", "sensor-ingress-lint"] {
                    assert_eq!(warned(f), 1, "{f} warning once; stderr: {}", r.stderr);
                }
            }
        }
        sizes.push(size);
    }
    let (d, n, l) = (sizes[0], sizes[1], sizes[2]);
    assert!(l < n && n < d, "lean {l} < no-snapshot {n} < default {d}");
    assert!(d - l >= MIN_LEAN_SAVING, "lean saves only {} B", d - l);
    eprintln!(
        "sizes: default {d} B, no-snapshot {n} B (-{} B), lean {l} B (-{} B)",
        d - n,
        d - l
    );
}
