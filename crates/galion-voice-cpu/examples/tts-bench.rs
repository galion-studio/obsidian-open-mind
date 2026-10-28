// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! KittenTTS nano CPU benchmark: G2P -> candle inference, RTF and peak RSS,
//! WAV output, and (with `--features whisper`) a Whisper round trip as an
//! intelligibility check.
//!
//! usage: tts-bench <kitten-dir> <cmudict> <out-dir> [whisper-dir] [voice]

use std::path::Path;
use std::time::Instant;

use galion_voice_cpu::g2p::{kitten_ids, kitten_symbol_table};
use galion_voice_cpu::kitten::KittenTts;
use galion_voice_cpu::resample::f32_to_i16;
#[cfg(feature = "whisper")]
use galion_voice_cpu::resample::linear;
use galion_voice_cpu::wav;

fn vm_hwm_kb() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM"))
                .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
        })
        .unwrap_or_else(|| "?".into())
}

const SENTENCES: [&str; 4] = [
    "Hello, world.",
    "Obsidian Open Mind, made by Galion, is listening.",
    "This high quality speech model works without a graphics card.",
    "And so my fellow Americans, ask not what your country can do for you, ask what you can do for your country.",
];

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        return Err(
            "usage: tts-bench <kitten-dir> <cmudict> <out-dir> [whisper-dir] [voice]".into(),
        );
    }
    let out_dir = Path::new(&a[2]);
    let voice = a.get(4).map_or("jasper", String::as_str);
    println!("candle threads: {}", candle_core::utils::get_num_threads());
    let t = Instant::now();
    let mut tts = KittenTts::load(Path::new(&a[0]), Path::new(&a[1]))?;
    println!(
        "load (model mmap + voices + CMUdict): {} ms, VmHWM {} kB, voices {:?}",
        t.elapsed().as_millis(),
        vm_hwm_kb(),
        tts.voice_names()
    );

    #[cfg(feature = "whisper")]
    let mut stt = match a.get(3).filter(|d| !d.is_empty() && *d != "-") {
        Some(d) => Some(galion_voice_cpu::whisper_stt::WhisperStt::load(Path::new(
            d,
        ))?),
        None => None,
    };

    // The official KittenTTS TextCleaner table (178 symbols = the 178 rows
    // of the model's embedding tables).
    let official = kitten_symbol_table();
    let tables = [("official", &official)];

    let _warm = tts.synthesize_f32("Warm up.", voice)?;
    for (ti, (tname, table)) in tables.iter().enumerate() {
        for (i, s) in SENTENCES.iter().enumerate() {
            let t = Instant::now();
            let ipa = tts.ipa(s);
            let g2p_us = t.elapsed().as_micros();
            let ids = kitten_ids(&ipa, table);
            let t = Instant::now();
            let audio = tts.synthesize_ids(&ids, voice, s.chars().count())?;
            let el = t.elapsed().as_secs_f64();
            let secs = audio.len() as f64 / 24_000.0;
            let pcm = f32_to_i16(&audio);
            let path = out_dir.join(format!("kitten-{tname}-{voice}-{i}.wav"));
            std::fs::write(&path, wav::encode(&pcm, 24_000)).map_err(|e| e.to_string())?;
            println!(
                "[{tname}] #{i}: {} tokens -> {secs:.2} s audio in {:.0} ms, RTF {:.3}; VmHWM {} kB; {}",
                ids.len(),
                el * 1000.0,
                el / secs.max(1e-9),
                vm_hwm_kb(),
                path.display()
            );
            if ti == 0 {
                println!("  ipa: {ipa}  (g2p {g2p_us} us)");
            }
            #[cfg(feature = "whisper")]
            if let Some(stt) = stt.as_mut() {
                use galion_voice_cpu::stt::SpeechToText;
                let text = stt.transcribe(&linear(&audio, 24_000, 16_000))?;
                println!("  whisper heard: {text:?}");
            }
        }
    }
    println!("peak VmHWM {} kB", vm_hwm_kb());
    Ok(())
}
