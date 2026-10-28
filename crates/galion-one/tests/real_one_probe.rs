// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Real-life test: starts the real galion-one binary on spare ports
// (127.0.0.1:19057, mouth 19058; swarm slot 05) with a fixture home and hits
// its HTTP routes. Skipped unless GALION_REAL=1. Set GALION_ONE_BIN to test a
// release build (e.g. the static musl one); default is cargo's test build.
//
//   GALION_REAL=1 cargo test -p galion-one --test real_one_probe -- --nocapture

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::ports::Block;

/// Slot 375 (real-ports-batch-2): block movable by `ONE_PROBE_REAL_PORT_BASE`
/// (unset = 19057): one +0, mouth +1, dead upstream +2.
static PORTS: Block = Block::new("ONE_PROBE_REAL_PORT_BASE", 19057, 3);
fn one_at() -> &'static str {
    PORTS.addr(0)
}
fn mouth_at() -> &'static str {
    PORTS.addr(1)
}
fn dead_at() -> &'static str {
    PORTS.addr(2)
}

struct Kill(Child, PathBuf);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn get(addr: &str, path: &str, extra: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n{extra}\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let code = text.split_whitespace().nth(1)?.parse().ok()?;
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_string())?;
    Some((code, body))
}

#[test]
fn real_galion_one_on_spare_ports() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }
    let bin = std::env::var("GALION_ONE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-one").to_string());
    let dir = std::env::temp_dir().join(format!("one-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home/one")).unwrap();
    std::fs::create_dir_all(dir.join("home/guardian")).unwrap();
    std::fs::write(
        dir.join("home/one/index.html"),
        "<html><body>fx</body></html>",
    )
    .unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::copy(
        fixtures.join("guardian-fixture.db"),
        dir.join("home/guardian/guardian.db"),
    )
    .unwrap();
    std::fs::write(dir.join("hermes.env"), "").unwrap();
    // No device store at all: pairing must fail closed.
    let child = Command::new(&bin)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir.join("home"))
        .env("ONE_NO_WARM", "1")
        .env("ONE_BIND", one_at())
        .env("ONE_MOUTH_BIND", mouth_at())
        .env("GALION_HOME", dir.join("home"))
        .env("ONE_HERMES_DIR", dir.join("hermes"))
        .env("ONE_HERMES_ENV", dir.join("hermes.env"))
        .env("ONE_DEVICE_STORE", dir.join("missing-store.json"))
        .env("GALION_MIND", dead_at())
        .env("GALION_EAR", dead_at())
        .env("GALION_EYE", dead_at())
        .env("GALION_MEMORY", dead_at())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start galion-one");
    let _guard = Kill(child, dir.clone());

    let start = Instant::now();
    while get(one_at(), "/health", "").is_none() {
        assert!(start.elapsed() < Duration::from_secs(10), "no /health");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        get(one_at(), "/health", "").unwrap(),
        (200, "ok\n".to_string())
    );

    // ONE-PROBE: the `one` pulse dials our own bind and comes up.
    let mut awake = String::new();
    while start.elapsed() < Duration::from_secs(10) {
        awake = get(one_at(), "/api/v1/awake", "").unwrap().1;
        if awake.contains(r#"{"id":"one","ok":true"#) {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(awake.contains(r#"{"id":"one","ok":true"#), "{awake}");

    let link = get(one_at(), "/link", "").unwrap().1;
    assert!(
        link.contains(&format!(r#""mouth":"{}""#, mouth_at())),
        "{link}"
    );

    let (code, services) = get(one_at(), "/api/v1/services", "").unwrap();
    assert_eq!(code, 200);
    assert_eq!(
        services,
        format!(
            r#"{{"services":[{{"id":"hermes-rs","port":{p},"ok":false,"ms":0}},{{"id":"openmind-memory","port":{p},"ok":false,"ms":0}},{{"id":"link-watch-rs","port":null,"ok":false,"ms":0}}]}}"#,
            p = PORTS.port(2)
        )
    );

    // Fail closed: no store, so no cookie and no forged cookie is accepted.
    assert_eq!(get(one_at(), "/me", "").unwrap().1, r#"{"you":false}"#);
    for path in [
        "/api/v1/analytics",
        "/session",
        "/files",
        "/model",
        "/models",
    ] {
        for cookie in ["", "Cookie: galion_admin=forged\r\n"] {
            let (code, body) = get(one_at(), path, cookie).unwrap();
            assert_eq!(
                (code, body.as_str()),
                (401, r#"{"error":"pair"}"#),
                "{path}"
            );
        }
    }
    assert_eq!(get(one_at(), "/nope", "").unwrap().0, 404);
    let (code, body) = get(mouth_at(), "/health", "").unwrap();
    assert_eq!(code, 503, "{body}");
}
