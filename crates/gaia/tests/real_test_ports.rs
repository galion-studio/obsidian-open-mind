// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 423 (D-383-2 b, F-383-7): real-life test of the `GAIA_TEST_PORTS`
//! override with the real release `gaia` and `gaia-dummy` binaries, gated by
//! `GALION_REAL=1`; run with `--release`. Ports 22930-22939 by default
//! (`GAIA_TEST_PORTS_REAL_PORT_BASE` moves the block; it must stay inside
//! 19000-23999). Nothing here touches a live port, hades-gate or systemd:
//! `heal-plan` only prints, `heal-tick` stops at the scope guard, and the
//! dummy is a loopback child that is killed at the end.
//!
//! The full probe -> nerve -> refused -> healthy loop through the real
//! nerve is `openmind-nerve/tests/real_event_feed.rs` with `GAIA_BIN` set.

use galion_realtest::ports::Block;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

static PORTS: Block = Block::new("GAIA_TEST_PORTS_REAL_PORT_BASE", 22930, 10);

const GAIA: &str = env!("CARGO_BIN_EXE_gaia");
const DUMMY: &str = env!("CARGO_BIN_EXE_gaia-dummy");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Run `gaia` with a clean override environment plus `env`.
fn gaia(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(GAIA);
    c.args(args)
        .env_remove("GAIA_TEST_PORTS")
        .env_remove("GALION_REAL");
    for (k, v) in env {
        c.env(k, v);
    }
    c.stdin(Stdio::null()).output().expect("run gaia")
}

fn plan(port: u16, allow: &str, env: &[(&str, &str)]) -> Output {
    let p = port.to_string();
    gaia(
        &[
            "heal-plan",
            "--compact",
            "--service",
            "t423",
            "--port",
            &p,
            "--allow-ports",
            allow,
            "--",
            "/bin/true",
            &p,
        ],
        env,
    )
}

