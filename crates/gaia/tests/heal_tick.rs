// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Hermetic checks of `gaia heal-tick`'s refusal paths against an in-process
//! fake nerve on an ephemeral loopback port. Nothing is started or restarted
//! here: every case must stop before the apply step. The full apply path is
//! in `real_gaia_live.rs` (GALION_REAL=1) and scripts/real/gaia-live.sh.

use gaia::live_heal::{exit, heal_tick, parse_cli, HealCfg};
use gaia_live::heal::TestTarget;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Log = Arc<Mutex<Vec<Value>>>;

/// A minimal nerve: GET /health, POST /event, GET /events.
fn fake_nerve() -> (SocketAddr, Log) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let log: Log = Arc::default();
    let log2 = log.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut first = String::new();
            r.read_line(&mut first).unwrap();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let resp = if first.starts_with("GET /health") {
                r#"{"status":"ok"}"#.to_string()
            } else if first.starts_with("POST /event") {
                let mut v: Value = serde_json::from_slice(&body).unwrap();
                v["ts"] = Value::from("2026-10-20T19:00:00+02:00");
                log2.lock().unwrap().push(v);
                r#"{"ok":true}"#.to_string()
            } else {
                Value::Array(log2.lock().unwrap().clone()).to_string()
            };
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                resp.len()
            );
        }
    });
    (addr, log)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gaia-heal-tick-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn prog() -> String {
    ["/bin/true", "/usr/bin/true"]
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap()
        .to_string()
}

const TOK: &str = "00112233445566778899aabbccddeeff";

/// A config whose target port is free (bound and released), inside a range
/// that is only that port, so nothing answers the probe.
fn cfg(tag: &str, nerve: SocketAddr, port: u16) -> HealCfg {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp(tag);
    let trust = d.join("trust.token");
    std::fs::write(&trust, TOK).unwrap();
    std::fs::set_permissions(&trust, std::fs::Permissions::from_mode(0o600)).unwrap();
    HealCfg {
        target: TestTarget {
            name: "gaia-dummy".into(),
            port,
            health_path: "/health".into(),
            argv: vec![prog(), "--port".into(), port.to_string()],
        },
        allowed: 19000..=19999,
        nerve,
        state_dir: d.join("state"),
        trust_file: trust,
        approval_file: None,
        wait: Duration::from_millis(200),
    }
}

/// A port in 19000-19999 with nothing listening (slot 03's range first).
fn free_test_port() -> u16 {
    (19037..=19039)
        .chain(19900..=19999)
        .find(|p| {
            TcpListener::bind(("127.0.0.1", *p)).is_ok()
                && std::net::TcpStream::connect(("127.0.0.1", *p)).is_err()
        })
        .expect("a free test port")
}

fn kinds(log: &Log) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_string())
        .collect()
}

#[test]
fn live_stack_port_is_refused_before_any_probe() {
    let (nerve, log) = fake_nerve();
    let mut c = cfg("scope", nerve, 8420);
    c.target.argv = vec![prog(), "--port".into(), "8420".into()];
    let t = heal_tick(&c).unwrap();
    assert_eq!(t.exit, exit::REFUSED);
    assert_eq!(kinds(&log), vec!["gaia.heal.refused"]);
    let e = &log.lock().unwrap()[0];
    assert_eq!(e["data"]["stage"], "scope");
    assert!(!c.state_dir.exists(), "nothing written for a refused scope");
}

#[test]
fn down_service_without_approval_is_planned_but_refused() {
    let (nerve, log) = fake_nerve();
    let c = cfg("noapproval", nerve, free_test_port());
    let t = heal_tick(&c).unwrap();
    assert_eq!(t.exit, exit::REFUSED, "{}", t.report.pretty());
    assert_eq!(kinds(&log), vec!["service.probe", "gaia.heal.refused"]);
    let text = t.report.compact();
    assert!(text.contains("\"step\":\"nerve-in\""), "{text}");
    assert!(text.contains("\"step\":\"hades-plan\""), "{text}");
    assert!(text.contains("\"outcome\":\"refused\""), "{text}");
    assert!(!c.state_dir.join("restarts.jsonl").exists());
}

#[test]
fn wrong_token_or_unknown_alpha_is_refused_and_token_never_logged() {
    for (tag, line) in [
        (
            "wrongtok",
            format!(
                "alpha=owner action=heal-apply service=gaia-dummy token={}",
                "a".repeat(32)
            ),
        ),
        (
            "stranger",
            format!("alpha=stranger action=heal-apply service=gaia-dummy token={TOK}"),
        ),
        (
            "kill",
            format!("alpha=owner action=kill service=gaia-dummy token={TOK}"),
        ),
    ] {
        let (nerve, log) = fake_nerve();
        let mut c = cfg(tag, nerve, free_test_port());
        let ap = c.state_dir.parent().unwrap().join("approval");
        std::fs::write(&ap, line).unwrap();
        c.approval_file = Some(ap);
        let t = heal_tick(&c).unwrap();
        assert_eq!(t.exit, exit::REFUSED, "{tag}");
        assert_eq!(
            kinds(&log),
            vec!["service.probe", "gaia.heal.refused"],
            "{tag}"
        );
        let all = format!("{} {:?}", t.report.compact(), log.lock().unwrap());
        assert!(!all.contains(TOK), "{tag}: trusted token leaked");
        assert!(!c.state_dir.join("restarts.jsonl").exists(), "{tag}");
    }
}

#[test]
fn cli_parse_keeps_start_argv_separate() {
    let a = [
        "--service",
        "gaia-dummy",
        "--port",
        "19032",
        "--allow-ports",
        "19030-19039",
        "--nerve",
        "127.0.0.1:19031",
        "--state",
        "/tmp/s",
        "--trust-file",
        "/tmp/t",
        "--",
        "/x/gaia-dummy",
        "--port",
        "19032",
    ];
    let c = parse_cli(&a).unwrap();
    assert_eq!(c.target.port, 19032);
    assert_eq!(c.target.argv, vec!["/x/gaia-dummy", "--port", "19032"]);
    assert_eq!(c.allowed, 19030..=19039);
    assert!(parse_cli(&a[..12]).is_err(), "needs --");
}
