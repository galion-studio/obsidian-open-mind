// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 433, console-studio-names-retire, D-414-1 A):
//! `console.galion.studio` / `dashboard.galion.studio` (NXDOMAIN, never in
//! the tunnel) are no longer console hosts; every live name still gates the
//! same way.
//!
//! Starts the real release binary twice on the box, the way it is run:
//! A. `--ui rust --upstream <closed port>` (gate asks a dead upstream:
//!    remote console hosts fail closed with 503, loopback renders 200);
//! B. `--dist <tmp> --ui rust --admin-store <tmp>` (live console-dist
//!    layout, in-process local-api: remote console hosts get 401 on /api/*
//!    until paired, the pair page on pages).
//!
//! For each config it sends real HTTP requests with a matrix of Host headers
//! (loopback, console.galion.app, another `console.*` / `dashboard.*` name,
//! the two retired names in plain / `www.` / `:443` / trailing-dot / upper
//! case form, and the unknown-host controls galion.app / evil.example) and
//! asserts:
//! - the retired names answer exactly like the unknown-host control
//!   (same status and same body);
//! - live names do NOT answer like the unknown-host control, and loopback /
//!   console.galion.app keep the expected 200 / 503 / 401 codes;
//! - with `CONSOLE_433_BASE_BIN` (the same binary built from base
//!   c2522a99) also started: every live-host row has the same status as on
//!   the base binary, and on the base binary the retired names still
//!   answered like console.galion.app (negative control: the change is real).
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_studio_names_retire -- --nocapture`
//! Ports 23030-23039 (slot 433). No VPS contact, no live change, NO deploy.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const A_NEW: &str = "127.0.0.1:23030";
const B_NEW: &str = "127.0.0.1:23031";
const A_BASE: &str = "127.0.0.1:23032";
const B_BASE: &str = "127.0.0.1:23033";
/// Closed on purpose: the dead upstream of config A.
const DEAD: &str = "127.0.0.1:23039";

const LIVE_HOSTS: &[&str] = &[
    "console.galion.app",
    "www.console.galion.app",
    "console.galion.app:443",
    "console.galion.app.",
    "dashboard.galion.app",
    "console.example.org",
];
const RETIRED: &[&str] = &[
    "console.galion.studio",
    "dashboard.galion.studio",
    "www.console.galion.studio",
    "dashboard.galion.studio:443",
    "console.galion.studio.",
    "Dashboard.Galion.Studio",
];
const UNKNOWN: &str = "galion.app";
const PATHS: &[&str] = &[
    "/",
    "/console",
    "/agents",
    "/api/v1/health",
    "/api/v1/domains",
];

struct Kill(Child, String);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Resp {
    code: u16,
    location: String,
    body: String,
}

fn get(at: &str, host: &str, path: &str) -> Resp {
    let mut s = TcpStream::connect(at).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let location = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("location")
                .then(|| v.trim().to_string())
        })
        .unwrap_or_default();
    Resp {
        code,
        location,
        body: body.to_string(),
    }
}

fn wait_up(at: &str) {
    let t0 = Instant::now();
    while TcpStream::connect(at).is_err() {
        assert!(t0.elapsed() < Duration::from_secs(10), "{at} did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn start(bin: &Path, args: &[String], tag: &str) -> Kill {
    let child = Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("start {tag}: {e}"));
    Kill(child, tag.to_string())
}

fn args_a(bind: &str) -> Vec<String> {
    [
        "--ui",
        "rust",
        "--upstream",
        DEAD,
        "--bind",
        bind,
        "--quiet",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn args_b(bind: &str, tmp: &Path) -> Vec<String> {
    let dist = tmp.join("dist");
    std::fs::create_dir_all(dist.join("static")).unwrap();
    let store = tmp.join(format!("admin-{}.json", bind.rsplit(':').next().unwrap()));
    vec![
        "--dist".into(),
        dist.to_str().unwrap().into(),
        "--ui".into(),
        "rust".into(),
        "--admin-store".into(),
        store.to_str().unwrap().into(),
        "--bind".into(),
        bind.into(),
        "--quiet".into(),
    ]
}

/// One config: probe the matrix, assert, return (host, path, code) rows.
fn check(at: &str, cfg: &str) -> Vec<(String, String, u16)> {
    let mut rows = Vec::new();
    let mut differs: Vec<String> = Vec::new();
    for path in PATHS {
        let unknown = get(at, UNKNOWN, path);
        let evil = get(at, "evil.example", path);
        assert_eq!(unknown.code, evil.code, "{cfg} unknown controls {path}");
        rows.push((UNKNOWN.to_string(), path.to_string(), unknown.code));
        for h in RETIRED {
            let r = get(at, h, path);
            assert_eq!(
                r, unknown,
                "{cfg} retired {h} {path} must answer like unknown host"
            );
            rows.push((h.to_string(), path.to_string(), r.code));
        }
        for h in LIVE_HOSTS.iter().chain(["127.0.0.1"].iter()) {
            let r = get(at, h, path);
            if (r.code, &r.body) != (unknown.code, &unknown.body) {
                differs.push(h.to_string());
            }
            rows.push((h.to_string(), path.to_string(), r.code));
        }
    }
    // Some rows (e.g. /api/* proxied to a dead upstream: 502 for every host)
    // look the same for all hosts; each live name must differ from the
    // unknown-host answer on at least one path.
    for h in LIVE_HOSTS.iter().chain(["127.0.0.1"].iter()) {
        assert!(
            differs.iter().any(|d| d == h),
            "{cfg} live {h} answers like an unknown host on every path"
        );
    }
    rows
}

fn code_of(rows: &[(String, String, u16)], host: &str, path: &str) -> u16 {
    rows.iter()
        .find(|(h, p, _)| h == host && p == path)
        .map(|r| r.2)
        .unwrap_or_else(|| panic!("no row {host} {path}"))
}

#[test]
fn real_studio_names_retire() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_galion-console-static"));
    let tmp = std::env::temp_dir().join(format!("real433-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let _a = start(&bin, &args_a(A_NEW), "A new");
    let _b = start(&bin, &args_b(B_NEW, &tmp), "B new");
    wait_up(A_NEW);
    wait_up(B_NEW);

    let a = check(A_NEW, "A");
    let b = check(B_NEW, "B");

    // Fixed expectations for the live names (as before the change).
    assert_eq!(code_of(&a, "127.0.0.1", "/console"), 200);
    assert_eq!(code_of(&a, "console.galion.app", "/console"), 503);
    assert_eq!(code_of(&a, "console.galion.app.", "/console"), 503);
    assert_eq!(code_of(&b, "console.galion.app", "/api/v1/domains"), 401);
    assert_eq!(code_of(&b, "127.0.0.1", "/api/v1/domains"), 200);
    let lo = get(B_NEW, "127.0.0.1", "/api/v1/domains").body;
    assert!(lo.contains("console.galion.app"), "{lo}");
    assert!(!lo.contains("galion.studio\",\"brand\":\"Ops"), "{lo}");
    for gone in ["console.galion.studio", "dashboard.galion.studio"] {
        assert!(!lo.contains(gone), "{gone} still in /api/v1/domains");
    }
    let svc = get(B_NEW, "127.0.0.1", "/api/v1/services").body;
    assert!(!svc.contains("dashboard.galion.studio"), "{svc}");
    // Root redirect only on console hosts.
    let r = get(A_NEW, "console.galion.app", "/");
    assert!(r.location.ends_with("/console"), "{r:?}");
    let r = get(A_NEW, "console.galion.studio", "/");
    assert!(!r.location.ends_with("/console"), "{r:?}");

    println!("cfg | host | path | new | base");
    let base = std::env::var("CONSOLE_433_BASE_BIN")
        .ok()
        .map(PathBuf::from);
    let (ab, bb) = if let Some(base) = &base {
        let _ab = start(base, &args_a(A_BASE), "A base");
        let _bb = start(base, &args_b(B_BASE, &tmp), "B base");
        wait_up(A_BASE);
        wait_up(B_BASE);
        let mut out = Vec::new();
        for at in [A_BASE, B_BASE] {
            let mut rows = Vec::new();
            for path in PATHS {
                for h in LIVE_HOSTS
                    .iter()
                    .chain(RETIRED.iter())
                    .chain(["127.0.0.1", UNKNOWN].iter())
                {
                    rows.push((h.to_string(), path.to_string(), get(at, h, path).code));
                }
            }
            // Negative control: on base the retired names were console hosts.
            for path in PATHS {
                for h in ["console.galion.studio", "dashboard.galion.studio"] {
                    assert_eq!(
                        get(at, h, path),
                        get(at, "console.galion.app", path),
                        "base {at} {h} {path} was a console host"
                    );
                }
            }
            out.push(rows);
        }
        (Some(out.remove(0)), Some(out.remove(0)))
    } else {
        eprintln!("CONSOLE_433_BASE_BIN unset: base comparison skipped");
        (None, None)
    };
    let mut changed = 0;
    let mut same = 0;
    for (cfg, new, old) in [("A", &a, &ab), ("B", &b, &bb)] {
        for (h, p, c) in new.iter() {
            let o = old.as_ref().map(|o| code_of(o, h, p));
            println!(
                "{cfg} | {h} | {p} | {c} | {}",
                o.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
            );
            if let Some(o) = o {
                let retired = RETIRED.contains(&h.as_str());
                if retired {
                    if o != *c {
                        changed += 1;
                    }
                } else if h != "console.galion.app." {
                    // console.galion.app. (trailing dot) is the one live-name
                    // row allowed to differ: base did not strip the root dot.
                    assert_eq!(o, *c, "{cfg} {h} {p}: live host code changed");
                    same += 1;
                }
            }
        }
    }
    if base.is_some() {
        assert!(changed > 0, "retired rows should differ from base");
        println!("live-host rows identical to base: {same}; retired rows changed: {changed}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
