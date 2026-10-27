// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Real-life test for docs/real-tests/mouth-honest-stub-rs.md (swarm slot 291,
// ports 21600-21609). Starts the real `galion-mouth-stub` binary (no keep-warm,
// no model load) and proves:
//
//   * GET /health -> 503 JSON with "voice not deployed", never "loading"/
//     "chatterbox";
//   * POST /synthesize -> 503 with the same reason (no audio);
//   * body is constant over several seconds (not a warm-up);
//   * unknown paths 404;
//   * process has no keep-warm side effects (stub has none to begin with).
//
// Skipped unless GALION_REAL=1.
//
//   GALION_REAL=1 cargo test -p galion-one --release --test real_mouth_honest_stub -- --nocapture

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:21600";
const ALT: &str = "127.0.0.1:21601";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(addr: &str, method: &str, path: &str, body: &[u8]) -> Option<(u16, Vec<u8>)> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).ok()?;
    s.write_all(body).ok()?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let code = String::from_utf8_lossy(&raw[..end])
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((code, raw[end..].to_vec()))
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn start_stub(bind: &str) -> Kill {
    let bin = std::env::var("GALION_MOUTH_STUB_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-mouth-stub").to_string());
    let child = Command::new(&bin)
        .args(["--bind", bind])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn galion-mouth-stub ({bin}): {e}"));
    let guard = Kill(child);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if TcpStream::connect(bind).is_ok() {
            return guard;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(guard);
    panic!("galion-mouth-stub did not bind {bind}");
}

fn assert_honest(tag: &str, body: &str) {
    assert!(
        body.contains(r#""status":"unavailable""#),
        "{tag}: missing unavailable: {body}"
    );
    assert!(
        body.contains("voice not deployed"),
        "{tag}: missing voice not deployed: {body}"
    );
    assert!(
        !body.contains("loading") && !body.contains("chatterbox") && !body.contains("cuda"),
        "{tag}: still lying: {body}"
    );
}

#[test]
fn real_mouth_honest_stub() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }

    let _stub = start_stub(BIND);

    // A: /health is 503 honest and constant over 4 s.
    let first = request(BIND, "GET", "/health", b"").expect("health A");
    assert_eq!(first.0, 503, "A status");
    let first_body = text(&first.1);
    assert_honest("A", &first_body);
    eprintln!("[A] GET /health -> {} {}", first.0, first_body);
    for i in 0..4 {
        std::thread::sleep(Duration::from_secs(1));
        let again = request(BIND, "GET", "/health", b"").expect("health poll");
        assert_eq!(
            again, first,
            "A poll {i}: body changed (would look like loading)"
        );
    }

    // B: /synthesize is 503 with same reason, no WAV, fast.
    let t = Instant::now();
    let (code, body) = request(
        BIND,
        "POST",
        "/synthesize",
        br#"{"text":"hello from slot 291"}"#,
    )
    .expect("synth");
    let ms = t.elapsed().as_millis();
    let body_s = text(&body);
    assert_eq!(code, 503, "B status {body_s}");
    assert_honest("B", &body_s);
    assert!(!body.starts_with(b"RIFF"), "B must not return audio");
    assert!(ms < 2000, "B stub must fail fast, took {ms} ms");
    eprintln!("[B] POST /synthesize -> {code} {body_s} in {ms} ms");

    // C: unknown path 404.
    let (code, body) = request(BIND, "GET", "/nope", b"").expect("404");
    assert_eq!(code, 404);
    assert!(text(&body).contains("not found"), "{}", text(&body));
    eprintln!("[C] GET /nope -> {code}");

    // D: second bind via env GALION_MOUTH_STUB_BIND (alt port).
    let bin = std::env::var("GALION_MOUTH_STUB_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-mouth-stub").to_string());
    let child = Command::new(&bin)
        .env("GALION_MOUTH_STUB_BIND", ALT)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn alt");
    let _alt = Kill(child);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if TcpStream::connect(ALT).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let (code, body) = request(ALT, "GET", "/health", b"").expect("alt health");
    assert_eq!(code, 503);
    assert_honest("D", &text(&body));
    eprintln!("[D] env bind {ALT} GET /health -> {code} {}", text(&body));

    eprintln!("real_mouth_honest_stub: all cases A-D ok");
}
