// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the `/api/v1/state` `cortex_events` title leak (gated:
//! `GALION_REAL=1`, run with `--release` for the release binary). Slot 105:
//! ports 19750-19759.
//!
//! `openmind-nerve serve` on 127.0.0.1:19750 with a live service (:19751),
//! a stray loopback listener (:19754) and a stray wildcard listener
//! (0.0.0.0:19753, never accepted, closed within the test). The listeners
//! sensor reports both. The loopback finding shape (`cortex`) still names
//! the bind address and ports for the operator; the B33 `cortex_events`
//! carry ids, component, cause and counts only: no title, no fix, no
//! address, no stray port.

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get_raw(port: u16, path: &str) -> Option<String> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    Some(b.split_once("\r\n\r\n")?.1.to_string())
}

fn ids(st: &Value) -> Vec<String> {
    st["cortex"]["vulnerabilities"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn real_state_cortex_events_carry_no_titles() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-real-titles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let svc = TcpListener::bind("127.0.0.1:19751").expect("19751 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let comps = dir.join("components.json");
    std::fs::write(&comps, r#"[{"name":"live-svc","port":19751}]"#).unwrap();
    // Never accepted; dropped at the end of the test.
    let wild = TcpListener::bind("0.0.0.0:19753").expect("19753 free");
    let stray = TcpListener::bind("127.0.0.1:19754").expect("19754 free");
    let t0 = Instant::now();
    let nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .env("NERVE_ADDR", "127.0.0.1:19750")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "19750-19759")
            .env_remove("NERVE_SENSORS_OFF")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let want = ["public-listener-19753", "unmanaged-listeners"];
    let (raw, st) = loop {
        let raw = get_raw(19750, "/api/v1/state").unwrap_or_default();
        let st: Value = serde_json::from_str(&raw).unwrap_or_default();
        let have = ids(&st);
        if want.iter().all(|w| have.iter().any(|h| h == w))
            || t0.elapsed() > Duration::from_secs(25)
        {
            break (raw, st);
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let have = ids(&st);
    for w in want {
        assert!(have.iter().any(|h| h == w), "{w} not reported: {have:?}");
    }
    // The operator shape keeps the detail (loopback API, unchanged).
    let titles = st["cortex"]["vulnerabilities"].to_string();
    assert!(titles.contains("port 19753 listens on 0.0.0.0"), "{titles}");
    assert!(titles.contains(":19754"), "{titles}");
    // The B33 events carry none of it.
    let evs = &st["cortex_events"];
    let ev = evs
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == "listeners")
        .cloned()
        .expect("listeners event");
    let e: NerveEvent = serde_json::from_value(ev.clone()).expect("B33 NerveEvent");
    assert_eq!(
        (e.kind.as_str(), e.severity.as_str()),
        ("vulnerability", "high")
    );
    assert_eq!(ev["data"]["findings"], 2);
    assert_eq!(
        ev["data"]["top"],
        serde_json::json!({"id": "public-listener-19753", "component": "host",
                           "cause": null, "evidence_n": 1})
    );
    let all = evs.to_string();
    for leak in [
        "0.0.0.0",
        "listens on",
        ":19753",
        ":19754",
        "\"title\"",
        "\"fix\"",
    ] {
        assert!(!all.contains(leak), "{leak} leaked in cortex_events: {all}");
    }
    eprintln!(
        "serve :19750 cycle {} ({} ms to both findings), state {} bytes; cortex_events {}",
        st["cycle"],
        t0.elapsed().as_millis(),
        raw.len(),
        all
    );
    drop(nerve);
    drop(wild);
    drop(stray);
    let _ = std::fs::remove_dir_all(&dir);
}
