// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for inc-865945dd "galion-one mouth stuck loading" (slot 358,
//! ports 22280-22289). Gated: `GALION_REAL=1`, run with `--release`.
//!
//! Three REAL binaries serve a mouth (no mocks):
//! * `REAL_MOUTH_LIVE_BIN`   galion-one built from 40454f9 = the build live on
//!   the VPS since 2026-10-21 (mouth :22281, text :22284). Its `/health` is a
//!   hard-coded `"status":"loading"` over the `Unavailable` speech stub.
//! * `REAL_MOUTH_HONEST_BIN` galion-one from this branch (mouth :22282, text
//!   :22285): honest 503 `"unavailable"` + reason "voice not deployed".
//! * `REAL_MOUTH_STUB_BIN`   galion-mouth-stub from this branch (:22283).
//!
//! `openmind-nerve serve` on :22280 watches all three. Proven:
//! 1. the live build is NOT stuck: its body is identical before and after a
//!    real restart (SIGKILL + respawn) and `/synthesize` fails at once, so a
//!    restart (the old advice) can never clear it;
//! 2. the nerve reports the live build as `stuck-loading` (error) with the
//!    new "a restart will not help" fix text;
//! 3. the nerve reports both honest bodies as `not-deployed` (warn), never
//!    `stuck-loading`, with the "voice not deployed" reason visible.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const NERVE: u16 = 22280;
const LIVE_MOUTH: u16 = 22281;
const HONEST_MOUTH: u16 = 22282;
const STUB: u16 = 22283;
const LIVE_TEXT: u16 = 22284;
const HONEST_TEXT: u16 = 22285;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn req(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(
        s,
        "{method} {path} HTTP/1.0\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let code = b.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, b.split_once("\r\n\r\n")?.1.to_string()))
}

fn wait_up(port: u16, what: &str) {
    let t0 = Instant::now();
    while req(port, "GET", "/health", "").is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{what} :{port} never answered"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn need_bin(var: &str) -> String {
    let p = std::env::var(var).unwrap_or_else(|_| panic!("set {var} (no skipping: slot 358 rule)"));
    assert!(Path::new(&p).is_file(), "{var}={p} is not a file");
    p
}

fn one(bin: &str, mouth: u16, text: u16, home: &Path) -> Kill {
    std::fs::create_dir_all(home).unwrap();
    let k = Kill(
        Command::new(bin)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home)
            .env("GALION_HOME", home)
            .env("ONE_BIND", format!("127.0.0.1:{text}"))
            .env("ONE_MOUTH_BIND", format!("127.0.0.1:{mouth}"))
            .env("ONE_NO_WARM", "1")
            .env("ONE_HERMES_DIR", home.join("hermes"))
            .env("ONE_DEVICE_STORE", home.join("device.json"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn galion-one"),
    );
    wait_up(mouth, "galion-one mouth");
    k
}

fn findings(st: &Value) -> Vec<Value> {
    st["cortex"]["bleeding"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn find<'a>(f: &'a [Value], id: &str) -> Option<&'a Value> {
    f.iter().find(|x| x["id"] == id)
}

#[test]
fn real_mouth_not_deployed() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let live_bin = need_bin("REAL_MOUTH_LIVE_BIN");
    let honest_bin = need_bin("REAL_MOUTH_HONEST_BIN");
    let stub_bin = need_bin("REAL_MOUTH_STUB_BIN");
    let dir = std::env::temp_dir().join(format!("nerve-real-mouth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 1. The live build: hard-coded "loading", unchanged by a real restart.
    let live = one(&live_bin, LIVE_MOUTH, LIVE_TEXT, &dir.join("live"));
    let (c1, b1) = req(LIVE_MOUTH, "GET", "/health", "").unwrap();
    let t_syn = Instant::now();
    let (sc, sb) = req(LIVE_MOUTH, "POST", "/synthesize", r#"{"text":"hello"}"#).unwrap();
    let syn_ms = t_syn.elapsed().as_millis();
    eprintln!("live 40454f9 :{LIVE_MOUTH} /health {c1} {b1}");
    eprintln!("live 40454f9 /synthesize {sc} in {syn_ms} ms: {sb}");
    assert_eq!(c1, 503);
    assert!(
        b1.contains(r#""status":"loading""#) && b1.contains("chatterbox"),
        "{b1}"
    );
    assert_ne!(sc, 200, "live build has no voice worker: {sb}");
    assert!(
        syn_ms < 3000,
        "synthesize should fail at once, took {syn_ms} ms"
    );
    drop(live); // SIGKILL + wait, then respawn the same binary
    std::thread::sleep(Duration::from_millis(300));
    let _live = one(&live_bin, LIVE_MOUTH, LIVE_TEXT, &dir.join("live"));
    let (c2, b2) = req(LIVE_MOUTH, "GET", "/health", "").unwrap();
    eprintln!("live 40454f9 after SIGKILL+respawn /health {c2} {b2}");
    assert_eq!((c2, &b2), (c1, &b1), "restart changed the body");

    // Honest bodies from this branch (real binaries).
    let _honest = one(&honest_bin, HONEST_MOUTH, HONEST_TEXT, &dir.join("honest"));
    let _stub = Kill(
        Command::new(&stub_bin)
            .args(["--bind", &format!("127.0.0.1:{STUB}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn galion-mouth-stub"),
    );
    wait_up(STUB, "galion-mouth-stub");
    for (port, what) in [
        (HONEST_MOUTH, "honest galion-one"),
        (STUB, "galion-mouth-stub"),
    ] {
        let (c, b) = req(port, "GET", "/health", "").unwrap();
        eprintln!("{what} :{port} /health {c} {b}");
        assert_eq!(c, 503);
        assert!(
            b.contains(r#""status":"unavailable""#) && b.contains("voice not deployed"),
            "{b}"
        );
        assert!(!b.contains("loading") && !b.contains("chatterbox"), "{b}");
    }

    // 2+3. The nerve watching all three.
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"galion-one-mouth","port":{LIVE_MOUTH},"group":"galion-one","critical":false}},
                {{"name":"mouth-honest-one","port":{HONEST_MOUTH},"group":"honest-one","critical":false}},
                {{"name":"mouth-stub","port":{STUB},"group":"mouth-stub","critical":false}}]"#
        ),
    )
    .unwrap();
    let t0 = Instant::now();
    let _nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{NERVE}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_LOADING_GRACE", "3")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "22280-22289")
            .env_remove("NERVE_SENSORS_OFF")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let want = [
        "stuck-loading-galion-one-mouth",
        "not-deployed-mouth-honest-one",
        "not-deployed-mouth-stub",
    ];
    let st = loop {
        let raw = req(NERVE, "GET", "/api/v1/state", "")
            .map(|r| r.1)
            .unwrap_or_default();
        let st: Value = serde_json::from_str(&raw).unwrap_or_default();
        let f = findings(&st);
        if want.iter().all(|w| find(&f, w).is_some()) || t0.elapsed() > Duration::from_secs(60) {
            break st;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let f = findings(&st);
    let ids: Vec<&str> = f.iter().filter_map(|x| x["id"].as_str()).collect();
    eprintln!(
        "nerve :{NERVE} cycle {} after {} ms, bleeding ids {ids:?}",
        st["cycle"],
        t0.elapsed().as_millis()
    );
    for w in want {
        assert!(find(&f, w).is_some(), "{w} missing: {ids:?}");
    }
    assert!(
        find(&f, "stuck-loading-mouth-honest-one").is_none(),
        "{ids:?}"
    );
    assert!(find(&f, "stuck-loading-mouth-stub").is_none(), "{ids:?}");
    assert!(
        find(&f, "not-deployed-galion-one-mouth").is_none(),
        "{ids:?}"
    );

    let stuck = find(&f, "stuck-loading-galion-one-mouth").unwrap();
    eprintln!("stuck: {stuck}");
    assert_eq!(
        stuck["severity"].as_str().or(stuck["sev"].as_str()),
        Some("error"),
        "{stuck}"
    );
    assert!(
        stuck.to_string().contains("a restart will not help"),
        "{stuck}"
    );
    for id in ["not-deployed-mouth-honest-one", "not-deployed-mouth-stub"] {
        let nd = find(&f, id).unwrap();
        eprintln!("{id}: {nd}");
        let s = nd.to_string();
        assert_eq!(
            nd["severity"].as_str().or(nd["sev"].as_str()),
            Some("warn"),
            "{s}"
        );
        assert!(s.contains("voice not deployed"), "{s}");
        assert!(s.contains("do not restart"), "{s}");
    }
    let incs = st["incidents"].to_string();
    eprintln!("incidents: {incs}");
    assert!(incs.contains("stuck loading for"), "{incs}");
    assert!(incs.contains("not deployed for"), "{incs}");
    let open = st["incidents"]["open"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for i in &open {
        let next = i["next"].as_str().unwrap_or_default();
        assert!(
            next.contains("a restart will not help"),
            "{} next: {next}",
            i["cause"]
        );
    }
    assert_eq!(open.len(), 3, "one incident per mouth: {incs}");
    let _ = std::fs::remove_dir_all(&dir);
}
