// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 333 (nerve-merge-6, D-316-1 option A), real-life test gated with
//! `GALION_REAL=1`. Real `openmind-nerve` binary (release with `--release`),
//! ports 22030-22039 by default (REAL_NERVE_PORT_BASE remaps).
//!
//! 1. `replay` of the two live VPS listener tables (read-only `ss -ltnp`
//!    2026-10-27 22:26 and 2026-10-28 01:16 +01:00) with the LIVE component
//!    file (`/etc/galion-nerve/components-live.json`, copied read-only 01:17)
//!    reports `unmanaged_ports [18080]`, exactly like the running live nerve
//!    9d1a4f3; with the STAGED file `deploy/staged/nerve/components-live.json`
//!    it reports none and every other port keeps its owner.
//! 2. `serve --mode event` with the staged `console` + `console-rust-primary`
//!    entries moved into the block, against an HTTP stand-in answering like the
//!    live galion-console-serve 2d846fb8 on :8080 / :18080 at 01:14 +01:00
//!    (`/` 302, `/api/v1/admin` 404, `/api/v1/services` 404): the primary is
//!    monitored (degrading on the two deep routes, never hard down, not
//!    unmanaged), and the same listener is unmanaged again with the live table.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const fn port_block(default: u16) -> u16 {
    match option_env!("REAL_NERVE_PORT_BASE") {
        None => default,
        Some(s) => {
            let b = s.as_bytes();
            let mut i = 0;
            let mut v: u32 = 0;
            assert!(!b.is_empty(), "REAL_NERVE_PORT_BASE empty");
            while i < b.len() {
                assert!(b[i].is_ascii_digit(), "REAL_NERVE_PORT_BASE not a number");
                v = v * 10 + (b[i] - b'0') as u32;
                assert!(v <= 65_526, "REAL_NERVE_PORT_BASE too large");
                i += 1;
            }
            v as u16
        }
    }
}

const BLOCK: u16 = port_block(22030);
const NERVE_PORT: u16 = BLOCK;
/// Stand-in for galion-console-serve 2d846fb8 (staged `console-rust-primary`).
const PRIMARY: u16 = BLOCK + 3;
/// Stand-in for the :8080 console (staged `console`).
const CONSOLE: u16 = BLOCK + 4;

const LIVE_COMPONENTS: &str = "tests/fixtures/own-services/components-live-vps-20261028-0117.json";
const STAGED_COMPONENTS: &str = "../../deploy/staged/nerve/components-live.json";

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skipped: set GALION_REAL=1");
    }
    on
}

fn path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-333-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ports(v: &serde_json::Value) -> Vec<u64> {
    v.as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|p| p.as_u64())
        .collect()
}

