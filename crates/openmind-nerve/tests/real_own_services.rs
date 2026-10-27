// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 238 nerve-own-services (D-T44-2), real-life test on the box, gated
//! with `GALION_REAL=1`. Runs the real `openmind-nerve` binary (release when
//! built with `--release`), ports 21080-21089 by default (REAL_NERVE_PORT_BASE remaps).
//!
//! 1. `replay` over the live VPS listener table captured read-only on
//!    2026-10-22 01:09:46 +02:00 (`ss -ltnp`, fixture
//!    `tests/fixtures/own-services/`). The live nerve bb287cc reported
//!    `unmanaged_ports [8672, 8674, 18800, 19120]` at that time. Expected
//!    now: only :18800 (sshd in a login session: a manual ssh port forward,
//!    a real stray) is unmanaged; 8672/8674/19120 are owned by
//!    galion-nerve-shadow / gaia-cortex / openmind-queue.
//! 2. The same table with `NERVE_UNOWNED_PORTS=8674`: the documented-unowned
//!    override still wins over the built-in own-service list.
//! 3. `serve` in both modes (legacy and `--mode event`) on the block base
//!    port with a real stray listener on base+1 (held by this test, no unit):
//!    it is still reported in `unmanaged_ports` with this process as the holder.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Port block start. Default is slot 238's original range; slot 292
/// (nerve-merge-4) moves the whole block at build time with
/// `REAL_NERVE_PORT_BASE=<port>` (e.g. 21620) so merged re-runs stay inside
/// their own slot range. Offsets inside the block are unchanged.
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

const BLOCK: u16 = port_block(21080);
const NERVE_PORT: u16 = BLOCK;
const STRAY: u16 = BLOCK + 1;
const DEAD_PORT: u16 = BLOCK + 2;
fn nerve_addr() -> String {
    format!("127.0.0.1:{NERVE_PORT}")
}

fn listen_scope() -> String {
    format!("{BLOCK}-{}", BLOCK + 9)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn real() -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("skipped: set GALION_REAL=1");
    }
    on
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_openmind-nerve")
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-238-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn capture_dir(tag: &str) -> PathBuf {
    capture_dir_from(
        tag,
        "ss-ltnp-vps-20261022-0110.txt",
        "2026-10-22T01:09:46+02:00",
    )
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/own-services")
            .join(name),
    )
    .unwrap()
}

fn capture_dir_from(tag: &str, ss_fixture: &str, ts: &str) -> PathBuf {
    let d = tmp(tag);
    let ss = fixture(ss_fixture);
    std::fs::write(d.join("ss.txt"), ss).unwrap();
    std::fs::write(
        d.join("state-1.json"),
        format!(r#"{{"ts":"{ts}""#) + r#","components":[{"name":"lxc-host","status":"healthy","host":{"disk_total_kb":100,"disk_avail_kb":75,"ram_total_kb":100,"ram_avail_kb":90,"load1":0.5}}],"probe_errors":[]}"#,
    )
    .unwrap();
    d
}

fn replay(dir: &Path, env: &[(&str, &str)]) -> serde_json::Value {
    let mut c = Command::new(bin());
    c.arg("replay").arg(dir).arg("--state");
    c.env("NERVE_DATA_DIR", dir.join("data"));
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.stderr(Stdio::null()).output().expect("run replay");
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("replay json: {e}: {}", String::from_utf8_lossy(&out.stdout)))
}

fn ports(v: &serde_json::Value) -> Vec<u64> {
    v.as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|p| p.as_u64())
        .collect()
}

fn owner_of(exposure: &serde_json::Value, port: u64) -> serde_json::Value {
    exposure["ownership"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["port"] == port)
        .cloned()
        .unwrap_or_default()
}

fn unmanaged_title(st: &serde_json::Value) -> Option<String> {
    st["cortex"]["vulnerabilities"]
        .as_array()?
        .iter()
        .find(|f| f["id"] == "unmanaged-listeners")
        .and_then(|f| f["title"].as_str().map(str::to_string))
}

#[test]
fn real_replay_live_vps_listeners() {
    if !real() {
        return;
    }
    let dir = capture_dir("replay");
    let st = replay(&dir, &[]);
    let ex = &st["cortex"]["exposure"];
    assert_eq!(ports(&ex["unmanaged_ports"]), [18800], "{ex}");
    for (p, owner, gaia) in [
        (8672, "galion-nerve-shadow", "AETHER"),
        (8674, "gaia-cortex", "AETHER"),
        (19120, "openmind-queue", "POSEIDON"),
    ] {
        let o = owner_of(ex, p);
        assert_eq!(
            (o["owner"].as_str(), o["gaia"].as_str()),
            (Some(owner), Some(gaia)),
            "{o}"
        );
    }
    let s = owner_of(ex, 18800);
    assert_eq!(s["owner"], "unknown owner", "{s}");
    assert_eq!(s["process"], "sshd", "{s}");
    assert_eq!(
        unmanaged_title(&st).as_deref(),
        Some("1 loopback listener(s) no monitored unit owns: :18800")
    );
    eprintln!(
        "238 replay: unmanaged {:?}; finding {:?}",
        ex["unmanaged_ports"],
        unmanaged_title(&st)
    );

    // Override: a port listed in NERVE_UNOWNED_PORTS is reported again.
    let st = replay(&dir, &[("NERVE_UNOWNED_PORTS", "8674")]);
    let ex = &st["cortex"]["exposure"];
    assert_eq!(ports(&ex["unmanaged_ports"]), [8674, 18800], "{ex}");
    let o = owner_of(ex, 8674);
    assert_eq!(o["documented_unowned"], true, "{o}");
    eprintln!("238 replay override: unmanaged {:?}", ex["unmanaged_ports"]);
    let _ = std::fs::remove_dir_all(&dir);
}

