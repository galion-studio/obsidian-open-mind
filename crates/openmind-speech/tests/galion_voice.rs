// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Fixtures captured from a real galion-voice-serve run on the box
//! (2026-10-20 19:13 CEST (+02:00), 127.0.0.1:19060, whisper-tiny.en +
//! KittenTTS nano 0.8). The ear answer has the whisper-server `text` shape,
//! and the mouth answer is a PCM WAV, so this adapter reads both unchanged.

use openmind_speech::{Engine, SpeechProvider, TranscriptSource};

#[test]
fn galion_voice_ear_answer_parses_as_whisper_server_json() {
    let stt =
        SpeechProvider::new("galion-voice", "http://127.0.0.1:8003", Engine::WhisperCpp).unwrap();
    let body = include_str!("../fixtures/galion_voice_stt.json");
    let t = stt.parse_whisper(body).unwrap();
    assert_eq!(t.source, TranscriptSource::ServerJson);
    assert!(t.text.starts_with("And so my fellow Americans ask not"));
    assert!(t.text.ends_with("what you can do for your country."));
    assert!(t.segments.is_empty());
}

#[test]
fn galion_voice_mouth_answer_is_24k_mono_pcm() {
    let tts = SpeechProvider::new("galion-voice", "http://127.0.0.1:8003", Engine::Piper).unwrap();
    let w = tts
        .parse_piper_wav(include_bytes!("../fixtures/galion_voice_tts.wav"))
        .unwrap();
    assert_eq!((w.audio_format, w.channels, w.bits_per_sample), (1, 1, 16));
    assert_eq!(w.sample_rate, 24_000);
    assert!(
        w.duration_ms > 300 && w.duration_ms < 3_000,
        "{}",
        w.duration_ms
    );
}
