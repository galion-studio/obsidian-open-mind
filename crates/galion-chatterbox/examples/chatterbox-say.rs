// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! chatterbox-say MODEL_DIR "text" OUT.wav [--ref voice.wav] [--seed N]
//! [--steps N] [--exaggeration X] [--cfg X]
//! [--t3 q8|f32] [--prompt-seconds S]
//! CHATTERBOX_PROF=1 prints where the time went.

use std::path::PathBuf;
use std::time::Instant;

use galion_chatterbox::{Chatterbox, GenerateOptions};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!(
            "usage: chatterbox-say MODEL_DIR TEXT OUT.wav [--ref voice.wav] [--seed N] [--steps N]"
        );
        std::process::exit(2);
    }
    let flag = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let t = Instant::now();
    let lo = galion_chatterbox::LoadOptions {
        t3_weights: flag("--t3")
            .map(|s| galion_chatterbox::T3Weights::parse(&s).expect("--t3"))
            .unwrap_or_default(),
    };
    let m = Chatterbox::load_with(&PathBuf::from(&args[0]), &lo).unwrap_or_else(|e| {
        eprintln!("load: {e}");
        std::process::exit(1)
    });
    eprintln!(
        "loaded in {} ms (t3 weights {})",
        t.elapsed().as_millis(),
        lo.t3_weights.as_str()
    );
    let mut o = GenerateOptions {
        seed: flag("--seed").and_then(|s| s.parse().ok()),
        ..Default::default()
    };
    if let Some(s) = flag("--steps").and_then(|s| s.parse().ok()) {
        o.flow_steps = s;
    }
    if let Some(s) = flag("--exaggeration").and_then(|s| s.parse().ok()) {
        o.exaggeration = s;
    }
    if let Some(s) = flag("--cfg").and_then(|s| s.parse().ok()) {
        o.cfg_weight = s;
    }
    let conds = match flag("--ref") {
        Some(r) => {
            let t = Instant::now();
            let c = m
                .conds_from_wav_file(&PathBuf::from(&r), o.exaggeration)
                .unwrap_or_else(|e| {
                    eprintln!("ref: {e}");
                    std::process::exit(1)
                });
            eprintln!(
                "reference {r}: {} T3 prompt tokens, {} S3Gen prompt tokens, {} ms",
                c.t3.prompt_tokens.len(),
                c.gen.prompt_tokens.len(),
                t.elapsed().as_millis()
            );
            c
        }
        None => m.builtin.clone(),
    };
    let conds = match flag("--prompt-seconds").and_then(|s| s.parse::<f32>().ok()) {
        Some(s) => conds.with_gen_prompt_limit(s),
        None => conds,
    };
    let t = Instant::now();
    let (wav, tm) = m.generate(&args[1], &conds, &o).unwrap_or_else(|e| {
        eprintln!("generate: {e}");
        std::process::exit(1)
    });
    let secs = wav.len() as f64 / 24000.0;
    let wall = t.elapsed().as_secs_f64();
    eprintln!(
        "{} speech tokens, audio {:.2} s, wall {:.2} s (t3 {} ms, flow {} ms, hift {} ms), RTF {:.2}",
        tm.speech_tokens, secs, wall, tm.t3_ms, tm.flow_ms, tm.hift_ms, wall / secs.max(1e-3)
    );
    if galion_chatterbox::prof::enabled() {
        eprint!("{}", galion_chatterbox::prof::report());
    }
    std::fs::write(&args[2], galion_chatterbox::wav::encode_pcm16(&wav, 24000)).expect("write");
}