fn get(path: &str) -> Option<(u16, String)> {
    let mut s =
        TcpStream::connect_timeout(&nerve_addr().parse().ok()?, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn serve_once(mode_event: bool) {
    let stray = TcpListener::bind(("127.0.0.1", STRAY)).expect("stray port free");
    let dir = tmp(if mode_event { "event" } else { "legacy" });
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"dead-svc","port":{DEAD_PORT}}}]"#),
    )
    .unwrap();
    let mut c = Command::new(bin());
    c.arg("serve");
    if mode_event {
        c.args(["--mode", "event"]);
    }
    let addr = nerve_addr();
    let scope = listen_scope();
    let _nerve = Kill(
        c.env("NERVE_ADDR", &addr)
            .env("NERVE_INTERVAL", "3")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_LISTEN_SCOPE", &scope)
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    let mut last = serde_json::Value::Null;
    let mut ok = false;
    while t.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(500));
        let Some((200, b)) = get("/api/v1/state") else {
            continue;
        };
        last = serde_json::from_str(&b).unwrap_or_default();
        if ports(&last["cortex"]["exposure"]["unmanaged_ports"]) == [STRAY as u64] {
            ok = true;
            break;
        }
    }
    let ex = &last["cortex"]["exposure"];
    assert!(ok, "mode_event={mode_event}: {ex}");
    let o = owner_of(ex, STRAY as u64);
    assert_eq!(o["owner"], "unknown owner", "{o}");
    if let Some(pid) = o["pid"].as_u64() {
        assert_eq!(pid, std::process::id() as u64, "{o}");
    }
    let me = owner_of(ex, NERVE_PORT as u64);
    assert_eq!(me["owner"], "galion-nerve (self)", "{me}");
    eprintln!(
        "238 serve mode_event={mode_event} after {:.1}s: unmanaged {:?}; stray holder {} pid {}; finding {:?}",
        t.elapsed().as_secs_f64(),
        ex["unmanaged_ports"],
        o["process"],
        o["pid"],
        unmanaged_title(&last)
    );
    drop(stray);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_serve_strays_still_reported() {
    if !real() {
        return;
    }
    serve_once(false);
    serve_once(true);
}

/// Slot 316 (nerve-merge-5): replay of the live VPS listener table captured
/// read-only on 2026-10-27 22:26 +01:00 (`ss -ltnp`) must give the same
/// listener ownership as the live nerve openmind-nerve-9d1a4f3 reported in
/// its /state at 22:15 +01:00 (same listeners: the newest one, :18080
/// console-rust-primary, was up since 18:28 +01:00). Before the own-services
/// fold, merge-4 5dac9b4a would add :8674 and :19120 to unmanaged_ports.
#[test]
fn real_replay_matches_live_9d1a4f3_20261027() {
    if !real() {
        return;
    }
    let dir = capture_dir_from(
        "live1027",
        "ss-ltnp-vps-20261027-2226.txt",
        "2026-10-27T22:26:10+01:00",
    );
    let st = replay(&dir, &[]);
    let ex = &st["cortex"]["exposure"];
    let live: serde_json::Value =
        serde_json::from_str(&fixture("live-exposure-9d1a4f3-20261027-2215.json")).unwrap();
    let lex = &live["exposure"];
    assert_eq!(ports(&ex["unmanaged_ports"]), [18080], "{ex}");
    assert_eq!(
        ports(&ex["unmanaged_ports"]),
        ports(&lex["unmanaged_ports"]),
        "unmanaged differs from live"
    );
    let key = |e: &serde_json::Value| -> Vec<(u64, String, String)> {
        let mut v: Vec<_> = e["ownership"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o["port"].as_u64().unwrap(),
                    o["owner"].as_str().unwrap_or("").to_string(),
                    o["gaia"].as_str().unwrap_or("-").to_string(),
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(key(ex), key(lex), "ownership differs from live 9d1a4f3");
    for (p, owner, gaia) in [
        (8674, "gaia-cortex", "AETHER"),
        (19120, "openmind-queue", "POSEIDON"),
    ] {
        let o = owner_of(ex, p);
        assert_eq!(
            (o["owner"].as_str(), o["gaia"].as_str()),
            (Some(owner), Some(gaia)),
            "{o}"
        );
    }
    assert_eq!(
        unmanaged_title(&st).as_deref(),
        Some("1 loopback listener(s) no monitored unit owns: :18080")
    );
    eprintln!(
        "316 live-parity replay: unmanaged {:?}; {} listeners judged == live 9d1a4f3",
        ex["unmanaged_ports"],
        key(ex).len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
