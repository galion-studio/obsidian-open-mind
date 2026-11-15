// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test for wave-4 slot 405 (memory-freshness-sensor, D-388-3
//! prep). Gated: `GALION_REAL=1`. Ports 22750-22759 (slot range).
//!
//! Real binaries only: the release `openmind-memory` (server + `freshness`
//! CLI) and the release `openmind-nerve serve` with the staged
//! `memory-freshness` sensor switched on through `NERVE_MEMFRESH_*`.
//!
//! - STALE throwaway store (live layout: data dir + instance `galion` +
//!   legacy root DB; newest row 2026-10-19 14:57:55 +02:00, mtimes pushed
//!   back): the CLI exits 3; the nerve shows a warn `memory-stale` finding,
//!   an open incident with cause `memory-freshness` and a warn alert in
//!   `alerts.json`; no memory content, session keys or paths leak.
//! - One real `/capture` into that same store (real server on :22751): the
//!   next check is fresh, the finding goes and the incident resolves.
//! - FRESH throwaway store from the start: no finding, no incident.
//! - Old binary without the subcommand (`/bin/false` stand-in) and the
//!   sensor switched off: info only / not registered, no alert.
//!
//! Overrides: `OPENMIND_MEMORY_BIN`, `NERVE_BIN` (prebuilt binaries, e.g.
//! static musl builds on the VPS), `REAL_MEMFRESH_BASE_PORT` (22750),
//! `REAL_MEMFRESH_DIR` (scratch root).

use openmind_memory::store::{L0Record, Store};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skip: set GALION_REAL=1");
    }
    on
}

fn base() -> u16 {
    std::env::var("REAL_MEMFRESH_BASE_PORT")
        .ok()
        .map(|v| v.parse().expect("REAL_MEMFRESH_BASE_PORT"))
        .unwrap_or(22750)
}

fn mem_bin() -> PathBuf {
    std::env::var_os("OPENMIND_MEMORY_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_openmind-memory")))
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}

/// The release nerve (built here unless `NERVE_BIN` points at one).
fn nerve_bin() -> PathBuf {
    if let Some(b) = std::env::var_os("NERVE_BIN") {
        return PathBuf::from(b);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let ok = Command::new(cargo)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", target_dir())
        .args([
            "build",
            "-q",
            "-j",
            "2",
            "--release",
            "-p",
            "openmind-nerve",
        ])
        .status()
        .expect("run cargo build")
        .success();
    assert!(ok, "release build of openmind-nerve failed");
    target_dir().join("release/openmind-nerve")
}

struct Kill(Option<Child>);
impl Drop for Kill {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn http(port: u16, method: &str, path: &str, body: Option<&Value>) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let body = body.map(Value::to_string).unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n")?;
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, rest.to_string()))
}

fn clean(cmd: &mut Command) -> &mut Command {
    for k in [
        "OPENMIND_MEMORY_API_KEY",
        "TDAI_GATEWAY_API_KEY",
        "OPENMIND_MEMORY_READ_ONLY",
        "OPENMIND_MEMORY_INSTANCE_ID",
        "TDAI_INSTANCE_ID",
        "TDAI_DATA_DIR",
        "NERVE_URL",
        "NERVE_TELEGRAM_ENV",
        "NERVE_SENSORS_OFF",
        "OPENROUTER_API_KEY",
    ] {
        cmd.env_remove(k);
    }
    for (k, _) in std::env::vars() {
        if k.starts_with("NERVE_") {
            cmd.env_remove(k);
        }
    }
    cmd
}

fn port_free(p: u16) {
    assert!(
        TcpStream::connect(("127.0.0.1", p)).is_err(),
        "port {p} already in use"
    );
}

