// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the S2 sensors `proc-owner` (T2.8) and `surface`
//! (T2.9) on the box (gated: `GALION_REAL=1`; run with `--release`).
//! Slot 57: ports 19570-19579. Loopback only, no outbound network.
//!
//! The real binary serves on 127.0.0.1:19570 with
//! `NERVE_PROC_OWNER_USERS=root` (19570 and the two fixture origins may be
//! the box user) and
//! `NERVE_SURFACE_TARGETS` pointing at three local fixture servers:
//! - :19571 a stray listener of this test (box user) -> `stray-owner`;
//! - :19572 a copy of the nerve binary, deleted after start -> `stray-owner`
//!   and `deleted-binary`;
//! - :19573 a fake TLS 1.2 server replaying the real galion.app flight
//!   (fixtures/cortex-s2-c, notAfter 2026-10-31 18:28:08+01:00) ->
//!   `surface-tls-expiring` with `NERVE_SURFACE_WARN_DAYS=60`;
//! - :19574 an HTTP origin answering 503 for `@200` -> `surface-http-status`;
//! - :19575 nothing listening -> `surface-http-down`.
//!
//! Then the strays go away and the proc-owner findings clear.

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    s.write_all(format!("GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").as_bytes())
        .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

fn wait_state(port: u16, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = get(port, "/api/v1/state").unwrap_or_default();
        if ok(&st) || t.elapsed() > Duration::from_secs(secs) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn finding(st: &Value, id: &str) -> Option<Value> {
    ["vulnerabilities", "bleeding"]
        .iter()
        .filter_map(|k| st["cortex"][k].as_array())
        .flatten()
        .find(|f| f["id"] == id)
        .cloned()
}

fn read_head(c: &mut TcpStream) -> Vec<u8> {
    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
    let mut got = vec![];
    let mut b = [0u8; 1024];
    while got.len() < 5 || got.len() < 5 + u16::from_be_bytes([got[3], got[4]]) as usize {
        if got.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        match c.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&b[..n]),
        }
    }
    got
}

fn serve(port: u16, reply: Vec<u8>, sni: Option<&'static str>) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("fixture port free");
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let req = read_head(&mut c);
            if let Some(name) = sni {
                if !req.windows(name.len()).any(|w| w == name.as_bytes()) {
                    continue; // no SNI: drop (the nerve must send it)
                }
            }
            let _ = c.write_all(&reply);
        }
    });
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn my_uid() -> String {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("Uid:"))
        .and_then(|l| l.split_whitespace().nth(2).map(String::from))
        .unwrap()
}

