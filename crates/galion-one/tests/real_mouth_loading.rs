// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Real-life test for docs/ops/mouth-loading.md (swarm slot 181,
// ports 20510-20519). Starts the real galion-one binary on spare loopback
// ports and shows why its mouth listener answers 503 "loading" for days:
//
//   * this branch's binary (integration-4: honest /health) says 503
//     "unavailable" with a reason when no speech backend is set, passes a
//     remote voice service's /health through when GALION_MOUTH is set, and
//     says 503 "unreachable" when that service is down;
//   * the binary that is live on the VPS since 2026-10-21 08:39 +02:00
//     (swarm/one-no-shellout 40454f9, sha256 24cb4976...; optional, set
//     GALION_ONE_LIVE_BIN to a copy of it) answers a hard-coded 503
//     "loading" that never changes, and keeps answering it even when
//     GALION_MOUTH points at a working mouth that it does forward
//     /synthesize to.
//
// A small in-test fake voice service on 20512 stands in for
// galion-voice-serve (no models needed). Skipped unless GALION_REAL=1.
//
//   GALION_REAL=1 GALION_ONE_LIVE_BIN=/path/galion-one-live \
//     cargo test -p galion-one --release --test real_mouth_loading -- --nocapture

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FAKE: &str = "127.0.0.1:20512";
const DEAD: &str = "127.0.0.1:20513";
const FAKE_HEALTH: &str = r#"{"ok":true,"status":"ok","provider":"fake-voice-serve","slot":181}"#;
const LIVE_LOADING: &str =
    r#"{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}"#;

struct Kill(Child, PathBuf);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn request(addr: &str, method: &str, path: &str, body: &[u8]) -> Option<(u16, Vec<u8>)> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(15))).ok()?;
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

/// Fake galion-voice-serve: GET /health 200, POST /synthesize 200 audio/wav.
fn start_fake(hits: Arc<AtomicUsize>) {
    let listener = TcpListener::bind(FAKE).expect("bind fake mouth 20512");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let hits = hits.clone();
            std::thread::spawn(move || {
                let mut s = stream;
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut tmp) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                    if buf.len() > 65536 {
                        return;
                    }
                }
                hits.fetch_add(1, Ordering::SeqCst);
                let first = text(&buf).lines().next().unwrap_or("").to_string();
                let (ctype, body): (&str, Vec<u8>) = if first.starts_with("GET /health") {
                    ("application/json", FAKE_HEALTH.as_bytes().to_vec())
                } else if first.starts_with("POST /synthesize") {
                    let pcm = vec![0u8; 4800]; // 0.1 s of silence, 24 kHz s16le
                    let mut w = b"RIFF".to_vec();
                    w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
                    w.extend_from_slice(b"WAVEfmt ");
                    w.extend_from_slice(&16u32.to_le_bytes());
                    w.extend_from_slice(&1u16.to_le_bytes());
                    w.extend_from_slice(&1u16.to_le_bytes());
                    w.extend_from_slice(&24000u32.to_le_bytes());
                    w.extend_from_slice(&48000u32.to_le_bytes());
                    w.extend_from_slice(&2u16.to_le_bytes());
                    w.extend_from_slice(&16u16.to_le_bytes());
                    w.extend_from_slice(b"data");
                    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
                    w.extend_from_slice(&pcm);
                    ("audio/wav", w)
                } else {
                    ("text/plain", b"not found\n".to_vec())
                };
                let code = if ctype == "text/plain" {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                let head = format!(
                    "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            });
        }
    });
}

fn start_one(bin: &str, tag: &str, one: &str, mouth: &str, galion_mouth: Option<&str>) -> Kill {
    let dir = std::env::temp_dir().join(format!("one-181-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home/one")).unwrap();
    std::fs::write(dir.join("home/one/index.html"), "<html>fx</html>").unwrap();
    std::fs::write(dir.join("hermes.env"), "").unwrap();
    let mut cmd = Command::new(bin);
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir.join("home"))
        .env("ONE_NO_WARM", "1")
        .env("ONE_BIND", one)
        .env("ONE_MOUTH_BIND", mouth)
        .env("GALION_HOME", dir.join("home"))
        .env("ONE_HERMES_DIR", dir.join("hermes"))
        .env("ONE_HERMES_ENV", dir.join("hermes.env"))
        .env("ONE_DEVICE_STORE", dir.join("missing-store.json"))
        .env("GALION_MIND", DEAD)
        .env("GALION_EAR", DEAD)
        .env("GALION_EYE", DEAD)
        .env("GALION_MEMORY", DEAD)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(m) = galion_mouth {
        cmd.env("GALION_MOUTH", m);
    }
    let child = cmd.spawn().expect("start galion-one");
    let guard = Kill(child, dir);
    let t = Instant::now();
    while request(mouth, "GET", "/health", b"").is_none()
        || request(one, "GET", "/health", b"").is_none()
    {
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "{tag}: listeners not up"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("[{tag}] up in {} ms ({bin})", t.elapsed().as_millis());
    guard
}

/// Poll the mouth /health `n` times over `secs`; every answer must be equal.
fn poll_constant(tag: &str, mouth: &str, n: u32, secs: u64) -> (u16, String) {
    let first = request(mouth, "GET", "/health", b"").unwrap();
    for _ in 1..n {
        std::thread::sleep(Duration::from_millis(secs * 1000 / u64::from(n)));
        let again = request(mouth, "GET", "/health", b"").unwrap();
        assert_eq!(again, first, "{tag}: /health changed over time");
    }
    let out = (first.0, text(&first.1));
    eprintln!(
        "[{tag}] GET /health x{n} over {secs}s -> {} {}",
        out.0, out.1
    );
    out
}

