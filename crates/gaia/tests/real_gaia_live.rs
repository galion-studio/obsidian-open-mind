// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL test (GALION_REAL=1): the real `openmind-nerve` binary
//! (GAIA_NERVE_BIN) on 127.0.0.1:19035 and `gaia-dummy` on 127.0.0.1:19036,
//! both slot 03 ports on the box. Kill the dummy, let `gaia heal-tick`
//! restart it three times with a test approval token, see the fourth
//! refused by the cap, and read every step back from the nerve.
//! Run with: `GALION_REAL=1 GAIA_NERVE_BIN=/path/openmind-nerve cargo test -p gaia --test real_gaia_live`.

use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const NERVE: &str = "127.0.0.1:19035";
const PORT: u16 = 19036;

struct Kill(Vec<Child>, Vec<u32>);
impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
        for p in &self.1 {
            let _ = Command::new("kill")
                .arg(p.to_string())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn get(path: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(NERVE).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut b = String::new();
    s.read_to_string(&mut b).unwrap();
    b.split_once("\r\n\r\n").unwrap().1.to_string()
}

fn wait_port(port: u16) {
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("port {port} never opened");
}

fn dummy_pid() -> Option<u32> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", PORT)).ok()?;
    write!(
        s,
        "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut b = String::new();
    s.read_to_string(&mut b).ok()?;
    let v: serde_json::Value = serde_json::from_str(b.split_once("\r\n\r\n")?.1).ok()?;
    v["pid"].as_u64().map(|p| p as u32)
}

#[test]
fn real_kill_detect_plan_approve_restart_cap_and_nerve() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 and GAIA_NERVE_BIN to run the real test");
        return;
    }
    let nerve_bin = std::env::var("GAIA_NERVE_BIN").expect("GAIA_NERVE_BIN");
    let gaia = env!("CARGO_BIN_EXE_gaia");
    let dummy = env!("CARGO_BIN_EXE_gaia-dummy");
    let dir = std::env::temp_dir().join(format!("gaia-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tok = format!(
        "{:016x}{:016x}",
        std::process::id() as u64 * 0x9e37_79b9,
        gaia_core::fnv1a(dir.to_string_lossy().as_bytes())
    );
    let trust = dir.join("trust.token");
    std::fs::write(&trust, &tok).unwrap();
    std::fs::set_permissions(&trust, std::fs::Permissions::from_mode(0o600)).unwrap();
    let approval = dir.join("approval");
    std::fs::write(
        &approval,
        format!("alpha=owner action=heal-apply service=gaia-dummy token={tok}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&approval, std::fs::Permissions::from_mode(0o600)).unwrap();

    let nerve = Command::new(&nerve_bin)
        .args(["serve", "--listen", NERVE, "--log"])
        .arg(dir.join("nerve.jsonl"))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let first = Command::new(dummy)
        .args(["--port", &PORT.to_string()])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut guard = Kill(vec![nerve, first], vec![]);
    wait_port(19035);
    wait_port(PORT);

    let tick = || {
        let out = Command::new(gaia)
            .args([
                "heal-tick",
                "--compact",
                "--service",
                "gaia-dummy",
                "--port",
                &PORT.to_string(),
                "--allow-ports",
                "19030-19039",
                "--nerve",
                NERVE,
                "--state",
            ])
            .arg(dir.join("state"))
            .arg("--trust-file")
            .arg(&trust)
            .arg("--approval-file")
            .arg(&approval)
            .args(["--", dummy, "--port", &PORT.to_string()])
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )
    };

    let (c, out) = tick();
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"outcome\":\"healthy\""), "{out}");

    // Kill the first instance (our own child).
    let mut first = guard.0.remove(1);
    first.kill().unwrap();
    first.wait().unwrap();
    for n in 1..=3 {
        let (c, out) = tick();
        assert_eq!(c, 0, "restart {n}: {out}");
        assert!(out.contains("\"outcome\":\"restarted\""), "{out}");
        assert!(out.contains(&format!("restart {n}/3 this hour")), "{out}");
        let pid = dummy_pid().expect("dummy back up");
        guard.1.push(pid);
        Command::new("kill").arg(pid.to_string()).status().unwrap();
        std::thread::sleep(Duration::from_millis(300));
    }
    let (c, out) = tick();
    assert_eq!(c, 5, "fourth restart must be capped: {out}");
    assert!(out.contains("\"outcome\":\"capped\""), "{out}");

    let ev = get("/events?n=100");
    let v: serde_json::Value = serde_json::from_str(&ev).unwrap();
    let kinds: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds.iter().filter(|k| **k == "gaia.heal.applied").count(),
        3,
        "{kinds:?}"
    );
    assert_eq!(
        kinds.iter().filter(|k| **k == "gaia.heal.capped").count(),
        1,
        "{kinds:?}"
    );
    assert!(!ev.contains(&tok), "token must not reach the nerve");
    for e in v.as_array().unwrap() {
        let ts = e["ts"].as_str().unwrap();
        assert!(ts.ends_with("+02:00") || ts.ends_with("+01:00"), "{ts}");
    }
    drop(guard);
    let _ = std::fs::remove_dir_all(&dir);
}
