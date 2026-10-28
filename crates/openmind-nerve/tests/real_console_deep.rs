// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (gated: `GALION_REAL=1`), wave-4 slot 315, N-307-1 /
//! F-313-2: the nerve called the Rust console healthy because `/` answers 302
//! while `/api/v1/admin` and the board routes are 404. This test starts the
//! real `openmind-nerve serve` (127.0.0.1:21850) against two consoles and
//! checks the deep route probe:
//!
//! - `console-blind` (:21851, deep probes on): must be `degrading` with one
//!   reason per missing route, never hard down (no restart reflex);
//! - `console-blind-nodeep` (same port, no deep list): `healthy`, i.e. the old
//!   blind spot, kept as the control;
//! - `console-full` (:21852, deep probes on): `healthy`, every deep route ok;
//!   then it is killed and must turn `failing` with no deep record.
//!
//! Consoles: the real Rust `galion-console-serve` binaries when
//! `NERVE_DEEP_CONSOLE_BLIND_BIN` (e.g. the live build 2d846fb8) and
//! `NERVE_DEEP_CONSOLE_FULL_BIN` (e.g. 9c0f9d1a with the board API and admin
//! gate) are set, plus `CONSOLE_DIST_STATIC` (a console-dist `static`
//! folder); otherwise in-process stubs answering like them. Ports 21850-21859.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const NERVE: &str = "127.0.0.1:21850";
const BLIND: u16 = 21851;
const FULL: u16 = 21852;
const DEEP: &str = r#"["/api/v1/admin","/api/v1/services"]"#;

struct Kill(Option<Child>);
impl Kill {
    fn stop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
impl Drop for Kill {
    fn drop(&mut self) {
        self.stop();
    }
}

fn get(addr: &str, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect_timeout(&addr.parse().ok()?, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn state() -> serde_json::Value {
    get(NERVE, "/api/v1/state")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn comp(st: &serde_json::Value, n: &str) -> serde_json::Value {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == n).cloned())
        .unwrap_or_default()
}

/// Stub console: `full` answers 200 everywhere; blind answers like the live
/// 2d846fb8 `--local-api` (302 on `/`, 200 health, 404 admin/board).
fn stub(port: u16, full: bool, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("stub port free");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut c, _)) => {
                    let _ = c.set_nonblocking(false);
                    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = [0u8; 2048];
                    let n = c.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let code = match (full, path.as_str()) {
                        (true, _) => "200 OK",
                        (false, "/") => "302 Found",
                        (false, "/api/v1/health") => "200 OK",
                        _ => "404 Not Found",
                    };
                    let _ = write!(
                        c,
                        "HTTP/1.0 {code}\r\nLocation: /console\r\nContent-Length: 3\r\n\r\n{{}}\n"
                    );
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    })
}

fn console(bin: &Path, port: u16, dist: &Path, admin: Option<&Path>) -> Child {
    let mut c = Command::new(bin);
    c.args(["--bind", &format!("127.0.0.1:{port}")])
        .arg("--dist")
        .arg(dist)
        .args(["--ui", "rust", "--local-api"])
        // Live argv has --pulse 8091 / --desk 8646; point them at dead ports here.
        .args([
            "--pulse",
            "127.0.0.1:21858",
            "--desk",
            "127.0.0.1:21859",
            "--quiet",
        ]);
    if let Some(d) = admin {
        c.arg("--admin-store")
            .arg(d.join("admin-device.json"))
            .arg("--admin-mirror")
            .arg(d.join("mirror/admin-device.json"));
    }
    c.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn console")
}