fn synth(tag: &str, mouth: &str) -> (u16, Vec<u8>, u128) {
    let t = Instant::now();
    let (code, body) = request(
        mouth,
        "POST",
        "/synthesize",
        br#"{"text":"Hello from slot 181."}"#,
    )
    .unwrap();
    let ms = t.elapsed().as_millis();
    let shown = if body.starts_with(b"RIFF") {
        format!("audio/wav {} B", body.len())
    } else {
        text(&body)
    };
    eprintln!("[{tag}] POST /synthesize -> {code} {shown} in {ms} ms");
    (code, body, ms)
}

#[test]
fn real_mouth_loading_diag() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }
    let hits = Arc::new(AtomicUsize::new(0));
    start_fake(hits.clone());
    let branch = std::env::var("GALION_ONE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-one").to_string());

    // A: no GALION_MOUTH (the VPS unit today): honest, permanent 503.
    {
        let _g = start_one(
            &branch,
            "A-branch-no-mouth",
            "127.0.0.1:20510",
            "127.0.0.1:20511",
            None,
        );
        let (code, body) = poll_constant("A-branch-no-mouth", "127.0.0.1:20511", 6, 6);
        assert_eq!(code, 503);
        assert!(body.contains(r#""status":"unavailable""#), "{body}");
        assert!(body.contains("set GALION_MOUTH"), "{body}");
        assert!(
            !body.contains("loading") && !body.contains("cuda"),
            "{body}"
        );
        let (code, body, ms) = synth("A-branch-no-mouth", "127.0.0.1:20511");
        assert_eq!((code, text(&body).as_str()), (500, r#"{"error":"voice"}"#));
        assert!(ms < 2000, "stub must fail fast, took {ms} ms");
        assert_eq!(hits.load(Ordering::SeqCst), 0, "no remote mouth configured");
    }

    // B: GALION_MOUTH -> working voice service: /health 200 passthrough, audio.
    {
        let before = hits.load(Ordering::SeqCst);
        let _g = start_one(
            &branch,
            "B-branch-remote-up",
            "127.0.0.1:20514",
            "127.0.0.1:20515",
            Some(FAKE),
        );
        let (code, body) = poll_constant("B-branch-remote-up", "127.0.0.1:20515", 3, 2);
        assert_eq!((code, body.as_str()), (200, FAKE_HEALTH));
        let (code, body, _) = synth("B-branch-remote-up", "127.0.0.1:20515");
        assert_eq!(code, 200);
        assert!(
            body.starts_with(b"RIFF") && body.len() == 44 + 4800,
            "wav from fake"
        );
        assert!(
            hits.load(Ordering::SeqCst) >= before + 4,
            "health x3 + synth reached the fake"
        );
    }

    // C: GALION_MOUTH -> nothing listening: 503 "unreachable", not "loading".
    {
        let _g = start_one(
            &branch,
            "C-branch-remote-down",
            "127.0.0.1:20516",
            "127.0.0.1:20517",
            Some(DEAD),
        );
        let (code, body) = poll_constant("C-branch-remote-down", "127.0.0.1:20517", 3, 2);
        assert_eq!(code, 503);
        assert!(
            body.contains(r#""status":"unreachable""#) && body.contains(DEAD),
            "{body}"
        );
        let (code, _, _) = synth("C-branch-remote-down", "127.0.0.1:20517");
        assert_eq!(code, 500);
    }

    // L: the live VPS binary (optional copy), same two set-ups.
    let Ok(live) = std::env::var("GALION_ONE_LIVE_BIN") else {
        eprintln!("[L] GALION_ONE_LIVE_BIN not set: live-binary parity cases skipped");
        return;
    };
    {
        let _g = start_one(
            &live,
            "L1-live-no-mouth",
            "127.0.0.1:20518",
            "127.0.0.1:20519",
            None,
        );
        let (code, body) = poll_constant("L1-live-no-mouth", "127.0.0.1:20519", 6, 6);
        assert_eq!(
            (code, body.as_str()),
            (503, LIVE_LOADING),
            "constant 'loading' body"
        );
        let (code, body, ms) = synth("L1-live-no-mouth", "127.0.0.1:20519");
        assert_eq!((code, text(&body).as_str()), (500, r#"{"error":"voice"}"#));
        assert!(ms < 2000, "{ms} ms");
    }
    {
        let before = hits.load(Ordering::SeqCst);
        let _g = start_one(
            &live,
            "L2-live-remote-up",
            "127.0.0.1:20510",
            "127.0.0.1:20511",
            Some(FAKE),
        );
        let (code, body) = poll_constant("L2-live-remote-up", "127.0.0.1:20511", 3, 2);
        // Root cause 3: a working remote mouth does not clear the live 503.
        assert_eq!((code, body.as_str()), (503, LIVE_LOADING));
        let (code, body, _) = synth("L2-live-remote-up", "127.0.0.1:20511");
        assert_eq!(
            code, 200,
            "live binary forwards /synthesize to GALION_MOUTH"
        );
        assert!(body.starts_with(b"RIFF"));
        let after = hits.load(Ordering::SeqCst);
        assert!(after > before, "/synthesize reached the fake");
        eprintln!(
            "[L2-live-remote-up] fake mouth hits during case: {}",
            after - before
        );
    }
}