fn owners(ex: &serde_json::Value) -> Vec<(u64, String)> {
    let mut v: Vec<_> = ex["ownership"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["port"].as_u64().unwrap(),
                o["owner"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    v.sort();
    v
}

fn replay(ss_fixture: &str, ts: &str, components: &Path) -> serde_json::Value {
    let d = tmp("replay");
    std::fs::copy(
        path(&format!("tests/fixtures/own-services/{ss_fixture}")),
        d.join("ss.txt"),
    )
    .unwrap();
    std::fs::write(
        d.join("state-1.json"),
        format!(r#"{{"ts":"{ts}""#) + r#","components":[{"name":"lxc-host","status":"healthy","host":{"disk_total_kb":100,"disk_avail_kb":75,"ram_total_kb":100,"ram_avail_kb":90,"load1":0.5}}],"probe_errors":[]}"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
        .arg("replay")
        .arg(&d)
        .arg("--state")
        .env("NERVE_DATA_DIR", d.join("data"))
        .env("NERVE_COMPONENTS", components)
        .stderr(Stdio::null())
        .output()
        .expect("run replay");
    let _ = std::fs::remove_dir_all(&d);
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("replay json: {e}: {}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn real_staged_components_own_18080() {
    if !real() {
        return;
    }
    for (ss, ts) in [
        ("ss-ltnp-vps-20261027-2226.txt", "2026-10-27T22:26:10+01:00"),
        ("ss-ltnp-vps-20261028-0116.txt", "2026-10-28T01:16:38+01:00"),
    ] {
        let lv = replay(ss, ts, &path(LIVE_COMPONENTS));
        let lex = &lv["cortex"]["exposure"];
        assert_eq!(
            ports(&lex["unmanaged_ports"]),
            [18080],
            "live table {ss}: {lex}"
        );
        let st = replay(ss, ts, &path(STAGED_COMPONENTS));
        let sex = &st["cortex"]["exposure"];
        assert_eq!(
            ports(&sex["unmanaged_ports"]),
            Vec::<u64>::new(),
            "staged {ss}: {sex}"
        );
        assert!(
            !st["cortex"]["vulnerabilities"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .any(|f| f["id"] == "unmanaged-listeners"),
            "staged {ss}: unmanaged finding still raised"
        );
        // Same owners for every listener except 18080.
        let (lo, so) = (owners(lex), owners(sex));
        assert_eq!(lo.len(), so.len());
        for (l, s) in lo.iter().zip(&so) {
            if l.0 == 18080 {
                assert_eq!(s.1, "console-rust-primary", "{s:?}");
            } else {
                assert_eq!(l, s);
            }
        }
        eprintln!(
            "333 replay {ss}: live unmanaged {:?} -> staged unmanaged {:?}; {} listeners, 18080 owner {:?}",
            lex["unmanaged_ports"],
            sex["unmanaged_ports"],
            so.len(),
            so.iter().find(|o| o.0 == 18080).map(|o| &o.1)
        );
    }
}

/// HTTP stand-in answering like live galion-console-serve 2d846fb8.
fn console_stub(port: u16, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<u32> {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("stub port free");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut hits = 0;
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut s, _)) => {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut line = String::new();
                    let _ = BufReader::new(&s).read_line(&mut line);
                    let p = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    hits += 1;
                    let r = if p == "/" {
                        "HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    } else {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found"
                    };
                    let _ = s.write_all(r.as_bytes());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        hits
    })
}

fn get_state() -> Option<serde_json::Value> {
    let mut s = TcpStream::connect_timeout(
        &format!("127.0.0.1:{NERVE_PORT}").parse().ok()?,
        Duration::from_secs(2),
    )
    .ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET /api/v1/state HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    if h.split_whitespace().nth(1)? != "200" {
        return None;
    }
    serde_json::from_str(body).ok()
}

/// The staged (or live) table, reduced to the two console entries and moved
/// into the block (18080 -> PRIMARY, 8080 -> CONSOLE).
fn block_table(src: &str, dir: &Path) -> PathBuf {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path(src)).unwrap()).unwrap();
    let out: Vec<serde_json::Value> = v
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["name"] == "console" || c["name"] == "console-rust-primary")
        .map(|c| {
            let mut c = c.clone();
            c["port"] = match c["port"].as_u64() {
                Some(18080) => PRIMARY.into(),
                Some(8080) => CONSOLE.into(),
                o => panic!("unexpected console port {o:?}"),
            };
            c
        })
        .collect();
    let f = dir.join("components.json");
    std::fs::write(&f, serde_json::to_string(&out).unwrap()).unwrap();
    f
}

fn serve(
    src: &str,
    tag: &str,
    event: bool,
    done: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let dir = tmp(tag);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"));
    cmd.arg("serve");
    if event {
        // The live mode; without events it cycles once at start (heartbeat >= 60 s).
        cmd.args(["--mode", "event"]);
    }
    let comps = block_table(src, &dir);
    let _n = Kill(
        cmd.env("NERVE_ADDR", format!("127.0.0.1:{NERVE_PORT}"))
            .env("NERVE_INTERVAL", "3")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_ROLLBACK", "0")
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_EVT_ESCALATE", "off")
            .env("NERVE_LISTEN_SCOPE", format!("{BLOCK}-{}", BLOCK + 9))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    let mut last = serde_json::Value::Null;
    while t.elapsed() < Duration::from_secs(40) {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(st) = get_state() {
            last = st;
            if done(&last) {
                break;
            }
        }
    }
    eprintln!(
        "333 serve {tag}: {:.1}s cycle {} samples {:?} listeners_judged {}",
        t.elapsed().as_secs_f64(),
        last["cycle"],
        last["components"].as_array().map(|a| a
            .iter()
            .map(|c| (c["name"].clone(), c["samples"].clone()))
            .collect::<Vec<_>>()),
        last["cortex"]["exposure"]["listeners_judged"]
    );
    let _ = std::fs::remove_dir_all(&dir);
    last
}

fn comp<'a>(st: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == name))
        .unwrap_or(&serde_json::Value::Null)
}

#[test]
fn real_serve_staged_console_rust_primary() {
    if !real() {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let h1 = console_stub(PRIMARY, stop.clone());
    let h2 = console_stub(CONSOLE, stop.clone());
    // Legacy polling (2 samples) and the live `--mode event` (start-up cycle).
    for (event, want) in [(false, 2u64), (true, 1u64)] {
        let tag = if event { "event" } else { "legacy" };
        let samples =
            |st: &serde_json::Value, n: &str| comp(st, n)["samples"].as_u64().unwrap_or(0);
        let st = serve(STAGED_COMPONENTS, &format!("staged-{tag}"), event, |st| {
            samples(st, "console-rust-primary") >= want && samples(st, "console") >= want
        });
        let p = comp(&st, "console-rust-primary");
        assert!(samples(&st, "console-rust-primary") >= want, "{tag}: {p}");
        assert_eq!(p["status"], "degrading", "{tag}: {p}");
        assert_eq!(p["http"], 302, "{tag}: {p}");
        let reasons: Vec<&str> = p["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.as_str())
            .collect();
        for r in [
            "degrading: deep route /api/v1/admin HTTP 404",
            "degrading: deep route /api/v1/services HTTP 404",
        ] {
            assert!(reasons.contains(&r), "{tag}: {p}");
        }
        assert_eq!(
            p["consecutive_down"], 0,
            "{tag}: deep 404s never arm the reflex: {p}"
        );
        assert_eq!(p["reflex_restarts_per_hour"], 0, "{tag}: {p}");
        assert_eq!(
            comp(&st, "console")["status"],
            "degrading",
            "{tag}: staged console gets deep too"
        );
        let ex = &st["cortex"]["exposure"];
        let un = ports(&ex["unmanaged_ports"]);
        assert!(
            ex["listeners_judged"].as_u64().unwrap_or(0) >= 3,
            "{tag}: {ex}"
        );
        assert!(un.is_empty(), "{tag}: {ex}");
        eprintln!(
            "333 staged {tag}: console-rust-primary {} {:?} http {}; console {}; unmanaged {:?}",
            p["status"],
            reasons,
            p["http"],
            comp(&st, "console")["status"],
            un
        );

        // Control: the live table (no primary entry, no deep) on the same listeners.
        let lv = serve(LIVE_COMPONENTS, &format!("live-{tag}"), event, |st| {
            samples(st, "console") >= want
                && ports(&st["cortex"]["exposure"]["unmanaged_ports"]).contains(&(PRIMARY as u64))
        });
        let lex = &lv["cortex"]["exposure"];
        assert_eq!(comp(&lv, "console-rust-primary"), &serde_json::Value::Null);
        assert_eq!(
            comp(&lv, "console")["status"],
            "healthy",
            "{tag}: live table = blind spot"
        );
        assert_eq!(
            ports(&lex["unmanaged_ports"]),
            [PRIMARY as u64],
            "{tag}: {lex}"
        );
        eprintln!(
            "333 live-table control {tag}: console {}; unmanaged {:?}",
            comp(&lv, "console")["status"],
            lex["unmanaged_ports"]
        );
    }
    stop.store(true, Ordering::Relaxed);
    let hits = h1.join().unwrap() + h2.join().unwrap();
    eprintln!("333 stub hits {hits}");
    assert!(hits >= 12, "{hits}");
}
