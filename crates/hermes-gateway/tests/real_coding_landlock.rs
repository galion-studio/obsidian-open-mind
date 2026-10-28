// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 334 (coding-tools-landlock-stage), F-320-2 end to end: start the
//! real `hermes-gateway` binary with the STAGED coding-tools drop-in alone
//! and with each staged D-320-1 alternative
//! (`deploy/staged/hermes-rs.service.d/landlock-alternatives/*.conf`), and
//! record what the gateway does at start on this kernel: exit 2 ("coding:
//! refusing to start") or `/health` 200 with "coding: tools on".
//!
//! The gateway's preflight probes the kernel and the tools would run the
//! sandbox, so this only runs with `GALION_REAL=1 GALION_SANDBOX_ON_VPS=1`
//! (sandbox_vps_only: VPS, non-root, /var/tmp/sandbox-test-<ts>). Paths are
//! remapped into a throwaway dir; the delegated cgroup is dropped (a plain
//! user has none). No model call is made, no secret is read: HERMES_HOME is
//! an empty temp dir and the API key is a fixed test string.
//! Env: `HCL_BIN` (gateway binary), `HCL_REPO` (dir with deploy/staged),
//! `HCL_PORT` (first port, default 22040; slot 334 owns 22040-22049).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "slot334-test-key-not-a-secret";
const ALT_DIR: &str = "deploy/staged/hermes-rs.service.d/landlock-alternatives";

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1")
        && std::env::var("GALION_SANDBOX_ON_VPS").as_deref() == Ok("1");
    if !on {
        eprintln!("SKIP: real_coding_landlock needs GALION_REAL=1 GALION_SANDBOX_ON_VPS=1 (sandbox_vps_only)");
    }
    on
}

fn repo() -> PathBuf {
    std::env::var_os("HCL_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn bin() -> PathBuf {
    std::env::var_os("HCL_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn port(i: u16) -> u16 {
    std::env::var("HCL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22040)
        + i
}

fn env_lines(rel: &str) -> Vec<(String, String)> {
    std::fs::read_to_string(repo().join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Environment="))
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.trim_matches('"').to_string()))
        .collect()
}

fn health(p: u16) -> u16 {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", p)) else {
        return 0;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// Start the gateway with the staged env (+ `alt`); returns (exit code if it
/// exited, /health code, stderr).
fn start(d: &Path, i: u16, alt: Option<&str>) -> (Option<i32>, u16, String) {
    let run = d.join(format!("run{i}"));
    for sub in ["wt/.git", "toolchain/bin", "home", "state"] {
        std::fs::create_dir_all(run.join(sub)).unwrap();
    }
    std::fs::write(run.join("toolchain/bin/cargo"), b"#!/bin/sh\n").unwrap();
    let mut env = env_lines("deploy/staged/hermes-rs.service.d/coding-tools.conf");
    if let Some(f) = alt {
        env.extend(env_lines(&format!("{ALT_DIR}/{f}")));
    }
    let p = port(i);
    let errf = run.join("stderr.log");
    let mut c = Command::new(bin());
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("API_SERVER_KEY", KEY)
        .env("HERMES_HOME", run.join("home"))
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", p.to_string())
        .env("HERMES_RS_STATE_DB", run.join("rs-state.db"))
        .env("HERMES_RS_SPEND_FILE", run.join("rs-spend.json"))
        .env("HERMES_RS_CRON_STATE", run.join("rs-cron-state.json"))
        .env("HERMES_RS_CRON_OUTPUT", run.join("rs-cron-output"))
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_SESSION_USD_CAP", "1.00")
        .env("HERMES_RS_DAILY_USD_CAP", "2.00")
        .env("HERMES_RS_DAILY_CALL_CAP", "500")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&errf).unwrap());
    for (k, v) in &env {
        c.env(k, v);
    }
    let remap = [
        ("HERMES_RS_CODING_WORKTREE", run.join("wt")),
        ("HERMES_RS_CODING_STATE", run.join("state")),
        ("HERMES_RS_CODING_TOOLCHAIN", run.join("toolchain")),
    ];
    for (k, v) in remap {
        c.env(k, v);
    }
    c.env_remove("HERMES_RS_CODING_CGROUP")
        .env("HERMES_RS_CODING_REQUIRE_CGROUP", "0");
    let mut child = c.spawn().expect("spawn hermes-gateway");
    let t = Instant::now();
    let (mut code, mut h) = (None, 0);
    while t.elapsed() < Duration::from_secs(10) {
        if let Ok(Some(st)) = child.try_wait() {
            code = st.code();
            break;
        }
        h = health(p);
        if h == 200 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Always reap (kill is a no-op on an already exited child).
    let _ = child.kill();
    let _ = child.wait();
    let err = std::fs::read_to_string(&errf).unwrap_or_default();
    (code, h, err)
}

#[test]
fn real_gateway_start_with_staged_landlock_alternatives() {
    if !real() {
        return;
    }
    let base = std::env::var_os("SANDBOX_TEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let d = base.join(format!("hcl334-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let cases: [(&str, Option<&str>); 4] = [
        ("base", None),
        ("a", Some("landlock-a-allow.conf")),
        ("b", Some("landlock-b-refuse.conf")),
        ("c", Some("landlock-c-layered.conf")),
    ];
    let mut results = vec![];
    for (i, (name, alt)) in cases.iter().enumerate() {
        let t = Instant::now();
        let (code, h, err) = start(&d, i as u16, *alt);
        let coding: Vec<&str> = err.lines().filter(|l| l.contains("coding:")).collect();
        eprintln!(
            "CASE {name}: exit={code:?} health={h} in {} ms\n  {}",
            t.elapsed().as_millis(),
            coding.join("\n  ")
        );
        assert!(
            !port_open(port(i as u16)),
            "{name}: port {} still open after kill",
            port(i as u16)
        );
        results.push((*name, code, h, err));
    }
    // What is asserted is only consistency with the kernel: refuse => exit 2,
    // start => tools on. The kernel facts decide which one each case gets.
    let landlock = results
        .iter()
        .any(|(_, _, _, e)| e.contains("landlock_abi=") && !e.contains("landlock_abi=0"));
    for (name, code, h, err) in &results {
        let refuses = !landlock && matches!(*name, "base" | "b");
        if refuses {
            assert_eq!(*code, Some(2), "{name}: {err}");
            assert!(
                err.contains("coding: refusing to start: the kernel has no Landlock"),
                "{name}: {err}"
            );
        } else {
            assert_eq!(*h, 200, "{name}: exit {code:?}: {err}");
            assert!(err.contains("coding: tools on"), "{name}: {err}");
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

fn port_open(p: u16) -> bool {
    TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], p)),
        Duration::from_millis(200),
    )
    .is_ok()
}
