// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life tests of the Chatterbox mouth on CPU (GALION_REAL=1 only; models
//! in GALION_VOICE_MODELS, default /workspace/oom-models):
//! * in process: synthesize a line, transcribe it back with Whisper;
//! * the release/debug binary in `--worker` mode: the legacy worker.py frame
//!   protocol (`R`, request frame, PCM frame, end frame).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use galion_voice_cpu::chatterbox_tts::{ChatterboxConfig, ChatterboxTts};
use galion_voice_cpu::resample::linear;
use galion_voice_cpu::stt::{SpeechToText, TextToSpeech};
use galion_voice_cpu::whisper_stt::WhisperStt;

fn models() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return None;
    }
    Some(PathBuf::from(
        std::env::var("GALION_VOICE_MODELS").unwrap_or_else(|_| "/workspace/oom-models".into()),
    ))
}

#[test]
fn chatterbox_line_round_trips_through_whisper() {
    let Some(m) = models() else { return };
    let mut cfg = ChatterboxConfig::default();
    cfg.options.seed = Some(1);
    cfg.options.flow_steps = 5;
    cfg.prompt_seconds = 4.0;
    let mut tts = ChatterboxTts::load(&m.join("chatterbox"), &cfg).expect("chatterbox");
    let pcm = tts
        .synthesize(
            "Hello, I am Samantha. It is lovely to meet you.",
            "samantha",
        )
        .expect("synth");
    let secs = pcm.len() as f32 / 24_000.0;
    assert!((1.0..10.0).contains(&secs), "audio {secs} s");
    let f: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
    let mut stt = WhisperStt::load(&m.join("whisper-tiny.en")).expect("whisper");
    let text = stt.transcribe(&linear(&f, 24_000, 16_000)).expect("stt");
    eprintln!("heard: {text}");
    let t = text.to_lowercase();
    assert!(t.contains("samantha") && t.contains("meet you"), "{text}");
}

#[test]
fn worker_frame_protocol() {
    let Some(m) = models() else { return };
    let bin = env!("CARGO_BIN_EXE_galion-voice-serve");
    let mut child = Command::new(bin)
        .args([
            "--worker",
            "--tts",
            "chatterbox",
            "--flow-steps",
            "5",
            "--prompt-seconds",
            "4",
        ])
        .args(["--models", m.to_str().unwrap(), "--seed", "3"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn");
    let mut out = child.stdout.take().unwrap();
    let mut inp = child.stdin.take().unwrap();
    let mut r = [0u8; 1];
    out.read_exact(&mut r).unwrap();
    assert_eq!(&r, b"R");
    let read_frame = |out: &mut std::process::ChildStdout| {
        let mut l = [0u8; 4];
        out.read_exact(&mut l).unwrap();
        let mut b = vec![0u8; u32::from_le_bytes(l) as usize];
        out.read_exact(&mut b).unwrap();
        b
    };
    // Too short a request: only the end frame.
    inp.write_all(&1u32.to_le_bytes()).unwrap();
    inp.write_all(&[0]).unwrap();
    inp.flush().unwrap();
    assert!(read_frame(&mut out).is_empty());
    // voice byte 1 = jarvis, then text.
    let mut req = vec![1u8];
    req.extend_from_slice(b"All systems are running.");
    inp.write_all(&(req.len() as u32).to_le_bytes()).unwrap();
    inp.write_all(&req).unwrap();
    inp.flush().unwrap();
    let pcm = read_frame(&mut out);
    assert!(pcm.len() > 24_000, "{} bytes of PCM", pcm.len());
    assert_eq!(pcm.len() % 2, 0);
    assert!(read_frame(&mut out).is_empty());
    drop(inp);
    let _ = child.wait();
}
