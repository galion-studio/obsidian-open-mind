// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! F-313-2 regression guard (gated: `GALION_REAL=1`), wave-4 slot 332.
//!
//! The blind spot: the live nerve called the Rust console `healthy` because
//! `/` answers 302 while `/api/v1/admin` and the board routes were 404
//! (slot 313 VPS read-only probe). Slot 315 added deep route probes. This
//! test keeps that fixed: it boots REAL `galion-console-serve` binaries (no
//! stubs) with the admin gate and board routes disabled and asserts that the
//! nerve does NOT report them healthy on any surface.
//!
//! The probed console spec is the nerve's own built-in `console` spec
//! (`config::default_specs`: path, gaia, group, `deep` list), moved to a box
//! port, so emptying `CONSOLE_DEEP` or dropping it from the default spec
//! fails this test too.
//!
//! Consoles (block 22020-22029 by default, `REAL_NERVE_PORT_BASE` moves it;
//! nerve = block+0):
//! - `console` block+1 = the live build (`2d846fb8`, `--local-api`): admin +
//!   board 404 -> must be `degrading`, never `healthy`, never hard down;
//! - `console-noadmin` block+2 = fixed build (board API + admin gate, e.g.
//!   `dfa120c6`) WITHOUT `--admin-store`: admin 503 -> must not be healthy;
//!   then restarted WITH the store on the same port -> must go back to
//!   `healthy` (the verdict is not sticky);
//! - `console-full` block+3 = fixed build with the store: `healthy` control;
//! - finally `console` is killed -> `failing` (hard), no deep record.
//!
//! F-396-1 (slot 411): the blind console is also the box repro of the
//! public outage (Host `console.galion.app` -> 503 on every room, loopback
//! 200, since 2026-10-27 19:28 CET); the test asserts that shape first, so
//! "degrading" here is proven on the real 503 console, not a stand-in.
//!
//! Binaries: `NERVE_BLINDSPOT_CONSOLE_BLIND_BIN` (default
//! `/workspace/swarm/artifacts/t291u/galion-console-serve-2d846fb8`),
//! `NERVE_BLINDSPOT_CONSOLE_FIXED_BIN` (optional: without it only the blind
//! console part runs), `CONSOLE_DIST_STATIC` (console-dist `static` folder).
//! `NERVE_BLINDSPOT_NERVE_BIN` swaps the nerve under test (e.g. a pre-slot-315
//! build, which must FAIL this test: that is how the guard was proven).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use openmind_nerve::config::{default_specs, CONSOLE_DEEP};
use serde_json::{json, Value};

/// Port block start (default 22020, slot 332). Slot 411 (nerve-merge-7) moves
/// the block at build time with `REAL_NERVE_PORT_BASE=<port>` like the other
/// merged nerve real tests (real_console_deep); offsets are unchanged.
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

const BLOCK: u16 = port_block(22020);
fn nerve_addr() -> String {
    format!("127.0.0.1:{BLOCK}")
}
const BLIND: u16 = BLOCK + 1;
const NOADMIN: u16 = BLOCK + 2;
const FULL: u16 = BLOCK + 3;
/// Public host the live tunnel forwards (F-396-1: every room 503 there).
const PUBLIC_HOST: &str = "console.galion.app";
/// Public rooms checked for the F-396-1 shape (slot 396 box repro table).
const ROOMS: &[&str] = &["/console", "/node", "/hermes", "/status"];
const INTERVAL_S: u64 = 5;

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

fn code(port: u16, path: &str) -> Option<u16> {
    get(&format!("127.0.0.1:{port}"), path).map(|x| x.0)
}

