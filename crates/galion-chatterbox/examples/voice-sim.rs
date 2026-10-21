// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! voice-sim MODEL_DIR a.wav b.wav [c.wav ...]
//! Cosine similarity of speaker embeddings (Chatterbox voice encoder and
//! CAM++ x-vector) between the first clip and each other clip: evidence that
//! reference-clip conditioning moves the output towards the reference voice.

use std::path::Path;

use galion_chatterbox::{dsp, wav, Chatterbox, S3_SR};

fn cos(a: &[f32], b: &[f32]) -> f32 {
    let d: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    d / (na * nb).max(1e-9)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: voice-sim MODEL_DIR a.wav b.wav [...]");
        std::process::exit(2);
    }
    let m = Chatterbox::load(Path::new(&args[0])).expect("load");
    let emb = |p: &str| {
        let a = wav::read(&std::fs::read(p).expect("read")).expect("wav");
        let x = dsp::resample(&a.samples, a.sample_rate, S3_SR);
        (
            m.ve.embed(&x).expect("ve"),
            m.campplus.embed(&x).expect("cam"),
        )
    };
    let (va, ca) = emb(&args[1]);
    for p in &args[2..] {
        let (vb, cb) = emb(p);
        println!(
            "{} vs {}: voice-encoder cos {:.3}, cam++ cos {:.3}",
            args[1],
            p,
            cos(&va, &vb),
            cos(&ca, &cb)
        );
    }
}
