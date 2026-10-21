// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The assembled Chatterbox model: loading a model directory and
//! `generate(text) -> 24 kHz samples`.

use std::path::Path;
use std::time::Instant;

use candle_core::Device;

use crate::campplus::CamPlus;
use crate::conds::Conditionals;
use crate::flow::Flow;
use crate::hift::HiFT;
use crate::rng::Rng;
use crate::s3tok::S3Tokenizer;
use crate::t3::{SampleOptions, T3};
use crate::text::{punc_norm, EnTokenizer};
use crate::ve::VoiceEncoder;

/// Files a model directory must contain (Hugging Face ResembleAI/chatterbox).
pub const MODEL_FILES: [&str; 5] = [
    "t3_cfg.safetensors",
    "s3gen.safetensors",
    "ve.safetensors",
    "tokenizer.json",
    "conds.pt",
];

#[derive(Clone, Debug)]
pub struct GenerateOptions {
    pub exaggeration: f32,
    pub cfg_weight: f32,
    pub temperature: f32,
    pub repetition_penalty: f32,
    pub min_p: f32,
    pub top_p: f32,
    pub max_new_tokens: usize,
    pub flow_steps: usize,
    pub seed: Option<u64>,
}

impl Default for GenerateOptions {
    /// `ChatterboxTTS.generate` defaults (the legacy HTTP server used them).
    fn default() -> Self {
        GenerateOptions {
            exaggeration: 0.5,
            cfg_weight: 0.5,
            temperature: 0.8,
            repetition_penalty: 1.2,
            min_p: 0.05,
            top_p: 1.0,
            max_new_tokens: 1000,
            flow_steps: 10,
            seed: None,
        }
    }
}

/// Timings of one generate call, in milliseconds.
#[derive(Clone, Debug, Default)]
pub struct Timings {
    pub t3_ms: u128,
    pub flow_ms: u128,
    pub hift_ms: u128,
    pub speech_tokens: usize,
}

pub struct Chatterbox {
    pub t3: T3,
    pub flow: Flow,
    pub hift: HiFT,
    pub ve: VoiceEncoder,
    pub s3tok: S3Tokenizer,
    pub campplus: CamPlus,
    pub tokenizer: EnTokenizer,
    /// Built-in voice from conds.pt.
    pub builtin: Conditionals,
}

impl Chatterbox {
    pub fn load(dir: &Path) -> crate::Result<Self> {
        for f in MODEL_FILES {
            if !dir.join(f).is_file() {
                return Err(format!("missing {}", dir.join(f).display()));
            }
        }
        let dev = Device::Cpu;
        let t3vb =
            crate::nnx::load_vb(&[dir.join("t3_cfg.safetensors")], &dev).map_err(crate::e)?;
        let t3 = T3::load(t3vb, 8192).map_err(crate::e)?;
        let s3 = crate::nnx::load_vb(&[dir.join("s3gen.safetensors")], &dev).map_err(crate::e)?;
        let flow = Flow::load(s3.pp("flow")).map_err(crate::e)?;
        let hift = HiFT::load(s3.pp("mel2wav")).map_err(crate::e)?;
        let s3tok = S3Tokenizer::load(s3.pp("tokenizer")).map_err(crate::e)?;
        let campplus = CamPlus::load(s3.pp("speaker_encoder")).map_err(crate::e)?;
        let vevb = crate::nnx::load_vb(&[dir.join("ve.safetensors")], &dev).map_err(crate::e)?;
        let ve = VoiceEncoder::load(vevb).map_err(crate::e)?;
        let tokenizer = EnTokenizer::from_json(
            &std::fs::read_to_string(dir.join("tokenizer.json")).map_err(crate::e)?,
        )?;
        let builtin = Conditionals::load_pt(&dir.join("conds.pt"))?;
        Ok(Chatterbox {
            t3,
            flow,
            hift,
            ve,
            s3tok,
            campplus,
            tokenizer,
            builtin,
        })
    }

    /// Conditionals from a reference WAV file.
    pub fn conds_from_wav_file(
        &self,
        path: &Path,
        exaggeration: f32,
    ) -> crate::Result<Conditionals> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let a = crate::wav::read(&bytes)?;
        Conditionals::from_wav(self, &a.samples, a.sample_rate, exaggeration)
    }

    /// Synthesize one chunk of text with the given voice conditionals.
    pub fn generate(
        &self,
        text: &str,
        conds: &Conditionals,
        o: &GenerateOptions,
    ) -> crate::Result<(Vec<f32>, Timings)> {
        let mut rng = o.seed.map_or_else(Rng::from_entropy, Rng::new);
        let text = punc_norm(text);
        let mut toks = vec![crate::t3::START_TEXT];
        toks.extend(self.tokenizer.encode(&text));
        toks.push(crate::t3::STOP_TEXT);
        let mut c = conds.t3.clone();
        c.exaggeration = o.exaggeration;
        let so = SampleOptions {
            temperature: o.temperature,
            cfg_weight: o.cfg_weight,
            repetition_penalty: o.repetition_penalty,
            min_p: o.min_p,
            top_p: o.top_p,
            max_new_tokens: o.max_new_tokens,
        };
        let mut tm = Timings::default();
        let t0 = Instant::now();
        let speech = self
            .t3
            .generate(&c, &toks, &so, &mut rng)
            .map_err(crate::e)?;
        let speech = crate::t3::drop_invalid(&speech);
        tm.t3_ms = t0.elapsed().as_millis();
        tm.speech_tokens = speech.len();
        if speech.is_empty() {
            return Err("T3 produced no speech tokens".into());
        }
        let t1 = Instant::now();
        let mel = self
            .flow
            .inference(&speech, &conds.gen, o.flow_steps, &mut rng)
            .map_err(crate::e)?;
        tm.flow_ms = t1.elapsed().as_millis();
        let t2 = Instant::now();
        let mut wav = self.hift.inference(&mel, &mut rng).map_err(crate::e)?;
        tm.hift_ms = t2.elapsed().as_millis();
        // Silence the first 20 ms and fade in over the next 20 ms (reduces
        // spill-over from the reference clip), as the reference does.
        let n = (crate::S3GEN_SR / 50) as usize;
        for (i, s) in wav.iter_mut().enumerate().take(2 * n) {
            let g = if i < n {
                0.0
            } else {
                let k = (i - n) as f64 / (n - 1) as f64;
                ((std::f64::consts::PI * (1.0 - k)).cos() + 1.0) / 2.0
            };
            *s *= g as f32;
        }
        Ok((wav, tm))
    }
}
