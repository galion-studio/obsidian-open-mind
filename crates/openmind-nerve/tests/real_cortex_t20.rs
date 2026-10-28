// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the T2.0 merge (gated: `GALION_REAL=1`, run with
//! `--release`). Slot 104: ports 19740-19749. Loopback only.
//!
//! One real `openmind-nerve serve` on 127.0.0.1:19740 with every merged S2
//! sensor switched on at once (sshd, rustsec, drift, conn-rate, proc-owner,
//! surface, hades), one live service on :19741 and one 503 origin on :19742:
//! - `cortex_sensors` lists the 5 built-ins then the 7 S2 sensors in the
//!   registry order of `Sensors::from_env`;
//! - each merged branch contributes a finding from its fixtures
//!   (rustsec lock, drift manifest, surface 503, proc-owner stray);
//! - `/api/v1/cortex/events` (cortex-s2-d) serves B33 JSONL;
//! - the whole cycle stays inside the budget (< 50 ms, RSS < 8 MiB).

use openmind_schema::NerveEvent;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): PORT +0; `CORTEX_T20_REAL_PORT_BASE`
/// moves the block, unset keeps the old 19740-19749.
static PORTS: Block = Block::new("CORTEX_T20_REAL_PORT_BASE", 19740, 10);

#[allow(non_snake_case)]
fn PORT() -> u16 {
    PORTS.port(0)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, path: &str) -> Option<(String, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.to_string(), body.to_string()))
}

fn state() -> Value {
    http(PORT(), "/api/v1/state")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn ids(st: &Value) -> Vec<String> {
    let c = &st["cortex"];
    [
        &c["vulnerabilities"],
        &c["bleeding"],
        &c["ddos"]["findings"],
    ]
    .iter()
    .filter_map(|a| a.as_array())
    .flatten()
    .filter_map(|f| f["id"].as_str().map(str::to_string))
    .collect()
}

fn serve(port: u16, reply: &'static [u8]) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("origin port free");
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(reply);
        }
    });
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

#[test]
fn real_cortex_t20_all_sensors_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!("nerve-real-t20-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    serve(
        PORTS.port(1),
        b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n",
    );
    serve(
        PORTS.port(2),
        b"HTTP/1.0 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
    );
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"t20-svc","port":{}}}]"#, PORTS.port(1)),
    )
    .unwrap();
    let lock = fx.join("rustsec/Cargo.lock");
    let manifest = fx.join("drift/SHA256SUMS");
    let deployed = fx.join("drift/deployed");
    let nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{}", PORT()))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", &PORTS.range())
            .env_remove("NERVE_SENSORS_OFF")
            .env("NERVE_SSHD", "1")
            .env("NERVE_SSHD_AUTH_LOG", "0")
            .env("NERVE_RUSTSEC_LOCK", &lock)
            .env("NERVE_RUSTSEC_COMPONENT", "t20-svc")
            .env("NERVE_DRIFT_MANIFEST", &manifest)
            .env("NERVE_DRIFT_ROOT", &deployed)
            .env("NERVE_CONNRATE_PORTS", format!("{}=t20-svc", PORTS.port(1)))
            .env("NERVE_PROC_OWNER_USERS", "root")
            .env(
                "NERVE_SURFACE_TARGETS",
                format!(
                    "http://{}/@200,http://{}/@200",
                    PORTS.addr(1),
                    PORTS.addr(2)
                ),
            )
            .env("NERVE_HADES", "1")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );

    let want = [
        "rustsec-",
        "drift-missing-room",
        "surface-http-status",
        "stray-owner",
    ];
    let t = Instant::now();
    let st = loop {
        let st = state();
        let got = ids(&st);
        let all = want.iter().all(|w| got.iter().any(|g| g.starts_with(w)));
        if (all && st["cycle"].as_u64().unwrap_or(0) >= 2) || t.elapsed() > Duration::from_secs(40)
        {
            break st;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .expect("cortex_sensors")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "listeners",
            "scanners",
            "edge",
            "components",
            "ddos",
            "sshd",
            "rustsec",
            "drift",
            "conn-rate",
            "proc-owner",
            "surface",
            "hades",
            // T2.12 edge-rate is on by default since cortex-merge-all (NERVE_EDGE_HOSTS=0 turns it off).
            "edge-rate"
        ],
        "{st}"
    );
    let got = ids(&st);
    for w in want {
        assert!(
            got.iter().any(|g| g.starts_with(w)),
            "{w} missing in {got:?}"
        );
    }
    // Events: B33, no evidence lines leave.
    for e in st["cortex_events"].as_array().unwrap() {
        let _: NerveEvent = serde_json::from_value(e.clone()).expect("B33 event");
    }
    let (head, body) = http(PORT(), "/api/v1/cortex/events").expect("feed");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let feed: Vec<NerveEvent> = body
        .lines()
        .map(|l| serde_json::from_str(l).expect("B33 line"))
        .collect();
    let mut sources: Vec<&str> = feed.iter().map(|e| e.source.as_str()).collect();
    sources.sort_unstable();
    sources.dedup();
    let cycle_ms = st["cycle_ms"].as_u64().unwrap_or(u64::MAX);
    let rss = rss_kib(nerve.0.id());
    let us: Vec<String> = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| format!("{}={}", s["name"].as_str().unwrap_or("?"), s["us"]))
        .collect();
    eprintln!(
        "serve :{port} cycle {}: cycle_ms {cycle_ms}, rss {rss} KiB, sensors us [{}]",
        st["cycle"],
        us.join(" "),
        port = PORT()
    );
    eprintln!("finding ids {got:?}");
    eprintln!("feed: {} B33 line(s), sources {sources:?}", feed.len());
    assert!(cycle_ms < 50, "cycle budget: {cycle_ms} ms");
    assert!(rss > 0 && rss < 8 * 1024, "RSS budget: {rss} KiB");
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
