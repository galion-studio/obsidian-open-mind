// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! GALION_REAL=1 real tests for galion-stt (box ports 19651-19653).
//!
//! Needs the native engine and a short English 16 kHz WAV:
//! - `GALION_STT_LIB_DIR`   dir with libmoonshine.so + libonnxruntime.so.1 (pinned sha256)
//! - `GALION_STT_MODEL_DIR` dir with the 8 Tiny Streaming files (pinned sha256)
//! - `GALION_STT_WAV`       the public-domain JFK clip ("ask not what your country ...")
//!
//! Run: `GALION_REAL=1 GALION_STT_LIB_DIR=.. GALION_STT_MODEL_DIR=.. GALION_STT_WAV=..
//! cargo test --release -p galion-stt --test real_stt -- --test-threads 1`

use futures_util::{SinkExt, StreamExt};
use galion_stt::audio::{f32_to_pcm16le, parse_wav, SAMPLE_RATE};
use galion_stt::engine::{EngineConfig, LineTracker, Transcriber};
use galion_stt::ffi::Api;
use galion_stt::models;
use galion_stt::protocol::ServerMsg;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

const BIN: &str = env!("CARGO_BIN_EXE_galion-stt");

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var(k)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Returns (lib, model, wav) or None when the real test is not enabled.
fn setup() -> Option<(PathBuf, PathBuf, PathBuf)> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (see file header)");
        return None;
    }
    Some((
        env_path("GALION_STT_LIB_DIR").expect("GALION_STT_LIB_DIR"),
        env_path("GALION_STT_MODEL_DIR").expect("GALION_STT_MODEL_DIR"),
        env_path("GALION_STT_WAV").expect("GALION_STT_WAV"),
    ))
}

fn wav_samples(p: &PathBuf) -> Vec<f32> {
    let w = parse_wav(&std::fs::read(p).expect("read wav")).expect("parse wav");
    assert_eq!(w.sample_rate, SAMPLE_RATE);
    w.samples
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const JFK_REF: &str = "and so my fellow americans ask not what your country can do for you ask what you can do for your country";

/// Word error rate (Levenshtein over words) against the reference.
fn wer(reference: &str, hyp: &str) -> f64 {
    let r: Vec<&str> = reference.split_whitespace().collect();
    let h: Vec<&str> = hyp.split_whitespace().collect();
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1; h.len() + 1];
        for (j, hw) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(rw != hw);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

/// Tiny model, streamed: the exact words shift a little with chunk timing
/// (e.g. "America" for "Americans"), so the bar is WER <= 25 % plus the
/// anchor words.
fn assert_jfk(text: &str) {
    let t = norm(text);
    let w = wer(JFK_REF, &t);
    eprintln!("WER vs reference: {:.1} %", w * 100.0);
    assert!(w <= 0.25, "WER {w:.3} too high for {t:?}");
    for k in ["fellow", "ask not", "country"] {
        assert!(t.contains(k), "missing {k:?} in transcript {t:?}");
    }
}

#[test]
fn wer_math() {
    assert_eq!(wer("a b c d", "a b c d"), 0.0);
    assert_eq!(wer("a b c d", "a x c"), 0.5);
}

#[test]
fn real_engine_transcribes_jfk() {
    let Some((lib, model, wav)) = setup() else {
        return;
    };
    // Same as the service: single-threaded ORT (see main.rs `ort_threads`).
    std::env::set_var("MOONSHINE_ORT_SINGLE_THREAD", "1");
    models::verify_all(&lib, &model).expect("pinned files");
    let api = Arc::new(Api::open(&lib.join("libmoonshine.so")).expect("dlopen"));
    let mut tr = Transcriber::load(api, &model, &EngineConfig::default()).expect("load");
    let samples = wav_samples(&wav);
    let mut st = tr.open_stream().expect("stream");
    let mut tracker = LineTracker::default();
    let mut texts = Vec::new();
    for (i, c) in samples.chunks(1600).enumerate() {
        st.add_audio(c, SAMPLE_RATE).unwrap();
        if i % 3 == 2 {
            for ev in tracker.update(&st.transcribe(false).unwrap(), false) {
                if let ServerMsg::Final { text, .. } = ev {
                    texts.push(text);
                }
            }
        }
    }
    for ev in tracker.update(&st.finish().unwrap(), true) {
        if let ServerMsg::Final { text, .. } = ev {
            texts.push(text);
        }
    }
    let all = texts.join(" ");
    eprintln!("engine transcript: {all}");
    assert!(tracker.partials > 0, "no partials");
    assert_jfk(&all);
}

struct Svc(Child);
impl Drop for Svc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut b = String::new();
    s.read_to_string(&mut b).ok()?;
    let code = b.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, b.split("\r\n\r\n").nth(1).unwrap_or("").to_string()))
}