#[test]
fn real_proc_owner_and_surface_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let uid = my_uid();
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-s2-c");
    let dir = std::env::temp_dir().join(format!("nerve-real-s2c-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let comps = dir.join("components.json");
    std::fs::write(&comps, "[]").unwrap();

    // Fixture servers.
    let hex: String = std::fs::read_to_string(fx.join("tls12-flight-galion-app.hex"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect();
    let flight: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    serve(19573, flight, Some("localhost"));
    serve(
        19574,
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec(),
        None,
    );
    let stray = TcpListener::bind("127.0.0.1:19571").expect("19571 free");

    // A copy of the nerve on :19572 whose binary is deleted after start.
    let copy = dir.join("stray-nerve");
    std::fs::copy(BIN, &copy).unwrap();
    let ghost = Kill(
        Command::new(&copy)
            .arg("serve")
            .env_clear()
            .env("NERVE_ADDR", "127.0.0.1:19572")
            .env("NERVE_DATA_DIR", dir.join("data-19572"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_INTERVAL", "60")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ghost"),
    );
    let t = Instant::now();
    while get(19572, "/api/v1/state").is_none() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
    }
    std::fs::remove_file(&copy).unwrap();

    let nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .env("NERVE_ADDR", "127.0.0.1:19570")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data-19570"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "19570-19579")
            .env_remove("NERVE_SENSORS_OFF")
            .env("NERVE_PROC_OWNER_USERS", "root")
            .env(
                "NERVE_PROC_OWNER_PORTS",
                format!("19570={uid},19573={uid},19574={uid}"),
            )
            .env(
                "NERVE_SURFACE_TARGETS",
                "https://localhost:19573,http://localhost:19574/health@200,http://localhost:19575/",
            )
            .env("NERVE_SURFACE_EVERY_S", "5")
            .env("NERVE_SURFACE_TIMEOUT_MS", "1500")
            .env("NERVE_SURFACE_WARN_DAYS", "60")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let ids = [
        "stray-owner",
        "deleted-binary",
        "surface-tls-expiring",
        "surface-http-status",
        "surface-http-down",
    ];
    let st = wait_state(19570, 40, |s| ids.iter().all(|id| finding(s, id).is_some()));
    for id in ids {
        assert!(finding(&st, id).is_some(), "{id} missing: {}", st["cortex"]);
    }
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "listeners",
            "scanners",
            "edge",
            "components",
            "ddos",
            "proc-owner",
            "surface"
        ]
    );
    let so = finding(&st, "stray-owner").unwrap();
    assert_eq!(
        so["title"],
        format!("2 listener(s) run as an unexpected user: :19571 (uid {uid}) :19572 (uid {uid})")
    );
    assert_eq!(
        (so["severity"].as_str(), so["source"].as_str()),
        (Some("warn"), Some("proc-owner"))
    );
    let db = finding(&st, "deleted-binary").unwrap();
    assert_eq!(db["title"], "1 listener(s) run a deleted binary: :19572");
    assert!(db["evidence"][0]
        .as_str()
        .unwrap()
        .contains("stray-nerve (deleted)"));
    let tls = finding(&st, "surface-tls-expiring").unwrap();
    assert!(
        tls["title"]
            .as_str()
            .unwrap()
            .starts_with("1 of 3 public target(s) have a TLS certificate expiring soon: #1 ("),
        "{tls}"
    );
    assert_eq!(
        finding(&st, "surface-http-status").unwrap()["title"],
        "1 of 3 public target(s) answer an unexpected HTTP status: #2 (503, want 200)"
    );
    assert_eq!(
        finding(&st, "surface-http-down").unwrap()["title"],
        "1 of 3 public target(s) do not answer HTTP: #3"
    );
    let events = st["cortex_events"].as_array().unwrap();
    for src in ["proc-owner", "surface"] {
        let ev = events
            .iter()
            .find(|e| e["source"] == src)
            .cloned()
            .unwrap_or_else(|| panic!("{src} event"));
        let ev: NerveEvent = serde_json::from_value(ev).expect("B33 NerveEvent");
        assert!(
            ev.ts.ends_with("+02:00") || ev.ts.ends_with("+01:00"),
            "{}",
            ev.ts
        );
        let txt = serde_json::to_string(&ev).unwrap();
        assert!(
            !txt.contains("localhost") && !txt.contains("stray-nerve") && !txt.contains("/tmp"),
            "{txt}"
        );
        eprintln!("event {txt}");
    }
    let us: Vec<String> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| format!("{}={}", s["name"].as_str().unwrap_or("?"), s["us"]))
        .collect();
    eprintln!(
        "serve :19570 cycle {}: cycle_ms {}, rss {} KiB, sensors us [{}]",
        st["cycle"],
        st["cycle_ms"],
        rss_kib(nerve.0.id()),
        us.join(" ")
    );
    for id in ids {
        let f = finding(&st, id).unwrap();
        eprintln!(
            "finding {id} [{}] {}",
            f["severity"].as_str().unwrap_or("?"),
            f["title"]
        );
    }

    // Strays leave: the proc-owner findings clear, surface ones stay.
    drop(stray);
    drop(ghost);
    let st = wait_state(19570, 20, |s| {
        finding(s, "stray-owner").is_none() && finding(s, "deleted-binary").is_none()
    });
    assert!(finding(&st, "stray-owner").is_none(), "{}", st["cortex"]);
    assert!(finding(&st, "deleted-binary").is_none());
    assert!(finding(&st, "surface-http-status").is_some());
    eprintln!(
        "serve :19570 cycle {}: strays gone, proc-owner findings cleared",
        st["cycle"]
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