fn wait_up(port: u16) {
    let a = format!("127.0.0.1:{port}");
    let t = Instant::now();
    while get(&a, "/api/v1/health").is_none() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "console {port} not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn real_console_deep_probe() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-deep-315-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("admin/mirror")).unwrap();
    let bins = (
        std::env::var_os("NERVE_DEEP_CONSOLE_BLIND_BIN").map(PathBuf::from),
        std::env::var_os("NERVE_DEEP_CONSOLE_FULL_BIN").map(PathBuf::from),
    );
    let stop_blind = Arc::new(AtomicBool::new(false));
    let stop_full = Arc::new(AtomicBool::new(false));
    let mut threads = vec![];
    let (mut blind, mut full) = (Kill(None), Kill(None));
    let mode = if let (Some(b), Some(f)) = bins {
        let fixture = std::env::var_os("CONSOLE_DIST_STATIC")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                "/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static".into()
            });
        assert!(
            fixture.join("favicon.svg").is_file(),
            "{}",
            fixture.display()
        );
        let dist = dir.join("console-dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::os::unix::fs::symlink(&fixture, dist.join("static")).unwrap();
        blind.0 = Some(console(&b, BLIND, &dist, None));
        full.0 = Some(console(&f, FULL, &dist, Some(&dir.join("admin"))));
        "real galion-console-serve binaries"
    } else {
        threads.push(stub(BLIND, false, stop_blind.clone()));
        threads.push(stub(FULL, true, stop_full.clone()));
        "in-process stub consoles"
    };
    eprintln!("mode: {mode}");
    wait_up(BLIND);
    wait_up(FULL);
    for (p, path) in [
        (BLIND, "/"),
        (BLIND, "/api/v1/admin"),
        (BLIND, "/api/v1/services"),
        (FULL, "/"),
        (FULL, "/api/v1/admin"),
        (FULL, "/api/v1/services"),
    ] {
        let r = get(&format!("127.0.0.1:{p}"), path).map(|x| x.0);
        eprintln!("direct GET :{p}{path} -> {r:?}");
    }
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"console-blind","port":{BLIND},"path":"/","gaia":"HEPHAESTUS","deep":{DEEP}}},
                {{"name":"console-blind-nodeep","port":{BLIND},"path":"/","gaia":"HEPHAESTUS"}},
                {{"name":"console-full","port":{FULL},"path":"/","gaia":"HEPHAESTUS","deep":{DEEP}}}]"#
        ),
    )
    .unwrap();
    let _nerve = Kill(Some(
        Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", NERVE)
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_ROLLBACK", "0")
            .env("NERVE_EVT_ESCALATE", "off")
            .env("NERVE_LISTEN_SCOPE", "21850-21859")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    ));
    // Two samples of each console.
    let t0 = Instant::now();
    let st = loop {
        let st = state();
        if comp(&st, "console-full")["samples"].as_u64().unwrap_or(0) >= 2 {
            break st;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(45),
            "nerve never sampled twice: {st}"
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    let b = comp(&st, "console-blind");
    let nd = comp(&st, "console-blind-nodeep");
    let f = comp(&st, "console-full");
    eprintln!("after {} ms:", t0.elapsed().as_millis());
    for c in [&b, &nd, &f] {
        eprintln!(
            "  {} status={} http={} reasons={} deep={}",
            c["name"], c["status"], c["http"], c["reasons"], c["extra"]["deep"]
        );
    }
    assert_eq!(b["http"], 302, "{b}");
    assert_eq!(b["status"], "degrading", "{b}");
    let reasons: Vec<&str> = b["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r.as_str())
        .collect();
    assert!(
        reasons.contains(&"degrading: deep route /api/v1/admin HTTP 404"),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&"degrading: deep route /api/v1/services HTTP 404"),
        "{reasons:?}"
    );
    assert_eq!(
        b["consecutive_down"], 0,
        "deep failure must not arm the reflex"
    );
    assert_eq!(nd["status"], "healthy", "control (old behaviour) {nd}");
    assert!(nd["extra"]["deep"].is_null());
    assert_eq!(f["status"], "healthy", "{f}");
    let fd = f["extra"]["deep"].as_array().expect("full deep record");
    assert_eq!(fd.len(), 2);
    assert!(fd.iter().all(|d| d["ok"] == true), "{fd:?}");
    // The incident list names the blind console.
    let open = st["incidents"]["open"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    eprintln!("  incidents open: {}", open.len());
    // Kill the full console: health probe down -> failing, no deep probes run.
    full.stop();
    stop_full.store(true, Ordering::Relaxed);
    let t1 = Instant::now();
    let f2 = loop {
        let f2 = comp(&state(), "console-full");
        if f2["status"] == "failing" {
            break f2;
        }
        assert!(
            t1.elapsed() < Duration::from_secs(30),
            "full never failing: {f2}"
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    eprintln!(
        "  after kill {} ms: console-full status={} reasons={} deep={}",
        t1.elapsed().as_millis(),
        f2["status"],
        f2["reasons"],
        f2["extra"]["deep"]
    );
    assert!(
        f2["extra"]["deep"].is_null(),
        "no deep probes when health fails"
    );
    let b2 = comp(&state(), "console-blind");
    assert_eq!(b2["status"], "degrading", "{b2}");
    // Nerve GETs never created an admin store on the full console.
    let made: Vec<_> = std::fs::read_dir(dir.join("admin"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    eprintln!("  admin dir entries after probes: {made:?}");
    blind.stop();
    stop_blind.store(true, Ordering::Relaxed);
    for t in threads {
        let _ = t.join();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
