// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL test (GALION_REAL=1): the real `openmind-nerve` binary
//! (GAIA_NERVE_BIN) on 127.0.0.1:19233 and `gaia-dummy` on 127.0.0.1:19234,
//! both slot 23 ports on the box. Kill the dummy, let `gaia heal-tick`
//! restart it three times, each with a fresh hades-gate token (Ed25519,
//! 300 s, single use, scoped to heal-apply + gaia-dummy + plan hash), see
//! no-token, forged and replayed tokens refused, the fourth valid token
//! capped (and not consumed), and read every step back from the nerve.
//! Run with: `GALION_REAL=1 GAIA_NERVE_BIN=/path/openmind-nerve cargo test -p gaia --test real_gaia_live`.

use gaia::live_heal::heal_plan;
use gaia_live::heal::TestTarget;
use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::approval::unix_now;
use hades_gate::token::{plan_hash, Claims};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const NERVE: &str = "127.0.0.1:19233";
const PORT: u16 = 19234;
const ALLOW: &str = "19230-19239";

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

/// Admin side: mint one token with `seed` (in-process; same as
/// `hades-gate mint`) and write it to a new 0600 file.
fn mint(seed: &Path, target: &TestTarget, out: &Path) {
    let key = hades_gate::keys::load_seed(seed).unwrap();
    let now = unix_now();
    let tok = Claims {
        action: "heal-apply".into(),
        target: target.name.clone(),
        plan_hash: plan_hash(&gaia_plan_bytes(&heal_plan(target))),
        nonce: hades_gate::keys::os_random::<16>().unwrap(),
        issued_at: now,
        expires_at: now + 300,
    }
    .sign(&key);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(out)
        .unwrap()
        .write_all(tok.as_bytes())
        .unwrap();
}

#[test]
fn real_kill_detect_plan_signed_approve_restart_cap_and_nerve() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 and GAIA_NERVE_BIN to run the real test");
        return;
    }
    let nerve_bin = std::env::var("GAIA_NERVE_BIN").expect("GAIA_NERVE_BIN");
    let gaia = env!("CARGO_BIN_EXE_gaia");
    let dummy = env!("CARGO_BIN_EXE_gaia-dummy");
    let dir = std::env::temp_dir().join(format!("gaia-hades-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("gate")).unwrap();
    let (seed, public) = (dir.join("admin.seed"), dir.join("gate/admin.pub"));
    hades_gate::keys::generate(&seed, &public).unwrap();
    let evil = dir.join("evil.seed");
    hades_gate::keys::generate(&evil, &dir.join("evil.pub")).unwrap();
    let config = dir.join("gate/hades.toml");
    std::fs::write(
        &config,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            public.display(),
            dir.join("gate/ledger.jsonl").display(),
            dir.join("gate/DISABLED").display()
        ),
    )
    .unwrap();
    let target = TestTarget {
        name: "gaia-dummy".into(),
        port: PORT,
        health_path: "/health".into(),
        argv: vec![dummy.into(), "--port".into(), PORT.to_string()],
    };

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
    wait_port(19233);
    wait_port(PORT);

    let tick = |token: Option<&Path>| {
        let mut cmd = Command::new(gaia);
        cmd.args([
            "heal-tick",
            "--compact",
            "--service",
            "gaia-dummy",
            "--port",
            &PORT.to_string(),
            "--allow-ports",
            ALLOW,
            "--nerve",
            NERVE,
            "--state",
        ])
        .arg(dir.join("state"))
        .arg("--hades-config")
        .arg(&config);
        if let Some(t) = token {
            cmd.arg("--token-file").arg(t);
        }
        let out = cmd
            .args(["--", dummy, "--port", &PORT.to_string()])
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )
    };

    let (c, out) = tick(None);
    assert_eq!(c, 0, "{out}");
    assert!(out.contains("\"outcome\":\"healthy\""), "{out}");

    // Kill the first instance (our own child).
    let mut first = guard.0.remove(1);
    first.kill().unwrap();
    first.wait().unwrap();

    let (c, out) = tick(None);
    assert_eq!(c, 4, "no token: {out}");
    assert!(out.contains("\"code\":\"no-token\""), "{out}");
    mint(&evil, &target, &dir.join("forged.tok"));
    let (c, out) = tick(Some(&dir.join("forged.tok")));
    assert_eq!(c, 4, "forged: {out}");
    assert!(out.contains("\"code\":\"forged\""), "{out}");

    for n in 1..=3 {
        let tok = dir.join(format!("valid-{n}.tok"));
        mint(&seed, &target, &tok);
        let (c, out) = tick(Some(&tok));
        assert_eq!(c, 0, "restart {n}: {out}");
        assert!(out.contains("\"outcome\":\"restarted\""), "{out}");
        assert!(out.contains(&format!("restart {n}/3 this hour")), "{out}");
        let pid = dummy_pid().expect("dummy back up");
        guard.1.push(pid);
        Command::new("kill").arg(pid.to_string()).status().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        if n == 1 {
            let (c, out) = tick(Some(&tok));
            assert_eq!(c, 4, "replay: {out}");
            assert!(out.contains("\"code\":\"replayed\""), "{out}");
        }
    }
    let tok4 = dir.join("valid-4.tok");
    mint(&seed, &target, &tok4);
    let (c, out) = tick(Some(&tok4));
    assert_eq!(c, 5, "fourth restart must be capped: {out}");
    assert!(out.contains("\"outcome\":\"capped\""), "{out}");
    let ledger = std::fs::read_to_string(dir.join("gate/ledger.jsonl")).unwrap();
    assert_eq!(ledger.matches("\"event\":\"consumed\"").count(), 3);

    let ev = get("/events?n=200");
    let v: serde_json::Value = serde_json::from_str(&ev).unwrap();
    let kinds: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    let count = |k: &str| kinds.iter().filter(|x| **x == k).count();
    assert_eq!(count("gaia.heal.applied"), 3, "{kinds:?}");
    assert_eq!(count("gaia.heal.approved"), 3, "{kinds:?}");
    assert_eq!(count("gaia.heal.refused"), 3, "{kinds:?}");
    assert_eq!(count("gaia.heal.capped"), 1, "{kinds:?}");
    for f in ["forged.tok", "valid-1.tok", "valid-4.tok"] {
        let t = std::fs::read_to_string(dir.join(f)).unwrap();
        assert!(!ev.contains(&t), "token must not reach the nerve");
    }
    for e in v.as_array().unwrap() {
        let ts = e["ts"].as_str().unwrap();
        assert!(ts.ends_with("+02:00") || ts.ends_with("+01:00"), "{ts}");
    }
    drop(guard);
    let _ = std::fs::remove_dir_all(&dir);
}