/// Real memory server on `port` for `dir` (instance galion).
fn memory_server(dir: &Path, port: u16) -> Kill {
    port_free(port);
    let c = clean(&mut Command::new(mem_bin()))
        .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
        .env("OPENMIND_MEMORY_PORT", port.to_string())
        .env("OPENMIND_MEMORY_DATA_DIR", dir)
        .env("OPENMIND_MEMORY_INSTANCE_ID", "galion")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openmind-memory");
    let k = Kill(Some(c));
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if let Some((200, _)) = http(port, "GET", "/health", None) {
            return k;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("openmind-memory on {port} not healthy");
}

fn capture(port: u16, text: &str, session: &str) {
    let ts = now_ms();
    let (code, body) = http(
        port,
        "POST",
        "/capture",
        Some(&json!({
            "user_content": text, "assistant_content": "ok",
            "session_key": session,
            "messages": [
                {"role": "user", "content": text, "timestamp": ts},
                {"role": "assistant", "content": "ok noted", "timestamp": ts + 1}
            ],
        })),
    )
    .expect("capture reply");
    assert_eq!(code, 200, "{body}");
}

fn now_ms() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (epoch ms for the /capture wire timestamp, never printed)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn touch_old(p: &Path, when: &str) {
    if p.exists() {
        let ok = Command::new("touch")
            .args(["-d", when])
            .arg(p)
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }
}

/// A store shaped like the live VPS one, last written 2026-10-19.
fn stale_store(dir: &Path) {
    let inst = dir.join("instances/galion/vectors.db");
    {
        let st = Store::open(&inst, false).unwrap();
        for (i, at) in ["2026-10-18T21:57:27.000Z", "2026-10-19T12:57:55.447Z"] // repo-check: allow (stored UTC rows under test)
            .iter()
            .enumerate()
        {
            st.upsert_l0(&L0Record {
                id: format!("old-{i}"),
                session_key: "stale-secret-session".into(),
                session_id: "stale-secret-session".into(),
                role: "user".into(),
                message_text: "stale private memory text".into(),
                recorded_at: at.to_string(),
                timestamp: 1,
            })
            .unwrap();
        }
        let root = Store::open(&dir.join("vectors.db"), false).unwrap();
        root.upsert_l0(&L0Record {
            id: "legacy-1".into(),
            session_key: "legacy-secret-session".into(),
            session_id: "legacy-secret-session".into(),
            role: "user".into(),
            message_text: "legacy private text".into(),
            recorded_at: "2026-10-13T09:19:53.385Z".into(), // repo-check: allow (stored UTC row under test)
            timestamp: 1,
        })
        .unwrap();
    }
    for p in [
        inst.clone(),
        PathBuf::from(format!("{}-wal", inst.display())),
        PathBuf::from(format!("{}-shm", inst.display())),
    ] {
        touch_old(&p, "2026-10-19 12:57:55 UTC");
    }
    for p in ["vectors.db", "vectors.db-wal", "vectors.db-shm"] {
        touch_old(&dir.join(p), "2026-10-13 09:20:17 UTC");
    }
}

fn freshness_cli(dir: &Path) -> (i32, String) {
    let out = clean(&mut Command::new(mem_bin()))
        .args(["freshness", &dir.display().to_string(), "galion", "24"])
        .output()
        .expect("freshness");
    let t = String::from_utf8_lossy(&out.stdout).into_owned();
    println!(
        "--- freshness CLI (exit {:?}) ---\n{t}---",
        out.status.code()
    );
    (out.status.code().unwrap_or(-1), t)
}

/// A healthy dummy component so the nerve has something real to probe
/// (started once per test process; both tests share it).
fn dummy(port: u16) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| dummy_serve(port));
}

fn dummy_serve(port: u16) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("dummy port free");
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut b = [0u8; 1024];
            let _ = c.read(&mut b);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
}

fn nerve(port: u16, scratch: &Path, extra: &[(&str, String)]) -> Kill {
    port_free(port);
    let data = scratch.join(format!("nerve-{port}"));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let comps = scratch.join(format!("components-{port}.json"));
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"mf-svc","port":{}}}]"#, base() + 9),
    )
    .unwrap();
    let mut cmd = Command::new(nerve_bin());
    clean(&mut cmd)
        .arg("serve")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", &data)
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_EDGE_HOSTS", "0")
        .env("NERVE_LISTEN_SCOPE", format!("{}-{}", base(), base() + 9))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    Kill(Some(cmd.spawn().expect("spawn nerve")))
}