fn start(port: u16, lib: &PathBuf, model: &PathBuf) -> Svc {
    assert!((19651..=19653).contains(&port));
    let child = Command::new(BIN)
        .args(["serve", "--bind", &format!("127.0.0.1:{port}")])
        .arg("--lib-dir")
        .arg(lib)
        .arg("--model-dir")
        .arg(model)
        .args(["--max-sessions", "2"])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn");
    let svc = Svc(child);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(60) {
        if let Some((200, _)) = http_get(port, "/health") {
            return svc;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("service did not come up on {port}");
}

type Events = Vec<(u64, serde_json::Value)>;

/// Stream `samples` at `speed` x real time; returns events (ms since start)
/// and the close code.
async fn ws_session(port: u16, samples: &[f32], speed: f64) -> (Events, Option<u16>, u64) {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/stream"))
        .await
        .expect("connect");
    let (mut tx, mut rx) = ws.split();
    let frames: Vec<Vec<u8>> = samples.chunks(320).map(f32_to_pcm16le).collect();
    let t0 = Instant::now();
    let sender = tokio::spawn(async move {
        for (i, f) in frames.into_iter().enumerate() {
            let due = t0 + Duration::from_secs_f64(i as f64 * 0.02 / speed);
            tokio::time::sleep_until(due.into()).await;
            if tx.send(Message::Binary(f.into())).await.is_err() {
                return 0;
            }
        }
        let at = t0.elapsed().as_millis() as u64;
        let _ = tx.send(Message::Text(r#"{"type":"stop"}"#.into())).await;
        // keep tx alive until the reader finishes
        tokio::time::sleep(Duration::from_secs(20)).await;
        at
    });
    let mut ev = Vec::new();
    let mut code = None;
    while let Ok(Some(m)) = tokio::time::timeout(Duration::from_secs(30), rx.next()).await {
        match m {
            Ok(Message::Text(t)) => ev.push((
                t0.elapsed().as_millis() as u64,
                serde_json::from_str(t.as_str()).unwrap(),
            )),
            Ok(Message::Close(f)) => {
                code = f.map(|f| u16::from(f.code));
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    sender.abort();
    let stop_at = 0;
    (ev, code, stop_at)
}

fn finals_text(ev: &Events) -> String {
    ev.iter()
        .filter(|(_, v)| v["type"] == "final")
        .map(|(_, v)| v["text"].as_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn real_ws_stream_jfk_and_caps() {
    let Some((lib, model, wav)) = setup() else {
        return;
    };
    let port = 19651;
    let _svc = start(port, &lib, &model);
    let (code, body) = http_get(port, "/health").unwrap();
    assert_eq!(code, 200);
    let h: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(h["status"], "ok");
    assert_eq!(h["sessions_max"], 2);
    let samples = wav_samples(&wav);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        // 1) real-time session: partials arrive, finals match the reference,
        //    lines are numbered from 0 upwards, normal close 1000.
        let (ev, code, _) = ws_session(port, &samples, 1.0).await;
        let first_partial = ev.iter().find(|(_, v)| v["type"] == "partial").map(|e| e.0);
        eprintln!(
            "ws first partial at {first_partial:?} ms; {} events",
            ev.len()
        );
        assert!(first_partial.is_some(), "no partial");
        assert_eq!(code, Some(1000));
        let mut last_line = 0;
        for (_, v) in &ev {
            assert_ne!(v["type"], "error", "unexpected error {v}");
            let l = v["line"].as_u64().unwrap();
            assert!(l >= last_line, "line numbers went backwards");
            last_line = l;
        }
        let text = finals_text(&ev);
        eprintln!("ws transcript: {text}");
        assert_jfk(&text);

        // 2) session cap: hold two sessions open, the third gets "busy" + 1013.
        let short: Vec<f32> = samples[..SAMPLE_RATE as usize * 4].to_vec();
        let s1 = tokio::spawn({
            let s = short.clone();
            async move { ws_session(port, &s, 1.0).await }
        });
        let s2 = tokio::spawn({
            let s = short.clone();
            async move { ws_session(port, &s, 1.0).await }
        });
        tokio::time::sleep(Duration::from_millis(800)).await;
        let (ev3, code3, _) = ws_session(port, &short[..3200], 1.0).await;
        assert_eq!(code3, Some(1013), "third session should be refused");
        assert!(ev3[0].1["message"].as_str().unwrap().starts_with("busy"));
        let (a, b) = (s1.await.unwrap(), s2.await.unwrap());
        assert_eq!(a.1, Some(1000));
        assert_eq!(b.1, Some(1000));

        // 3) bad frame (odd length) -> non-fatal error, session still closes 1000.
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/stream"))
            .await
            .unwrap();
        let (mut tx, mut rx) = ws.split();
        tx.send(Message::Binary(vec![0u8; 641].into()))
            .await
            .unwrap();
        tx.send(Message::Text(r#"{"type":"stop"}"#.into()))
            .await
            .unwrap();
        let mut saw_err = false;
        let mut close = None;
        while let Some(Ok(m)) = rx.next().await {
            match m {
                Message::Text(t) => saw_err |= t.as_str().contains("\"error\""),
                Message::Close(f) => {
                    close = f.map(|f| u16::from(f.code));
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_err);
        assert_eq!(close, Some(1000));
    });
    let (_, body) = http_get(port, "/health").unwrap();
    let h: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(h["sessions_active"], 0, "engines returned to the pool");
    assert!(h["sessions_refused"].as_u64().unwrap() >= 1);
}

#[test]
fn real_refuses_non_loopback_bind() {
    if setup().is_none() {
        return;
    }
    let out = Command::new(BIN)
        .args(["serve", "--bind", "0.0.0.0:19652"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("loopback only"));
}
