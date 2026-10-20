// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test: start the real galion-voice-serve binary on 127.0.0.1:19063
//! (slot 06 range), send a real WAV in, get text out, send text in, get a WAV
//! out, then kill the process. Runs only with GALION_REAL=1 and the models in
//! GALION_VOICE_MODELS (default /workspace/oom-models; see
//! scripts/voice/fetch-models.sh).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:19063";

fn http(method: &str, path: &str, ctype: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(ADDR).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
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

#[test]
fn real_wav_in_text_out_and_text_in_wav_out() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let models =
        std::env::var("GALION_VOICE_MODELS").unwrap_or_else(|_| "/workspace/oom-models".into());
    let mut child = Command::new(env!("CARGO_BIN_EXE_galion-voice-serve"))
        .args(["--bind", ADDR, "--models", &models])
        .env("RAYON_NUM_THREADS", "2")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn galion-voice-serve");
    let t = Instant::now();
    let mut up = false;
    while t.elapsed() < Duration::from_secs(30) {
        if TcpStream::connect(ADDR).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let result = std::panic::catch_unwind(|| {
        assert!(up, "service did not start");
        let (code, body) = http("GET", "/health", "text/plain", b"");
        assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
        let wav = std::fs::read(format!("{models}/whisper-tiny.en/samples_jfk.wav")).unwrap();
        let (code, body) = http("POST", "/v1/audio/transcriptions", "audio/wav", &wav);
        let text = String::from_utf8_lossy(&body).to_string();
        assert_eq!(code, 200, "{text}");
        assert!(
            text.contains("ask not what your country can do for you"),
            "{text}"
        );
        let (code, body) = http(
            "POST",
            "/synthesize",
            "application/json",
            br#"{"text":"Hello from the Galion mouth."}"#,
        );
        assert_eq!(code, 200);
        assert_eq!(&body[..4], b"RIFF");
        assert!(body.len() > 44 + 24_000, "under half a second of audio");
    });
    let _ = child.kill();
    let _ = child.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