fn state(port: u16) -> Value {
    http(port, "GET", "/api/v1/state", None)
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn bleeding(st: &Value) -> Vec<(String, String)> {
    st["cortex"]["bleeding"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            (
                f["id"].as_str().unwrap_or("").to_string(),
                f["severity"]
                    .as_str()
                    .or(f["sev"].as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect()
}

fn open_causes(st: &Value) -> Vec<String> {
    st["incidents"]["open"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["cause"].as_str().map(str::to_string))
        .collect()
}

fn wait_for(port: u16, what: &str, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state(port);
        if ok(&st) {
            eprintln!(
                "  [{port}] {what}: ok after {} ms (cycle {})",
                t.elapsed().as_millis(),
                st["cycle"]
            );
            return st;
        }
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "[{port}] timed out waiting for {what}: bleeding {:?} incidents {:?}",
            bleeding(&st),
            open_causes(&st)
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn memfresh_env(dir: &Path, bin: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("NERVE_MEMFRESH_BIN", bin.display().to_string()),
        ("NERVE_MEMFRESH_DATA_DIR", dir.display().to_string()),
        ("NERVE_MEMFRESH_INSTANCE", "galion".into()),
        ("NERVE_MEMFRESH_MAX_AGE_H", "24".into()),
        ("NERVE_MEMFRESH_EVERY_S", "2".into()),
        ("NERVE_MEMFRESH_TIMEOUT_MS", "5000".into()),
    ]
}

const SECRETS: [&str; 8] = [
    "stale private",
    "stale-secret",
    "legacy private",
    "legacy-secret",
    "zagreb tram",
    "fresh-live-session",
    "instances/galion",
    "vectors.db",
];

fn no_leak(what: &str, text: &str) {
    for s in SECRETS {
        assert!(!text.contains(s), "{what} leaked {s:?}");
    }
}

#[test]
fn real_memory_freshness_sensor_stale_alert_then_fresh() {
    if !real() {
        return;
    }
    let scratch = std::env::var_os("REAL_MEMFRESH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")))
        .join("real-memfresh-405");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let (stale, fresh) = (scratch.join("stale"), scratch.join("fresh"));
    let p = base();
    let nerve_b = nerve_bin();
    let mem = mem_bin();
    eprintln!(
        "binaries: nerve {} | memory {}",
        nerve_b.display(),
        mem.display()
    );
    dummy(p + 9);

    // Throwaway stores, checked with the real CLI first.
    stale_store(&stale);
    let (code, text) = freshness_cli(&stale);
    assert_eq!(code, 3, "{text}");
    assert!(text.contains("l0_last=2026-10-19T14:57:55+02:00"), "{text}");
    {
        let _srv = memory_server(&fresh, p + 1);
        capture(p + 1, "zagreb tram live", "fresh-live-session");
    }
    port_free(p + 1);
    let (code, text) = freshness_cli(&fresh);
    assert_eq!(code, 0, "{text}");

    // A) stale store: warn finding + incident + alert.
    let n = nerve(p, &scratch, &memfresh_env(&stale, &mem));
    let st = wait_for(p, "memory-stale + incident", 60, |st| {
        bleeding(st).iter().any(|(id, _)| id == "memory-stale")
            && open_causes(st).iter().any(|c| c == "memory-freshness")
    });
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(names.last(), Some(&"memory-freshness"), "{names:?}");
    let f = st["cortex"]["bleeding"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "memory-stale")
        .unwrap()
        .clone();
    eprintln!("  finding: {f}");
    assert_eq!(f["component"], "openmind-memory");
    let title = f["title"].as_str().unwrap_or("");
    assert!(title.contains("not written for"), "{title}");
    let ev = f["evidence"].to_string();
    assert!(
        ev.contains("newest_write=2026-10-19T14:57:55+02:00"),
        "{ev}"
    );
    assert!(ev.contains("instance_l0_rows=2"), "{ev}");
    let body = st.to_string();
    no_leak("/api/v1/state", &body);
    assert!(
        !body.contains(&stale.display().to_string()),
        "data dir path leaked"
    );
    let cycle_ms = st["cycle_ms"].as_u64().unwrap_or(u64::MAX);
    let us = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "memory-freshness")
        .map(|s| s["us"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    eprintln!("  cycle_ms {cycle_ms}, memory-freshness sensor {us} us in cycle");
    assert!(cycle_ms < 50, "cycle budget: {cycle_ms} ms");
    assert!(us < 5_000, "sensor must not block the cycle: {us} us");
    let alerts_p = scratch.join(format!("nerve-{p}/alerts.json"));
    let t = Instant::now();
    let alerts = loop {
        let a = std::fs::read_to_string(&alerts_p).unwrap_or_default();
        if a.contains("memory-freshness") || t.elapsed() > Duration::from_secs(20) {
            break a;
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let arr: Value = serde_json::from_str(&alerts).unwrap_or_default();
    let hit: Vec<&Value> = arr
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["component"] == "memory-freshness")
        .collect();
    eprintln!(
        "  alert: {}",
        hit.first().map(|v| v.to_string()).unwrap_or_default()
    );
    assert!(
        !hit.is_empty(),
        "no memory-freshness alert in alerts.json: {alerts}"
    );
    assert_eq!(hit[0]["severity"], "warn");
    no_leak("alerts.json", &alerts);

    // B) one real capture into the stale store -> fresh, finding gone, resolved.
    {
        let _srv = memory_server(&stale, p + 1);
        capture(p + 1, "zagreb tram live", "fresh-live-session");
    }
    let (code, text) = freshness_cli(&stale);
    assert_eq!(code, 0, "{text}");
    let st = wait_for(p, "memory-stale gone + incident resolved", 60, |st| {
        !bleeding(st).iter().any(|(id, _)| id.starts_with("memory-"))
            && !open_causes(st).iter().any(|c| c == "memory-freshness")
    });
    let resolved = st["incidents"]["resolved"].to_string();
    assert!(resolved.contains("memory-freshness"), "{resolved}");
    no_leak("/api/v1/state after capture", &st.to_string());
    drop(n);

    // C) fresh store from the start: silent over several checks.
    let n = nerve(p + 2, &scratch, &memfresh_env(&fresh, &mem));
    let st = wait_for(p + 2, "3 cycles", 60, |st| {
        st["cycle"].as_u64().unwrap_or(0) >= 3
    });
    assert!(
        !bleeding(&st)
            .iter()
            .any(|(id, _)| id.starts_with("memory-")),
        "{:?}",
        bleeding(&st)
    );
    assert!(!open_causes(&st).iter().any(|c| c == "memory-freshness"));
    drop(n);

    // D) old binary without the subcommand: info only, never an alert.
    let n = nerve(
        p + 3,
        &scratch,
        &memfresh_env(&stale, Path::new("/bin/false")),
    );
    let st = wait_for(p + 3, "memory-freshness-unknown", 60, |st| {
        bleeding(st)
            .iter()
            .any(|(id, _)| id == "memory-freshness-unknown")
    });
    assert!(!open_causes(&st).iter().any(|c| c == "memory-freshness"));
    let a = std::fs::read_to_string(scratch.join(format!("nerve-{}/alerts.json", p + 3)))
        .unwrap_or_default();
    assert!(
        !a.contains("memory-freshness"),
        "unknown must not alert: {a}"
    );
    drop(n);

    // E) sensor off (no NERVE_MEMFRESH_BIN): not registered.
    let n = nerve(p + 4, &scratch, &[]);
    let st = wait_for(p + 4, "1 cycle", 30, |st| {
        st["cycle"].as_u64().unwrap_or(0) >= 1
    });
    let names = st["cortex_sensors"].to_string();
    assert!(!names.contains("memory-freshness"), "{names}");
    drop(n);

    for q in p..p + 5 {
        port_free(q);
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Optional: `GALION_MEMORY_SNAPSHOT_DIR=<dir>` = a throwaway COPY of the
/// live VPS data dir (never the live dir itself). The sensor must call it
/// stale with the live newest write (2026-10-19 14:57:55 +02:00, slot 388)
/// and say nothing but counts and stamps.
#[test]
fn real_memory_freshness_sensor_live_copy() {
    if !real() {
        return;
    }
    let Some(dir) = std::env::var_os("GALION_MEMORY_SNAPSHOT_DIR").map(PathBuf::from) else {
        eprintln!("skip: set GALION_MEMORY_SNAPSHOT_DIR to a copy of the live data dir");
        return;
    };
    assert!(
        !dir.starts_with("/var/lib/memory-tencentdb"),
        "use a copy, never the live data dir"
    );
    let scratch = std::env::var_os("REAL_MEMFRESH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")))
        .join("real-memfresh-405-live");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let p = base() + 5;
    dummy(base() + 9);
    let (code, text) = freshness_cli(&dir);
    assert_eq!(code, 3, "{text}");
    let n = nerve(p, &scratch, &memfresh_env(&dir, &mem_bin()));
    let st = wait_for(p, "memory-stale on the live copy", 60, |st| {
        bleeding(st).iter().any(|(id, _)| id == "memory-stale")
            && open_causes(st).iter().any(|c| c == "memory-freshness")
    });
    let f = st["cortex"]["bleeding"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "memory-stale")
        .unwrap()
        .clone();
    eprintln!("  live-copy finding: {f}");
    let ev = f["evidence"].to_string();
    assert!(
        ev.contains("newest_write=2026-10-19T14:57:55+02:00"),
        "{ev}"
    );
    let body = st.to_string();
    assert!(!body.contains(&dir.display().to_string()), "path leaked");
    assert!(!body.contains("vectors.db"), "file name leaked");
    drop(n);
    port_free(p);
    let _ = std::fs::remove_dir_all(&scratch);
}
