// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! KittenTTS nano 0.8 (Apache-2.0 weights, StyleTTS2-distilled, 14 M params,
//! 24 kHz) on CPU via candle 0.9, fed by our own licence-clean G2P
//! ([`crate::g2p`]) instead of GPL espeak-ng.
//!
//! Model code: vendored from idle-intelligence/tts-web `kitten-core` (MIT).
//! Weights (not in git, e.g. `/workspace/oom-models/kitten-tts-nano`):
//! `kitten-nano.safetensors` and `kitten-voices.safetensors` from
//! `idle-intelligence/kitten-tts-nano-safetensors`, a conversion of
//! `KittenML/kitten-tts-nano-0.8` (its metadata names the source ONNX sha256
//! 320564d2..., the official file), plus a CMUdict file.

#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod bert;
pub mod config;
pub mod decoder;
pub mod model;
pub mod predictor;
pub mod text_encoder;

use std::collections::HashMap;
use std::path::Path;

use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;

use crate::g2p::{kitten_ids, kitten_symbol_table, Lexicon, Phonemizer};
use crate::stt::{clauses, sentences, TextToSpeech};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Voice names in `kitten-voices.safetensors` and KittenTTS's speed priors.
pub const VOICES: [(&str, f32); 8] = [
    ("bella", 0.8),
    ("bruno", 0.8),
    ("hugo", 0.9),
    ("jasper", 0.8),
    ("kiki", 0.8),
    ("leo", 0.8),
    ("luna", 0.8),
    ("rosie", 0.8),
];

pub struct KittenTts {
    model: model::KittenModel,
    voices: HashMap<String, Tensor>,
    g2p: Phonemizer,
    table: HashMap<char, u32>,
    pub default_voice: String,
    pub speed: f32,
    /// IPA and token count of the last call (for logs / benches).
    pub last_ipa: String,
    pub last_tokens: usize,
}

impl KittenTts {
    /// `dir` holds `kitten-nano.safetensors` and `kitten-voices.safetensors`;
    /// `cmudict` is a CMUdict-format pronouncing dictionary.
    pub fn load(dir: &Path, cmudict: &Path) -> Result<Self, String> {
        let device = Device::Cpu;
        let cfg = config::KittenConfig::nano();
        // Safety: the weight file is opened read-only and not modified while
        // mapped (same pattern as the Whisper loader).
        #[allow(unsafe_code)]
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[dir.join("kitten-nano.safetensors")],
                DType::F32,
                &device,
            )
        }
        .map_err(err)?;
        let model = model::KittenModel::load_vb(vb, &cfg).map_err(err)?;
        let voices = candle_core::safetensors::load(dir.join("kitten-voices.safetensors"), &device)
            .map_err(err)?;
        let mut g2p = Phonemizer::new(Lexicon::load(cmudict)?);
        g2p.add_word("galion", "G AE1 L IY0 AH0 N");
        Ok(KittenTts {
            model,
            voices,
            g2p,
            table: kitten_symbol_table(),
            default_voice: "jasper".into(),
            speed: 1.0,
            last_ipa: String::new(),
            last_tokens: 0,
        })
    }

    pub fn voice_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.voices.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }

    /// Token ids straight to audio (f32, 24 kHz). `text_len` picks the style
    /// row like KittenTTS does (`min(len(text), rows - 1)`).
    pub fn synthesize_ids(
        &self,
        ids: &[u32],
        voice: &str,
        text_len: usize,
    ) -> Result<Vec<f32>, String> {
        let name = if self.voices.contains_key(voice) {
            voice
        } else {
            self.default_voice.as_str()
        };
        let table = self
            .voices
            .get(name)
            .ok_or_else(|| format!("no voice {name:?}"))?;
        let rows = table.dim(0).map_err(err)?;
        let style = table
            .i(text_len.min(rows.saturating_sub(1)))
            .and_then(|t| t.unsqueeze(0))
            .map_err(err)?;
        let prior = VOICES
            .iter()
            .find(|(v, _)| *v == name)
            .map_or(1.0, |(_, s)| *s);
        let ids: Vec<i32> = ids.iter().map(|&i| i as i32).collect();
        self.model
            .synthesize(&ids, &style, self.speed * prior)
            .map_err(err)
    }

    /// Text to the IPA string the model gets (our G2P).
    pub fn ipa(&mut self, text: &str) -> String {
        self.g2p.phonemize(text)
    }

    /// Text to f32 audio at 24 kHz.
    pub fn synthesize_f32(&mut self, text: &str, voice: &str) -> Result<Vec<f32>, String> {
        let ipa = self.g2p.phonemize(text);
        let ids = kitten_ids(&ipa, &self.table);
        self.last_tokens = ids.len();
        self.last_ipa = ipa;
        if ids.len() <= 3 {
            return Ok(Vec::new());
        }
        self.synthesize_ids(&ids, voice, text.chars().count())
    }
}

impl TextToSpeech for KittenTts {
    fn sample_rate(&self) -> u32 {
        24_000
    }

    fn synthesize(&mut self, text: &str, voice: &str) -> Result<Vec<i16>, String> {
        let mut out = Vec::new();
        for chunk in sentences(text) {
            out.extend(crate::resample::f32_to_i16(
                &self.synthesize_f32(chunk, voice)?,
            ));
        }
        Ok(out)
    }

    /// One model call per sentence (long ones per clause): bounded BERT
    /// context (512 positions) and memory, and the pipeline can play chunk 1
    /// while making chunk 2.
    fn chunks<'a>(&self, text: &'a str) -> Vec<&'a str> {
        clauses(text, 60)
    }
}
