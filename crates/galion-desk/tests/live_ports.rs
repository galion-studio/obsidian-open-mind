// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Live-listener checks. These used to hard-code the production ports
// 8646 (galion-desk) and 8091 (galion-one) and therefore only passed on the
// VPS. The ports are now configurable so the same checks run against box
// test instances (scripts/real/desk-linkwatch.sh starts them on 19180/19181):
//
//   GALION_DESK_LIVE_DESK_PORT  desk listener to check   (default 8646)
//   GALION_DESK_LIVE_ONE_PORT   one listener to check    (default 8091)
//   GALION_DESK_LIVE_REQUIRED=1 a missing listener FAILS instead of skipping
//
// Every check is read-only: it reads /proc/net/tcp and sends one GET /health
// to the desk port. Nothing binds or signals those ports.

mod support;

use galion_desk::config::{Probes, Settings};
use galion_desk::Desk;
use std::net::TcpListener;
use support::*;

fn port(key: &str, default: u16) -> u16 {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{key}={v} is not a port")),
        _ => default,
    }
}

fn required() -> bool {
    std::env::var("GALION_DESK_LIVE_REQUIRED").is_ok_and(|v| v == "1")
}

/// Inode of the LISTEN socket on `port` (any local address), from /proc.
fn listen_inode(port: u16) -> Option<String> {
    let suffix = format!(":{port:04X}");
    for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 10 && cols[3] == "0A" && cols[1].ends_with(&suffix) && cols[9] != "0" {
                return Some(cols[9].to_string());
            }
        }
    }
    None
}

/// Socket inodes held by this test process.
fn own_socket_inodes() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/proc/self/fd") {
        for entry in dir.flatten() {
            if let Ok(target) = std::fs::read_link(entry.path()) {
                let t = target.to_string_lossy();
                if let Some(inode) = t.strip_prefix("socket:[").and_then(|r| r.strip_suffix(']')) {
                    out.push(inode.to_string());
                }
            }
        }
    }
    out
}

#[test]
fn health_leaves_the_desk_and_one_listeners_alone() {
    let desk_port = port("GALION_DESK_LIVE_DESK_PORT", 8646);
    let one_port = port("GALION_DESK_LIVE_ONE_PORT", 8091);
    let desk_inode = listen_inode(desk_port);
    let one_inode = listen_inode(one_port);
    for (name, p, inode) in [
        ("desk", desk_port, &desk_inode),
        ("one", one_port, &one_inode),
    ] {
        if inode.is_none() {
            assert!(
                !required(),
                "GALION_DESK_LIVE_REQUIRED=1 but nothing listens on {name} port {p}"
            );
            println!("SKIP {name} listener checks: nothing listens on {p}");
        }
    }

    // The running desk (live or box instance) answers the fixed health body.
    if desk_inode.is_some() {
        let resp = get(&format!("127.0.0.1:{desk_port}"), "/health");
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        assert_eq!(body(&resp), "{\"ok\":true}");
    }
    if one_inode.is_some() {
        let resp = get(&format!("127.0.0.1:{one_port}"), "/health");
        assert!(resp.starts_with("HTTP/1.1 200"), "one /health: {resp}");
    }

    // A fresh in-process desk serving /health must not dial anything, spawn
    // the mindpack, or take over either listener.
    let dir = scratch("live-ports");
    let watcher = Fake::start("{}");
    let marker = dir.join("mindpack-ran");
    let mp = dir.join("mp.sh");
    std::fs::write(
        &mp,
        format!("#!/bin/sh\ntouch '{}'\necho '{{}}'\n", marker.display()),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mp, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let a = watcher.addr.clone();
    let settings = Settings {
        bind: "127.0.0.1:0".into(),
        ollama: a.clone(),
        state_dir: dir.join("state"),
        one_dir: dir.join("one"),
        hermes_dir: dir.join("hermes"),
        hermes_bin: mp.clone(),
        probes: Probes {
            hermes: a.clone(),
            one: a.clone(),
            tunnel: a.clone(),
            console: a.clone(),
            origin: a.clone(),
            room: a.clone(),
            tulu: a,
        },
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let desk = Desk::new(settings);
    std::thread::spawn(move || desk.serve(listener));

    for path in ["/health", "/healthz", "/health/x"] {
        assert_eq!(body(&get(&addr, path)), "{\"ok\":true}");
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        watcher.requests().is_empty(),
        "health dialed: {:?}",
        watcher.requests()
    );
    assert!(!marker.exists(), "health ran the mindpack");

    // The instrument works: /desk does dial the probes and run the mindpack.
    let _ = get(&addr, "/desk");
    assert!(watcher.wait_for("GET /api/ps"), "desk did not dial ollama");
    assert!(marker.exists(), "desk did not run the mindpack");

    let ours = own_socket_inodes();
    for (p, before) in [(desk_port, &desk_inode), (one_port, &one_inode)] {
        assert_eq!(&listen_inode(p), before, "listener on {p} changed");
        if let Some(inode) = before {
            assert!(!ours.contains(inode), "this test process holds port {p}");
        }
    }
}