/// GET with an explicit `Host` (the public tunnel host), code only.
fn code_host(port: u16, host: &str, path: &str) -> Option<u16> {
    let addr = format!("127.0.0.1:{port}");
    let mut s = TcpStream::connect_timeout(&addr.parse().ok()?, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    b.split_whitespace().nth(1)?.parse().ok()
}

/// F-396-1 shape: the public host gets 503 on every room while loopback
/// renders them (the gate's upstream `/api/v1/admin` is 404). Returns the
/// (room, public, loopback) table.
fn f396_rooms(port: u16) -> Vec<(&'static str, Option<u16>, Option<u16>)> {
    ROOMS
        .iter()
        .map(|r| (*r, code_host(port, PUBLIC_HOST, r), code(port, r)))
        .collect()
}

fn json_get(path: &str) -> Value {
    get(&nerve_addr(), path)
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn comp(st: &Value, n: &str) -> Value {
    st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"] == n).cloned())
        .unwrap_or_default()
}

fn reasons(c: &Value) -> Vec<String> {
    c["reasons"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn console(bin: &Path, port: u16, dist: &Path, admin: Option<&Path>) -> Kill {
    let mut c = Command::new(bin);
    c.args(["--bind", &format!("127.0.0.1:{port}")])
        .arg("--dist")
        .arg(dist)
        .args(["--ui", "rust", "--local-api"])
        // Live argv has --pulse 8091 / --desk 8646; dead ports in our range.
        .args([
            "--pulse",
            &format!("127.0.0.1:{}", BLOCK + 8),
            "--desk",
            &format!("127.0.0.1:{}", BLOCK + 9),
            "--quiet",
        ]);
    if let Some(d) = admin {
        c.arg("--admin-store")
            .arg(d.join("admin-device.json"))
            .arg("--admin-mirror")
            .arg(d.join("mirror/admin-device.json"));
    }
    let child = c
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn console {}: {e}", bin.display()));
    let a = format!("127.0.0.1:{port}");
    let t = Instant::now();
    while get(&a, "/api/v1/health").is_none() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "console :{port} not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    Kill(Some(child))
}

/// Wait until `name` has at least `n` more samples than `from` (or any
/// `pred`-true state), return the state.
fn wait_for(what: &str, secs: u64, pred: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = json_get("/api/v1/state");
        if pred(&st) {
            eprintln!("  [{what}] after {} ms", t.elapsed().as_millis());
            return st;
        }
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "{what}: timed out after {secs} s; state components: {}",
            st["components"]
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn samples(st: &Value, n: &str) -> u64 {
    comp(st, n)["samples"].as_u64().unwrap_or(0)
}

fn show(c: &Value) {
    eprintln!(
        "  {} status={} http={} consecutive_down={} reasons={} deep={}",
        c["name"], c["status"], c["http"], c["consecutive_down"], c["reasons"], c["extra"]["deep"]
    );
}

/// The core guard: a console whose admin/board routes are gone is not healthy.
fn assert_not_healthy(c: &Value, missing: &[(&str, u16)]) {
    let name = &c["name"];
    assert_ne!(
        c["status"], "healthy",
        "F-313-2 REGRESSED: nerve reports {name} healthy while {missing:?} fail: {c}"
    );
    assert_eq!(c["status"], "degrading", "{c}");
    assert_eq!(
        c["consecutive_down"], 0,
        "{name}: a missing route must not arm the restart reflex: {c}"
    );
    let r = reasons(c);
    for (path, http) in missing {
        let want = format!("degrading: deep route {path} HTTP {http}");
        assert!(
            r.contains(&want),
            "{name}: missing reason {want:?} in {r:?}"
        );
        let d = c["extra"]["deep"]
            .as_array()
            .and_then(|a| a.iter().find(|d| d["path"] == *path).cloned())
            .unwrap_or_default();
        assert_eq!(d["ok"], false, "{name}: deep record for {path}: {d}");
        assert_eq!(d["http"], *http, "{name}: deep record for {path}: {d}");
    }
}

#[test]
fn real_console_blindspot_guard() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    // The built-in console spec is what the live nerve uses; guard it first.
    let spec = default_specs(None, None)
        .into_iter()
        .find(|s| s.name == "console")
        .expect("built-in console spec");
    assert!(
        spec.deep.contains(&"/api/v1/admin") && spec.deep.contains(&"/api/v1/services"),
        "F-313-2 REGRESSED: built-in console deep list {:?}",
        spec.deep
    );
    assert_eq!(spec.deep, CONSOLE_DEEP);

    let blind_bin = std::env::var_os("NERVE_BLINDSPOT_CONSOLE_BLIND_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/workspace/swarm/artifacts/t291u/galion-console-serve-2d846fb8".into());
    assert!(
        blind_bin.is_file(),
        "real blind console binary missing: {} (set NERVE_BLINDSPOT_CONSOLE_BLIND_BIN; no stubs in this guard)",
        blind_bin.display()
    );
    let fixed_bin = std::env::var_os("NERVE_BLINDSPOT_CONSOLE_FIXED_BIN").map(PathBuf::from);
    if let Some(f) = &fixed_bin {
        assert!(f.is_file(), "{}", f.display());
    }
    let nerve_bin = std::env::var_os("NERVE_BLINDSPOT_NERVE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_openmind-nerve").into());
    eprintln!(
        "nerve {} | blind console {} | fixed console {:?}",
        nerve_bin.display(),
        blind_bin.display(),
        fixed_bin
    );

    let dir = std::env::temp_dir().join(format!("nerve-blindspot-332-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("admin/mirror")).unwrap();
    std::fs::create_dir_all(dir.join("admin2/mirror")).unwrap();
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

    let mut blind = console(&blind_bin, BLIND, &dist, None);
    let mut noadmin = Kill(None);
    let mut _full = Kill(None);
    if let Some(f) = &fixed_bin {
        noadmin = console(f, NOADMIN, &dist, None);
        _full = console(f, FULL, &dist, Some(&dir.join("admin")));
    }

    // Preconditions: the blind spot really exists (else the guard is vacuous):
    // the health path answers while the deep routes do not.
    let pre = |p: u16| {
        let v: Vec<(&str, Option<u16>)> = std::iter::once(spec.path)
            .chain(spec.deep.iter().copied())
            .map(|path| (path, code(p, path)))
            .collect();
        eprintln!("direct GET :{p} -> {v:?}");
        v
    };
    let b = pre(BLIND);
    assert!(
        matches!(b[0].1, Some(200..=399)),
        "blind console {} must answer: {b:?}",
        spec.path
    );
    assert_eq!(
        b[1..].iter().map(|x| x.1).collect::<Vec<_>>(),
        vec![Some(404); spec.deep.len()],
        "blind console must 404 every deep route (the live 2d846fb8 shape): {b:?}"
    );
    // F-396-1 (slot 396): this is also the public-console outage shape. With
    // the public Host every room is 503 (fail-closed page gate), loopback 200.
    let rooms = f396_rooms(BLIND);
    eprintln!("F-396-1 rooms :{BLIND} (room, Host {PUBLIC_HOST}, loopback) -> {rooms:?}");
    assert!(
        rooms.iter().all(|r| r.1 == Some(503) && r.2 == Some(200)),
        "blind console must reproduce F-396-1 (public 503 / loopback 200): {rooms:?}"
    );
    if fixed_bin.is_some() {
        let n = pre(NOADMIN);
        assert!(matches!(n[0].1, Some(200..=399)), "{n:?}");
        assert_eq!(
            n[1].1,
            Some(503),
            "fixed w/o --admin-store: admin 503: {n:?}"
        );
        assert_eq!(n[2].1, Some(200), "fixed: board route 200: {n:?}");
        let f = pre(FULL);
        assert!(f.iter().all(|x| matches!(x.1, Some(200..=399))), "{f:?}");
        // The fixed build with its store (e.g. live 5aa31927) serves the
        // public rooms: F-396-1 does not apply, and the nerve says healthy.
        let fr = f396_rooms(FULL);
        eprintln!("F-396-1 rooms :{FULL} (room, Host {PUBLIC_HOST}, loopback) -> {fr:?}");
        assert!(
            fr.iter().all(|r| r.1 != Some(503)),
            "fixed console must not 503 the public rooms: {fr:?}"
        );
    }

    // Components file = the built-in console spec, moved to box ports.
    let entry = |name: &str, port: u16| {
        json!({"name": name, "port": port, "path": spec.path, "gaia": spec.gaia,
               "system": spec.gaia_system, "group": spec.group, "deep": spec.deep})
    };
    let mut list = vec![entry("console", BLIND)];
    if fixed_bin.is_some() {
        list.push(entry("console-noadmin", NOADMIN));
        list.push(entry("console-full", FULL));
    }
    let comps = dir.join("components.json");
    std::fs::write(&comps, Value::Array(list).to_string()).unwrap();
    let mut nerve = Kill(Some(
        Command::new(&nerve_bin)
            .arg("serve")
            .env("NERVE_ADDR", nerve_addr())
            .env("NERVE_INTERVAL", INTERVAL_S.to_string())
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
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    ));

    // Phase 1: two samples of every console.
    let names: Vec<&str> = if fixed_bin.is_some() {
        vec!["console", "console-noadmin", "console-full"]
    } else {
        vec!["console"]
    };
    let st = wait_for("2 samples each", 45, |st| {
        names.iter().all(|n| samples(st, n) >= 2)
    });
    for n in &names {
        show(&comp(&st, n));
    }
    let health = json_get("/health");
    eprintln!(
        "  system={} gaia.HEPHAESTUS={} /health.system={} open_incidents={}",
        st["system"], st["gaia"]["HEPHAESTUS"], health["system"], st["system"]["open_incidents"]
    );
    let c = comp(&st, "console");
    assert_eq!(c["http"], 302, "{c}");
    assert_not_healthy(&c, &[("/api/v1/admin", 404), ("/api/v1/services", 404)]);
    // Roll-ups must not hide it either.
    assert_ne!(
        st["system"]["components_worst"], "healthy",
        "{}",
        st["system"]
    );
    assert_ne!(
        st["gaia"]["HEPHAESTUS"]["status"], "healthy",
        "{}",
        st["gaia"]
    );
    let hep = st["gaia"]["HEPHAESTUS"]["components"].to_string();
    assert!(hep.contains("console:degrading"), "{hep}");
    // ... and an open warn incident carries the blind console's deep signal,
    // so the system status (incident driven) and /health are not healthy.
    let inc = st["incidents"]["open"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|i| {
            i["signals"].as_array().is_some_and(|s| {
                s.iter().any(|x| {
                    x["component"] == "console"
                        && x["msg"]
                            .as_str()
                            .is_some_and(|m| m.contains("deep route /api/v1/admin HTTP 404"))
                })
            })
        })
        .unwrap_or_else(|| {
            panic!(
                "no open incident for the blind console: {}",
                st["incidents"]
            )
        });
    eprintln!(
        "  incident {} cause={} severity={} components={}",
        inc["id"], inc["cause"], inc["severity"], inc["components"]
    );
    assert_eq!(inc["severity"], "warn", "{inc}");
    assert_ne!(st["system"]["status"], "healthy", "{}", st["system"]);
    assert_ne!(health["system"], "healthy", "/health: {health}");
    let html = get(&nerve_addr(), "/").map(|x| x.1).unwrap_or_default();
    assert!(
        html.contains("deep route /api/v1/admin HTTP 404"),
        "dashboard must show the deep reason"
    );

    if let Some(fb) = &fixed_bin {
        let na = comp(&st, "console-noadmin");
        assert_not_healthy(&na, &[("/api/v1/admin", 503)]);
        assert!(
            !reasons(&na).iter().any(|r| r.contains("/api/v1/services")),
            "board route is 200 on the fixed build: {na}"
        );
        let f = comp(&st, "console-full");
        assert_eq!(f["status"], "healthy", "control: {f}");
        assert!(reasons(&f).is_empty(), "{f}");

        // Phase 2: give console-noadmin its store (same port) -> healthy again.
        noadmin.stop();
        let _noadmin2 = console(fb, NOADMIN, &dist, Some(&dir.join("admin2")));
        assert_eq!(code(NOADMIN, "/api/v1/admin"), Some(200));
        let s0 = samples(&json_get("/api/v1/state"), "console-noadmin");
        let st = wait_for("noadmin recovers", 30, |st| {
            samples(st, "console-noadmin") > s0
                && comp(st, "console-noadmin")["status"] == "healthy"
        });
        show(&comp(&st, "console-noadmin"));
        // The blind one stays flagged on every later cycle.
        assert_not_healthy(
            &comp(&st, "console"),
            &[("/api/v1/admin", 404), ("/api/v1/services", 404)],
        );
    }

    // Phase 3: kill the blind console -> hard failing, no deep probes.
    blind.stop();
    let st = wait_for("blind console killed", 30, |st| {
        comp(st, "console")["status"] == "failing"
    });
    let c = comp(&st, "console");
    show(&c);
    assert!(
        c["extra"]["deep"].is_null(),
        "no deep probes when down: {c}"
    );
    assert!(reasons(&c).iter().all(|r| !r.contains("deep route")), "{c}");

    // GETs only: the admin store dirs hold nothing the probes wrote.
    for d in ["admin", "admin2"] {
        let names: Vec<String> = std::fs::read_dir(dir.join(d))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        eprintln!("  {d}/ after probes: {names:?}");
        assert_eq!(names, vec!["mirror".to_string()], "{d}: {names:?}");
    }
    nerve.stop();
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("F-313-2 guard: pass");
}