fn stderr_line(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn get_health(port: u16) -> Option<String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.write_all(b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
        .ok()?;
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    Some(buf)
}

#[test]
fn real_gaia_test_ports_override() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let range = PORTS.range();
    let svc = PORTS.port(1);
    let real = [("GALION_REAL", "1"), ("GAIA_TEST_PORTS", range.as_str())];
    eprintln!("real ports: {} -> 127.0.0.1:{range}", PORTS.var());

    // A. Production default: the slot block is refused (F-383-7 as before).
    let o = plan(svc, &range, &[]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    let e = stderr_line(&o);
    assert!(e.contains("outside the test range 19000-19999"), "{e}");
    eprintln!("A. default: exit 2 | {e}");

    // B. Override without GALION_REAL: refused (never reaches production).
    let o = plan(svc, &range, &[("GAIA_TEST_PORTS", range.as_str())]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    let e = stderr_line(&o);
    assert!(e.contains("needs GALION_REAL=1"), "{e}");
    eprintln!("B. override w/o GALION_REAL: exit 2 | {e}");

    // C. Override with GALION_REAL: accepted, the plan names the block.
    let o = plan(svc, &range, &real);
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).expect("plan JSON");
    let line = v["result"]["plan"][1].as_str().unwrap_or("").to_string();
    assert!(
        line.contains(&format!("box test ports {range} only")),
        "{line}"
    );
    eprintln!(
        "C. override: exit 0 | {line} | sha256 {}",
        v["result"]["plan_sha256"]
    );

    // D. Default-range plan text unchanged (so signed hashes are unchanged).
    let o = plan(19005, "19000-19009", &[]); // repo-check: allow (default-range plan text, t423)
    assert_eq!(o.status.code(), Some(0), "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).expect("plan JSON");
    assert_eq!(
        v["result"]["plan"][1],
        "restart box test service t423 on 127.0.0.1:19005 (box test ports 19000-19999 only)" // repo-check: allow
    );
    eprintln!(
        "D. default plan line unchanged, sha256 {}",
        v["result"]["plan_sha256"]
    );

    // E. Out-of-bounds overrides and live ports stay refused.
    for (ov, port, allow) in [
        ("24000-24009", 24001u16, "24000-24009"),
        ("18990-19009", 19001, "19000-19009"), // repo-check: allow (bounds edge)
        ("8000-8999", 8670, "8670"),
        ("1-65535", 8080, "8080"),
    ] {
        let o = plan(
            port,
            allow,
            &[("GALION_REAL", "1"), ("GAIA_TEST_PORTS", ov)],
        );
        assert_eq!(o.status.code(), Some(2), "{ov}: {o:?}");
        let e = stderr_line(&o);
        assert!(e.contains("override bounds 19000-23999"), "{ov}: {e}");
        eprintln!("E. GAIA_TEST_PORTS={ov}: exit 2 | {e}");
    }
    let o = plan(
        8670,
        "8670",
        &[("GALION_REAL", "1"), ("GAIA_TEST_PORTS", "19000-23999")],
    );
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    eprintln!(
        "E. live nerve 8670 under 19000-23999: exit 2 | {}",
        stderr_line(&o)
    );

    // F. heal-tick without the override stops at the scope guard (exit 4).
    let dir = std::env::temp_dir().join(format!("gaia-real-test-ports-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let state = dir.join("state");
    let hades = dir.join("no-hades.toml");
    let nerve = PORTS.addr(9);
    let p = svc.to_string();
    let tick = |env: &[(&str, &str)]| {
        gaia(
            &[
                "heal-tick",
                "--compact",
                "--service",
                "t423",
                "--port",
                &p,
                "--allow-ports",
                &range,
                "--nerve",
                nerve,
                "--state",
                &state.to_string_lossy(),
                "--hades-config",
                &hades.to_string_lossy(),
                "--wait-ms",
                "200",
                "--",
                "/bin/true",
                &p,
            ],
            env,
        )
    };
    let o = tick(&[]);
    assert_eq!(o.status.code(), Some(4), "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).expect("tick JSON");
    assert_eq!(v["result"]["outcome"], "refused");
    assert_eq!(v["result"]["steps"][0]["step"], "scope");
    assert_eq!(v["result"]["steps"][0]["detail"]["ok"], false);
    eprintln!("F. heal-tick default: exit 4 | {}", v["result"]["detail"]);
    // With the override the scope step passes; no nerve listens on the
    // block's last port, so the tick ends as a setup error, never a restart.
    let o = tick(&real);
    let out = String::from_utf8_lossy(&o.stdout);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("is not healthy") || out.contains("not healthy"),
        "scope passed, stopped at nerve health: {o:?}"
    );
    assert!(!state.join("restarts.jsonl").exists(), "nothing restarted");
    eprintln!(
        "F. heal-tick override: exit {:?} | {}",
        o.status.code(),
        err.lines().next().unwrap_or("").trim()
    );

    // G. gaia-dummy: refused on the block by default, serves with override.
    let ds = PORTS.port(2);
    let o = Command::new(DUMMY)
        .args(["--port", &ds.to_string()])
        .env_remove("GAIA_TEST_PORTS")
        .env_remove("GALION_REAL")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    eprintln!("G. gaia-dummy :{ds} default: exit 2 | {}", stderr_line(&o));
    let d = Kill(
        Command::new(DUMMY)
            .args(["--port", &ds.to_string()])
            .env("GALION_REAL", "1")
            .env("GAIA_TEST_PORTS", &range)
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let t0 = Instant::now();
    let body = loop {
        if let Some(b) = get_health(ds) {
            break b;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "dummy never answered"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        body.starts_with("HTTP/1.") && body.contains("\"status\":\"ok\""),
        "{body}"
    );
    eprintln!(
        "G. gaia-dummy :{ds} override: {}",
        body.lines().last().unwrap_or("")
    );
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}
