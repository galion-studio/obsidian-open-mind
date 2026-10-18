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
    frames.extend(std::iter::repeat_n(vec![0.0f32; spec.frame_samples], 40));
    for (i, c) in frames.iter().enumerate() {
        let mut samples = galion_voice_cpu::resample::f32_to_i16(c);
        samples.resize(spec.frame_samples, 0);
        let out = s
            .tick(&galion_voice_core::audio::Frame {
                seq: i as u64,
                samples,
            })
            .map_err(|e| e.to_string())?;
        for e in out.events {
            if let SessionEvent::Text(t) = e {
                println!("session text @frame {i}: {t}");
            }
        }
    }
    let tm = s.backend().last_timing;
    println!(
        "pipeline last turn: {tm:?}; session {:?}; VmHWM {} kB",
        s.stats(),
        vm_hwm_kb()
    );
    Ok(())
}
