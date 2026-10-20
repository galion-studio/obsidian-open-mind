// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test on the box (gated: `GALION_REAL=1`). Starts the real
//! `openmind-nerve serve` binary on 127.0.0.1:19105 (slot 10 range) against
//! one live HTTP service (a thread here, :19106) and one dead port (:19107),
//! then floods the nerve's API and expects the DDoS detector to fire.
//! The full scenario with tuner and room is `scripts/real/nerve.sh`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(path: &str) -> Option<(u16, String)> {
    let mut s =
        TcpStream::connect_timeout(&"127.0.0.1:19105".parse().ok()?, Duration::from_secs(2))
            .ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn json(path: &str) -> serde_json::Value {
    get(path)
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

#[test]
fn real_nerve_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let svc = TcpListener::bind("127.0.0.1:19106").expect("19106 free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let dir = std::env::temp_dir().join(format!("nerve-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        r#"[{"name":"live-svc","port":19106},{"name":"dead-svc","port":19107,"gaia":"APOLLO"}]"#,
    )
    .unwrap();
    let _nerve = Kill(
        Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", "127.0.0.1:19105")
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "19105-19107")
            .env("NERVE_DDOS_MIN_RPS", "5")
            .env("NERVE_DDOS_MIN_REQUESTS", "200")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    while get("/healthz").is_none() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_secs(6));
    let st = json("/api/v1/state");
    let comp = |n: &str| {
        st["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == n)
            .cloned()
            .unwrap()
    };
    assert_eq!(comp("live-svc")["status"], "healthy", "{st}");
    assert_eq!(comp("dead-svc")["status"], "failing");
    let open = st["incidents"]["open"].as_array().unwrap();
    assert!(
        open.iter()
            .any(|i| i["cause"] == "dead-svc" && i["severity"] == "critical"),
        "{open:?}"
    );
    assert_eq!(st["system"]["status"], "failing");
    // Flood: 8 threads x 100 requests.
    let hs: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| (0..100).filter(|_| get("/healthz").is_some()).count()))
        .collect();
    let sent: usize = hs.into_iter().map(|h| h.join().unwrap()).sum();
    assert!(sent >= 700, "sent {sent}");
    std::thread::sleep(Duration::from_secs(6));
    let d = json("/api/v1/ddos");
    let api = d["streams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["stream"] == "nerve-api")
        .cloned()
        .unwrap();
    assert_eq!(api["level"], "flood", "{api}");
    assert_eq!(d["firing"], true);
    assert_eq!(api["shape"], "single-source");
    let inc = json("/api/v1/incidents");
    assert!(
        inc["open"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["cause"] == "ddos:nerve-api"),
        "{inc}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
