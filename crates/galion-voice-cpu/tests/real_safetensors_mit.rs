// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for AP-1 option C (slot 295): the workspace patches `safetensors` with the clean-room
//! MIT vendor/safetensors-mit. Start the real galion-voice-serve (which loads every model through
//! candle -> safetensors) on 127.0.0.1:21650 (slot 295 range 21650-21659), wait for /health 200, send the
//! JFK WAV for STT and fixed text for TTS, check the answers, then kill the process.
//!
//! GALION_REAL=1 only. Models from GALION_VOICE_MODELS (default /workspace/oom-models).
//! Optional: GALION_ST_WAV_OUT=<path> writes the synthesized WAV; GALION_ST_BASELINE_WAV=<path> asserts
//! the WAV is byte-identical to one produced by a build that used upstream safetensors 0.7.0.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:21650";

fn http(method: &str, path: &str, ctype: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(ADDR).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(180))).unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").expect("head") + 4;
    let code = String::from_utf8_lossy(&buf[9..12]).parse().unwrap();
    (code, buf[end..].to_vec())
}

struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn real_voice_serve_loads_models_through_mit_safetensors() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let models =
        std::env::var("GALION_VOICE_MODELS").unwrap_or_else(|_| "/workspace/oom-models".into());
    let t0 = Instant::now();
    let _child = Kill(
        Command::new(env!("CARGO_BIN_EXE_galion-voice-serve"))
            .args(["--bind", ADDR, "--models", &models])
            .env("RAYON_NUM_THREADS", "2")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn galion-voice-serve"),
    );
    let mut up = false;
    while t0.elapsed() < Duration::from_secs(30) {
        if TcpStream::connect(ADDR).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(up, "service did not bind {ADDR}");
    let (mut code, mut body) = http("GET", "/health", "text/plain", b"");
    while code == 503 && t0.elapsed() < Duration::from_secs(180) {
        std::thread::sleep(Duration::from_millis(250));
        (code, body) = http("GET", "/health", "text/plain", b"");
    }
    let health = String::from_utf8_lossy(&body).to_string();
    assert_eq!(code, 200, "{health}");
    eprintln!("health 200 after {:?}: {health}", t0.elapsed());

    let wav = std::fs::read(format!("{models}/whisper-tiny.en/samples_jfk.wav")).unwrap();
    let t = Instant::now();
    let (code, body) = http("POST", "/v1/audio/transcriptions", "audio/wav", &wav);
    let text = String::from_utf8_lossy(&body).to_string();
    assert_eq!(code, 200, "{text}");
    assert!(
        text.contains("ask not what your country can do for you"),
        "{text}"
    );
    eprintln!("stt {:?}: {text}", t.elapsed());

    let t = Instant::now();
    let (code, out) = http(
        "POST",
        "/synthesize",
        "application/json",
        br#"{"text":"Hello from the Galion mouth."}"#,
    );
    assert_eq!(code, 200);
    assert_eq!(&out[..4], b"RIFF");
    assert!(out.len() > 44 + 24_000, "under half a second of audio");
    eprintln!("tts {:?}: {} bytes", t.elapsed(), out.len());
    if let Ok(p) = std::env::var("GALION_ST_WAV_OUT") {
        std::fs::write(&p, &out).unwrap();
    }
    if let Ok(p) = std::env::var("GALION_ST_BASELINE_WAV") {
        let base = std::fs::read(&p).unwrap();
        assert!(
            base == out,
            "TTS WAV differs from upstream-safetensors baseline {p}"
        );
        eprintln!("tts WAV byte-identical to baseline {p}");
    }
}
