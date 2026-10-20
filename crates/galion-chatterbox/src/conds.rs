// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Voice conditionals: the built-in voice shipped as `conds.pt`, or one made
//! from a reference clip (the legacy server's `samantha.wav`).

use std::path::Path;

use candle_core::{DType, Tensor};

use crate::flow::GenCond;
use crate::t3::T3Cond;

#[derive(Clone)]
pub struct Conditionals {
    pub t3: T3Cond,
    pub gen: GenCond,
}

fn ints(t: &Tensor) -> crate::Result<Vec<u32>> {
    let t = t.flatten_all().map_err(crate::e)?;
    let v = t
        .to_dtype(DType::I64)
        .map_err(crate::e)?
        .to_vec1::<i64>()
        .map_err(crate::e)?;
    Ok(v.into_iter().map(|x| x.max(0) as u32).collect())
}

fn floats(t: &Tensor) -> crate::Result<Vec<f32>> {
    t.flatten_all()
        .and_then(|t| t.to_dtype(DType::F32))
        .and_then(|t| t.to_vec1::<f32>())
        .map_err(crate::e)
}

impl Conditionals {
    /// Read `conds.pt` (a torch zip pickle: {"t3": {...}, "gen": {...}}).
    pub fn load_pt(path: &Path) -> crate::Result<Self> {
        let get = |key: &str| -> crate::Result<std::collections::HashMap<String, Tensor>> {
            let r = candle_core::pickle::read_all_with_key(path, Some(key)).map_err(crate::e)?;
            Ok(r.into_iter().collect())
        };
        let t3 = get("t3")?;
        let gen = get("gen")?;
        let need =
            |m: &std::collections::HashMap<String, Tensor>, k: &str| -> crate::Result<Tensor> {
                m.get(k)
                    .cloned()
                    .ok_or_else(|| format!("conds.pt: missing {k}"))
            };
        let feat = need(&gen, "prompt_feat")?;
        let frames = feat.dims().get(1).copied().unwrap_or(0);
        Ok(Conditionals {
            t3: T3Cond {
                speaker_emb: floats(&need(&t3, "speaker_emb")?)?,
                prompt_tokens: match t3.get("cond_prompt_speech_tokens") {
                    Some(t) => ints(t)?,
                    None => Vec::new(),
                },
                exaggeration: t3
                    .get("emotion_adv")
                    .map(floats)
                    .transpose()?
                    .and_then(|v| v.first().copied())
                    .unwrap_or(0.5),
            },
            gen: GenCond {
                prompt_tokens: ints(&need(&gen, "prompt_token")?)?,
                prompt_feat: floats(&feat)?,
                prompt_frames: frames,
                embedding: floats(&need(&gen, "embedding")?)?,
            },
        })
    }

    /// The same voice with the S3Gen prompt cut to at most `secs` seconds
    /// (25 tokens and 50 mel frames per second). The flow-matching decoder
    /// attends over prompt + output, so a shorter prompt is proportionally
    /// cheaper on CPU; the reference uses up to 10 s.
    pub fn with_gen_prompt_limit(&self, secs: f32) -> Conditionals {
        let mut c = self.clone();
        let max_tok = (secs.max(0.0) * 25.0) as usize;
        let toks = c
            .gen
            .prompt_tokens
            .len()
            .min(max_tok)
            .min(c.gen.prompt_frames / 2);
        c.gen.prompt_tokens.truncate(toks);
        c.gen.prompt_frames = 2 * toks;
        c.gen.prompt_feat.truncate(2 * toks * 80);
        c
    }

    /// Build conditionals from a reference clip (any rate; mono mix).
    /// The first 10 s condition S3Gen, the first 6 s give T3's prompt tokens.
    pub fn from_wav(
        m: &crate::Chatterbox,
        samples: &[f32],
        sample_rate: u32,
        exaggeration: f32,
    ) -> crate::Result<Self> {
        let ref24 = crate::dsp::resample(samples, sample_rate, crate::S3GEN_SR);
        let ref16_full = crate::dsp::resample(&ref24, crate::S3GEN_SR, crate::S3_SR);
        let ref24 = &ref24[..ref24.len().min(10 * crate::S3GEN_SR as usize)];
        if ref24.len() < crate::S3GEN_SR as usize / 2 {
            return Err("reference clip shorter than 0.5 s".into());
        }
        // S3Gen prompt: 24 kHz mel, x-vector and tokens of the 16 kHz version.
        let mel = mel24(ref24);
        let frames = mel.len() / 80;
        let ref16 = crate::dsp::resample(ref24, crate::S3GEN_SR, crate::S3_SR);
        let xvec = m.campplus.embed(&ref16).map_err(crate::e)?;
        let mut toks = m.s3tok.tokenize(&ref16, None).map_err(crate::e)?;
        let mut frames = frames;
        if frames != 2 * toks.len() {
            toks.truncate(frames / 2);
            frames = frames.min(2 * toks.len());
        }
        let prompt_feat = mel[..frames * 80].to_vec();
        // T3: speaker embedding from the voice encoder, 150 prompt tokens (6 s).
        let six = &ref16_full[..ref16_full.len().min(6 * crate::S3_SR as usize)];
        let t3_toks = m
            .s3tok
            .tokenize(six, Some(crate::t3::PROMPT_LEN))
            .map_err(crate::e)?;
        let spk = m.ve.embed(&ref16_full).map_err(crate::e)?;
        Ok(Conditionals {
            t3: T3Cond {
                speaker_emb: spk,
                prompt_tokens: t3_toks,
                exaggeration,
            },
            gen: GenCond {
                prompt_tokens: toks,
                prompt_feat,
                prompt_frames: frames,
                embedding: xvec,
            },
        })
    }
}

/// Matcha/CosyVoice mel for 24 kHz: n_fft 1920, hop 480, reflect pad 720,
/// no centring, magnitude, 80 Slaney bins 0-8000 Hz, natural log (clip 1e-5).
/// Returns (frames, 80) row-major.
pub fn mel24(wav: &[f32]) -> Vec<f32> {
    let padded = crate::dsp::reflect_pad(wav, 720, 720);
    let spec = crate::dsp::stft(&padded, 1920, 480, &crate::dsp::hann(1920), false, true);
    let filters = crate::dsp::mel_filters_slaney(24000.0, 1920, 80, 0.0, 8000.0);
    let mut out = Vec::with_capacity(spec.len() * 80);
    for p in spec {
        // |X| = sqrt(power + 1e-9)
        let mag: Vec<f32> = p.iter().map(|v| (v + 1e-9).sqrt()).collect();
        for v in crate::dsp::apply_mel(&filters, 80, &mag) {
            out.push(v.max(1e-5).ln());
        }
    }
    out
}
