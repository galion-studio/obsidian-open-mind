// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for slot 297 guardian-tick-wire (D-T24-3 follow-up).
//!
//! guardian-tick keeps its `openmind-nerve` lib file writer
//! (`<state>/nerve.jsonl`, authoritative) and now ALSO mirrors every event
//! to the live nerve through `nerve-client` (no-op unless `NERVE_URL`).
//! It is a one-shot, timer-driven CLI, so it pushes `started` + one
//! `heartbeat` per tick and calls `nerve_client::flush` (max 1.5 s) before
//! exit, so the push worker cannot lose the tail.
//!
//! Stack (override with `REAL_GUARDIAN_WIRE_PORT_BASE`, default 21670):
//!
//! | port  | process |
//! |-------|---------|
//! | 21670 | openmind-nerve serve |
//! | 21671 | in-test healthy HTTP listener (probe `healthy`) |
//! | 21672 | closed port (probe `dies`) |
//! | 21679 | closed port used as a DEAD nerve (tick must still pass, fast) |
//!
//! Gated by `GALION_REAL=1`. Skip the build with `NERVE_WIRING_BIN_DIR`.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let _ = self.0.wait();
    }
}

fn base() -> u16 {
    std::env::var("REAL_GUARDIAN_WIRE_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21670)
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

fn http_get(port: u16, path: &str) -> (u16, String) {
    let addr = format!("127.0.0.1:{port}").parse().unwrap();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn wait_up(port: u16, path: &str) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if http_get(port, path).0 == 200 {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("port {port} path {path} did not come up");
}

fn binaries() -> PathBuf {
    if let Some(d) = std::env::var_os("NERVE_WIRING_BIN_DIR") {
        return PathBuf::from(d);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let td = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("real-guardian-tick-wire");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"])
        .args(["-p", "openmind-nerve", "-p", "guardian-tick"])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build of guardian-tick wire stack failed");
    td.join("release")
}

/// The pushed signals of `guardian-tick` (newest first, at most 5).
fn recent(nerve: u16) -> (u64, Vec<Value>) {
    let (code, body) = http_get(nerve, "/api/v1/signals");
    assert_eq!(code, 200, "signals -> {code} {body}");
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let s = v["sources"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["source"] == "guardian-tick").cloned())
        .unwrap_or(Value::Null);
    (
        s["total"].as_u64().unwrap_or(0),
        s["recent"].as_array().cloned().unwrap_or_default(),
    )
}

fn has(r: &[Value], kind: &str, name: &str) -> bool {
    r.iter().any(|e| e["kind"] == kind && e["name"] == name)
}

fn tick(bin: &Path, nerve_url: Option<&str>, args: &[&str]) -> (Output, Duration) {
    let mut c = Command::new(bin.join("guardian-tick"));
    c.args(args).env_remove("NERVE_URL");
    if let Some(u) = nerve_url {
        c.env("NERVE_URL", u);
    }
    let t0 = Instant::now();
    let o = c.output().expect("run guardian-tick");
    let el = t0.elapsed();
    assert!(
        o.status.success(),
        "guardian-tick {args:?} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    (o, el)
}

#[test]
fn guardian_tick_mirrors_to_live_nerve_and_keeps_file_writer() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life guardian-tick wire test");
        return;
    }
    let bin = binaries();
    let (np, healthy, dead_probe, dead_nerve) = (base(), base() + 1, base() + 2, base() + 9);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work =
        std::env::temp_dir().join(format!("guardian-tick-wire-{}-{now}", std::process::id()));
    std::fs::create_dir_all(work.join("data")).unwrap();
    std::fs::write(work.join("components.json"), "[]").unwrap();

    let _nerve = Proc(
        Command::new(bin.join("openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{np}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", work.join("data"))
            .env("NERVE_COMPONENTS", work.join("components.json"))
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", format!("{np}-{}", np + 9))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn openmind-nerve"),
    );
    wait_up(np, "/healthz");

    let l = TcpListener::bind(("127.0.0.1", healthy)).unwrap();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            let _ = s.write_all(b"HTTP/1.0 200 OK\r\n\r\nok");
        }
    });
    let marker = work.join("restarts.txt");
    let probes = work.join("probes.toml");
    std::fs::write(
        &probes,
        format!(
            "[[probe]]\nkind=\"http\"\nname=\"healthy\"\nport={healthy}\npath=\"/\"\n\
             [[probe]]\nkind=\"http\"\nname=\"dies\"\nport={dead_probe}\npath=\"/\"\n\
             [[repair]]\nprobe=\"dies\"\nunit=\"dummy-dies\"\nafter_fails=1\nbackend=\"command\"\n\
             verify_after_ms=0\ncommand=[\"/bin/sh\",\"-c\",\"echo restart >> {}\"]\n",
            marker.display()
        ),
    )
    .unwrap();
    let url = format!("http://127.0.0.1:{np}");
    let (p, st) = (probes.to_str().unwrap(), work.join("state"));
    let s = st.to_str().unwrap();
    let report = work.join("report.json");
    println!("== {} nerve up on {np}; guardian-tick ticks start", stamp());

    // 1. report mode: started, probes_failed metric, probe_failed error, heartbeat.
    let (_, el) = tick(
        &bin,
        Some(&url),
        &["--probes", p, "--out", report.to_str().unwrap()],
    );
    let (total, r) = recent(np);
    println!(
        "-- report tick {el:?} total={total}: {}",
        Value::from(r.clone())
    );
    assert!(has(&r, "event", "started"), "report: no started");
    assert!(has(&r, "heartbeat", "tick"), "report: no heartbeat");
    assert!(has(&r, "metric", "probes_failed"), "report: no metric");
    assert!(
        r.iter().any(|e| e["kind"] == "error"
            && e["name"] == "probe_failed"
            && e["msg"].as_str().is_some_and(|m| m.starts_with("dies"))),
        "report: no probe_failed for dies"
    );
    let m = r.iter().find(|e| e["name"] == "probes_failed").unwrap();
    assert_eq!(m["value"].as_f64(), Some(1.0));

    // 2. heal dry-run: incident-opened (crit) -> live kind error; file kept.
    let (o, el) = tick(&bin, Some(&url), &["heal", "--probes", p, "--state", s]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("mode=dry-run"));
    let (t2, r) = recent(np);
    println!(
        "-- heal dry-run tick {el:?} total={t2}: {}",
        Value::from(r.clone())
    );
    assert!(
        has(&r, "error", "incident-opened"),
        "heal: no incident-opened"
    );
    assert!(has(&r, "heartbeat", "tick"));
    assert!(!marker.exists(), "dry-run must not run the backend");
    let file = std::fs::read_to_string(st.join("nerve.jsonl")).unwrap();
    assert!(
        file.contains("incident-opened"),
        "file writer must still get it"
    );

    // 3. heal apply (allow-listed, real command backend): heal-restart event.
    let (_, el) = tick(
        &bin,
        Some(&url),
        &["heal", "--probes", p, "--state", s, "--allow", "dummy-dies"],
    );
    let (t3, r) = recent(np);
    println!(
        "-- heal apply tick {el:?} total={t3}: {}",
        Value::from(r.clone())
    );
    assert!(
        r.iter().any(|e| e["name"] == "heal-restart"
            && e["msg"]
                .as_str()
                .is_some_and(|m| m.contains("\"mode\":\"apply\""))),
        "heal: no applied heal-restart"
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1);
    let file = std::fs::read_to_string(st.join("nerve.jsonl")).unwrap();
    assert!(
        file.contains("heal-restart"),
        "file writer must still get it"
    );
    // Every tick's signals arrived (nothing lost on exit).
    assert!(t3 > t2 && t2 > total, "totals must grow: {total} {t2} {t3}");

    // 4. dead nerve: tick still succeeds, file still written, bounded delay.
    let dead_url = format!("http://127.0.0.1:{dead_nerve}");
    let before = std::fs::read_to_string(st.join("nerve.jsonl"))
        .unwrap()
        .len();
    let (_, el) = tick(
        &bin,
        Some(&dead_url),
        &["heal", "--probes", p, "--state", s],
    );
    println!("-- dead-nerve tick {el:?}");
    assert!(
        el < Duration::from_secs(4),
        "dead nerve must not stall a tick: {el:?}"
    );
    let after = std::fs::read_to_string(st.join("nerve.jsonl"))
        .unwrap()
        .len();
    assert!(after >= before);

    // 5. NERVE_URL unset: no push at all (total unchanged), file writer works.
    let (_, el) = tick(&bin, None, &["heal", "--probes", p, "--state", s]);
    let (t5, _) = recent(np);
    println!("-- no-NERVE_URL tick {el:?} total={t5}");
    assert_eq!(t5, t3, "no NERVE_URL must push nothing");

    println!(
        "== {} PASS: guardian-tick mirrors to live nerve, file writer kept (slot 297)",
        stamp()
    );
    let _ = std::fs::remove_dir_all(&work);
}
