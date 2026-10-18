// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Whisper-tiny CPU benchmark: real-time factor and peak RSS.
//!
//! usage: stt-bench <model-dir> <wav> [threads]
//! Model dir holds model.safetensors, config.json, tokenizer.json, melfilters.bytes.

use std::path::Path;
use std::time::Instant;

use galion_voice_core::audio::FrameSpec;
use galion_voice_core::persona::StaticPersona;
use galion_voice_core::session::{Session, SessionConfig, SessionEvent};
use galion_voice_cpu::pipeline::mocks::{BeepTts, EchoModel};
use galion_voice_cpu::pipeline::CpuPipelineBackend;
use galion_voice_cpu::resample::{i16_to_f32, linear};
use galion_voice_cpu::stt::SpeechToText;
use galion_voice_cpu::wav;
use galion_voice_cpu::whisper_stt::{EncoderContext, WhisperStt};

fn vm_hwm_kb() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM"))
                .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string())
        })
        .unwrap_or_else(|| "?".into())
}

fn main() -> Result<(), String> {
    let mut a = std::env::args().skip(1);
    let dir = a.next().ok_or("usage: stt-bench <model-dir> <wav>")?;
    let wavp = a.next().ok_or("usage: stt-bench <model-dir> <wav>")?;
    let w = wav::parse(&std::fs::read(&wavp).map_err(|e| e.to_string())?)?;
    let pcm = linear(&i16_to_f32(&w.samples), w.sample_rate, 16_000);
    let secs = pcm.len() as f64 / 16_000.0;
    println!(
        "audio: {secs:.2} s; candle threads: {}",
        candle_core::utils::get_num_threads()
    );

    let t = Instant::now();
    let mut stt = WhisperStt::load(Path::new(&dir))?;
    println!(
        "load: {} ms, VmHWM {} kB",
        t.elapsed().as_millis(),
        vm_hwm_kb()
    );

    for (label, ctx, len) in [
        ("full-30s-window", EncoderContext::Full, pcm.len()),
        ("trimmed", EncoderContext::Trimmed, pcm.len()),
        (
            "trimmed-first-4s",
            EncoderContext::Trimmed,
            64_000.min(pcm.len()),
        ),
        (
            "trimmed-first-2s",
            EncoderContext::Trimmed,
            32_000.min(pcm.len()),
        ),
        ("full-first-4s", EncoderContext::Full, 64_000.min(pcm.len())),
        ("full-first-2s", EncoderContext::Full, 32_000.min(pcm.len())),
    ] {
        stt.context = ctx;
        let clip = &pcm[..len];
        let _warm = (label == "full-30s-window").then(|| stt.transcribe(clip));
        let t = Instant::now();
        let text = stt.transcribe(clip)?;
        let el = t.elapsed().as_secs_f64();
        let dur = len as f64 / 16_000.0;
        println!(
            "{label}: {:.0} ms for {dur:.2} s audio -> RTF {:.3}; VmHWM {} kB; text: {text:?}",
            el * 1000.0,
            el / dur,
            vm_hwm_kb()
        );
    }

    // Same model as a voice-core backend: the clip as mic audio at 24 kHz.
    // STT now runs on the backend's worker thread, so we tick in (simulated)
    // real time and record how long each tick takes.
    stt.context = EncoderContext::Trimmed;
    let spec = FrameSpec::MOSHI;
    let mic = linear(&i16_to_f32(&w.samples), w.sample_rate, spec.sample_rate);
    let mut s = Session::new(
        CpuPipelineBackend::new(stt, EchoModel, BeepTts, spec),
        SessionConfig::default(),
    );
    s.start(&StaticPersona::new("Galion", "default"))
        .map_err(|e| e.to_string())?;
    let mut frames: Vec<_> = mic.chunks(spec.frame_samples).map(|c| c.to_vec()).collect();
    let speech_frames = frames.len();
    frames.extend(std::iter::repeat_n(vec![0.0f32; spec.frame_samples], 400));
    let frame_dur = std::time::Duration::from_millis(spec.frame_ms() as u64);
    let mut worst_tick = std::time::Duration::ZERO;
    let mut end_tick = None;
    let mut speech_end_at = None;
    let mut first_audio_at = None;
    for (i, c) in frames.iter().enumerate() {
        let mut samples = galion_voice_cpu::resample::f32_to_i16(c);
        samples.resize(spec.frame_samples, 0);
        let t0 = Instant::now();
        let out = s
            .tick(&galion_voice_core::audio::Frame {
                seq: i as u64,
                samples,
            })
            .map_err(|e| e.to_string())?;
        let took = t0.elapsed();
        worst_tick = worst_tick.max(took);
        for e in &out.events {
            match e {
                SessionEvent::Text(t) => println!("session text @frame {i}: {t}"),
                SessionEvent::UserSpeechEnd => {
                    end_tick = Some(took);
                    speech_end_at = Some(Instant::now());
                }
                _ => {}
            }
        }
        if out.playout.is_some() && first_audio_at.is_none() {
            first_audio_at = Some(Instant::now());
        }
        if i >= speech_frames && first_audio_at.is_some() && !s.backend().busy() {
            break;
        }
        // Real time only after the speech, to keep the bench short.
        if i >= speech_frames {
            std::thread::sleep(frame_dur.saturating_sub(took));
        }
    }
    let tm = s.backend().last_timing;
    let reply_ms = match (speech_end_at, first_audio_at) {
        (Some(a), Some(b)) => b.duration_since(a).as_millis().to_string(),
        _ => "?".into(),
    };
    println!(
        "pipeline last turn: {tm:?}; end-of-turn tick {} us; worst tick {} us; \
         speech end -> first reply frame {reply_ms} ms; session {:?}; VmHWM {} kB",
        end_tick.map_or(0, |d| d.as_micros()),
        worst_tick.as_micros(),
        s.stats(),
        vm_hwm_kb()
    );
    Ok(())
}
