// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Probes against local fake servers: GET only, no credentials, verdicts.

use gaia_core::System;
use gaia_live::{probe, probe_all, Expect, HttpCheck, Opts, Probe, State};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Serve `n` connections with a fixed response; record the raw requests.
fn fake(resp: &'static str, n: usize) -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().take(n) {
            let mut c = c.unwrap();
            let mut buf = [0u8; 4096];
            let k = c.read(&mut buf).unwrap_or(0);
            s2.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..k]).to_string());
            let _ = c.write_all(resp.as_bytes());
        }
    });
    (port, seen)
}

fn opts() -> Opts {
    Opts {
        systemd: false,
        connect_timeout: Duration::from_millis(500),
        read_timeout: Duration::from_millis(1000),
        ..Default::default()
    }
}

fn hp(name: &str, port: u16, path: &str, expect: Expect) -> Probe {
    Probe {
        name: name.into(),
        system: System::MemoryGateway,
        http: Some(HttpCheck {
            port,
            path: path.into(),
            expect,
        }),
        unit: None,
        fresh_file: None,
    }
}

const JSON_OK: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}";

#[test]
fn json_ok_is_up_and_request_is_a_plain_get() {
    let (port, seen) = fake(JSON_OK, 1);
    let r = probe(&hp("m", port, "/health", Expect::JsonOk), &opts());
    assert_eq!(r.state, State::Up, "{:?}", r.notes);
    let req = seen.lock().unwrap()[0].clone();
    assert!(req.starts_with("GET /health HTTP/1.1\r\n"), "{req}");
    assert!(!req.to_ascii_lowercase().contains("authorization"));
    assert!(req.ends_with("\r\n\r\n"), "no body");
    assert_eq!(gaia_live::GET_ONLY, "GET");
}

#[test]
fn verdicts() {
    let (p1, _) = fake("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok", 1);
    assert_eq!(
        probe(
            &hp("one", p1, "/health", Expect::Body("ok".into())),
            &opts()
        )
        .state,
        State::Up
    );
    let (p2, _) = fake("HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\n\r\nnope", 1);
    assert_eq!(
        probe(&hp("console", p2, "/", Expect::Alive), &opts()).state,
        State::Up
    );
    let (p3, _) = fake("HTTP/1.1 500 Oops\r\nContent-Length: 0\r\n\r\n", 1);
    assert_eq!(
        probe(&hp("x", p3, "/", Expect::Alive), &opts()).state,
        State::Down
    );
    let (p4, _) = fake(
        "HTTP/1.1 200 OK\r\nContent-Length: 21\r\n\r\n{\"status\":\"degraded\"}",
        1,
    );
    let r = probe(&hp("m", p4, "/health", Expect::JsonOk), &opts());
    assert_eq!(r.state, State::Down);
    assert!(r.notes[0].contains("status not ok"));
    let (p5, _) = fake(
        "HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n{\"ok\":true}",
        1,
    );
    assert_eq!(
        probe(&hp("tuner", p5, "/health", Expect::JsonOk), &opts()).state,
        State::Up
    );
    // Nothing listening.
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let r = probe(&hp("gone", closed, "/health", Expect::Status2xx), &opts());
    assert_eq!(r.state, State::Down);
    assert!(r.notes[0].contains("connect"), "{:?}", r.notes);
}

#[test]
fn silent_server_times_out_as_down() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _keep: Vec<_> = l.incoming().take(1).collect();
        std::thread::sleep(Duration::from_secs(5));
    });
    let o = Opts {
        read_timeout: Duration::from_millis(300),
        ..opts()
    };
    let t0 = std::time::Instant::now();
    let r = probe(&hp("slow", port, "/health", Expect::JsonOk), &o);
    assert_eq!(r.state, State::Down);
    assert!(t0.elapsed() < Duration::from_secs(3));
}

#[test]
fn file_freshness_and_degraded_mix() {
    let dir = std::env::temp_dir().join(format!("gaia-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("link.json");
    std::fs::write(&f, "{}").unwrap();
    let fresh = Probe {
        name: "link-watch".into(),
        system: System::LinkWatch,
        http: None,
        unit: None,
        fresh_file: Some((f.clone(), Duration::from_secs(300))),
    };
    let r = probe(&fresh, &opts());
    assert_eq!(r.state, State::Up);
    assert!(r.file_age_s.unwrap() < 5);
    let missing = Probe {
        fresh_file: Some((
            PathBuf::from("/nonexistent/gaia/x.json"),
            Duration::from_secs(1),
        )),
        ..fresh.clone()
    };
    assert_eq!(probe(&missing, &opts()).state, State::Down);
    // HTTP fine, state file missing -> degraded.
    let (port, _) = fake(JSON_OK, 1);
    let mixed = Probe {
        http: Some(HttpCheck {
            port,
            path: "/health".into(),
            expect: Expect::JsonOk,
        }),
        ..missing
    };
    assert_eq!(probe(&mixed, &opts()).state, State::Degraded);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn probe_all_keeps_order_and_default_set_covers_the_stack() {
    let (a, _) = fake(JSON_OK, 1);
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let rs = probe_all(
        &[
            hp("a", a, "/health", Expect::JsonOk),
            hp("b", closed, "/health", Expect::JsonOk),
        ],
        &opts(),
    );
    assert_eq!((rs[0].name.as_str(), rs[0].state.clone()), ("a", State::Up));
    assert_eq!(
        (rs[1].name.as_str(), rs[1].state.clone()),
        ("b", State::Down)
    );
    let d = gaia_live::default_probes();
    let ports: Vec<u16> = d
        .iter()
        .filter_map(|p| p.http.as_ref().map(|h| h.port))
        .collect();
    assert_eq!(ports, vec![8420, 8642, 8650, 8091, 8080]);
    assert!(d.iter().any(|p| p.name == "link-watch"
        && p.fresh_file.is_some()
        && p.unit.as_deref() == Some("link-watch-rs.service")));
    assert_eq!(
        gaia_live::unit_state("bad;name").as_deref(),
        Some("invalid-unit-name")
    );
}
