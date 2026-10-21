// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Whisper (tiny / tiny.en) speech-to-text on CPU via candle.
//!
//! Model files live outside git (e.g. `/workspace/oom-models/whisper-tiny.en`):
//! `model.safetensors`, `config.json`, `tokenizer.json`, `melfilters.bytes`.
//! Greedy decoding, no timestamps, English-only prompt (`.en` models).
//! The tokenizer is decoded here (GPT-2 byte-level BPE, decode direction only),
//! so the `tokenizers` crate and its C regex dependency are not needed.

use std::collections::HashMap;
use std::path::Path;

use candle_core::{DType, Device, IndexOp, Tensor, D};
use candle_nn::VarBuilder;

use crate::stt::SpeechToText;
use crate::whisper::{audio, model::Whisper, Config, N_FRAMES};

/// How much of the 30 s Whisper window the encoder sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderContext {
    /// Always 3000 mel frames (30 s), as trained. Constant cost per call.
    Full,
    /// Only the frames that hold audio (like whisper.cpp `-ac`): much cheaper
    /// for short VAD windows, may cost accuracy.
    Trimmed,
}

pub struct WhisperStt {
    model: Whisper,
    cfg: Config,
    filters: Vec<f32>,
    vocab: Vec<Option<Vec<u8>>>,
    sot: u32,
    eot: u32,
    no_timestamps: u32,
    suppress: Vec<f32>,
    pub context: EncoderContext,
    pub max_tokens: usize,
    device: Device,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// GPT-2 `bytes_to_unicode` inverse: char -> byte.
fn unicode_to_byte() -> HashMap<char, u8> {
    let mut bs: Vec<u32> = (b'!' as u32..=b'~' as u32)
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut cs = bs.clone();
    let mut n = 0;
    for b in 0..256u32 {
        if !bs.contains(&b) {
            bs.push(b);
            cs.push(256 + n);
            n += 1;
        }
    }
    bs.iter()
        .zip(cs.iter())
        .filter_map(|(&b, &c)| char::from_u32(c).map(|c| (c, b as u8)))
        .collect()
}

/// Parse `tokenizer.json`: returns id -> bytes (None for special tokens) and
/// the special-token ids by name.
/// id -> token bytes (`None` = special token).
type Vocab = Vec<Option<Vec<u8>>>;

fn load_vocab(json: &str) -> Result<(Vocab, HashMap<String, u32>), String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(err)?;
    let map = v["model"]["vocab"]
        .as_object()
        .ok_or("tokenizer.json: no model.vocab")?;
    let u2b = unicode_to_byte();
    let mut vocab: Vec<Option<Vec<u8>>> = Vec::new();
    for (tok, id) in map {
        let id = id.as_u64().ok_or("bad vocab id")? as usize;
        if vocab.len() <= id {
            vocab.resize(id + 1, None);
        }
        vocab[id] = tok.chars().map(|c| u2b.get(&c).copied()).collect();
    }
    let mut special = HashMap::new();
    for t in v["added_tokens"].as_array().into_iter().flatten() {
        if let (Some(id), Some(s)) = (t["id"].as_u64(), t["content"].as_str()) {
            special.insert(s.to_string(), id as u32);
            if (id as usize) < vocab.len() {
                vocab[id as usize] = None;
            }
        }
    }
    Ok((vocab, special))
}

impl WhisperStt {
    pub fn load(dir: &Path) -> Result<Self, String> {
        let device = Device::Cpu;
        let cfg: Config =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).map_err(err)?)
                .map_err(err)?;
        let fb = std::fs::read(dir.join("melfilters.bytes")).map_err(err)?;
        let filters: Vec<f32> = fb
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let (vocab, special) =
            load_vocab(&std::fs::read_to_string(dir.join("tokenizer.json")).map_err(err)?)?;
        let id = |s: &str| {
            special
                .get(s)
                .copied()
                .ok_or_else(|| format!("tokenizer: missing {s}"))
        };
        let (sot, eot, no_timestamps) = (
            id("<|startoftranscript|>")?,
            id("<|endoftext|>")?,
            id("<|notimestamps|>")?,
        );
        // SAFETY: the weights file is opened read-only and not modified while
        // mapped (it lives in a model directory outside the repo).
        #[allow(unsafe_code)]
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[dir.join("model.safetensors")],
                DType::F32,
                &device,
            )
            .map_err(err)?
        };
        let model = Whisper::load(&vb, cfg.clone()).map_err(err)?;
        let mut suppress = vec![0f32; cfg.vocab_size];
        for &t in &cfg.suppress_tokens {
            if let Some(s) = suppress.get_mut(t as usize) {
                *s = f32::NEG_INFINITY;
            }
        }
        // Timestamp tokens come after <|notimestamps|>; never emit them.
        for s in suppress.iter_mut().skip(no_timestamps as usize + 1) {
            *s = f32::NEG_INFINITY;
        }
        Ok(WhisperStt {
            model,
            cfg,
            filters,
            vocab,
            sot,
            eot,
            no_timestamps,
            suppress,
            context: EncoderContext::Full,
            max_tokens: 224,
            device,
        })
    }

    fn run(&mut self, pcm16k: &[f32]) -> candle_core::Result<Vec<u32>> {
        let n_mels = self.cfg.num_mel_bins;
        let mel = audio::pcm_to_mel(&self.cfg, pcm16k, &self.filters);
        let frames = mel.len() / n_mels;
        let mel = Tensor::from_vec(mel, (1, n_mels, frames), &self.device)?;
        let want = match self.context {
            EncoderContext::Full => N_FRAMES,
            // Audio frames rounded up to even (conv stride 2), min 100 (1 s).
            EncoderContext::Trimmed => (pcm16k.len() / 160 + 1).div_ceil(2) * 2,
        }
        .clamp(100, N_FRAMES)
        .min(frames);
        let mel = mel.narrow(2, 0, want)?;
        let feats = self.model.encoder.forward(&mel, true)?;
        let suppress = Tensor::new(self.suppress.as_slice(), &self.device)?;
        let mut tokens = vec![self.sot, self.no_timestamps];
        // Budget ~8 tokens per second of audio: short VAD windows otherwise
        // fall into Whisper's repetition loops and burn CPU.
        let budget = (pcm16k.len() / 2_000 + 8).min(self.max_tokens);
        for i in 0..budget {
            let t = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let ys = self.model.decoder.forward(&t, &feats, i == 0)?;
            let seq = ys.dim(1)?;
            let logits = self
                .model
                .decoder
                .final_linear(&ys.i((..1, seq - 1..))?)?
                .i(0)?
                .i(0)?;
            let next = logits
                .broadcast_add(&suppress)?
                .argmax(D::Minus1)?
                .to_scalar::<u32>()?;
            if next == self.eot {
                break;
            }
            tokens.push(next);
            if let Some(n) = repeating(&tokens[2..]) {
                let keep = tokens.len() - n;
                tokens.truncate(keep);
                break;
            }
        }
        Ok(tokens[2..].to_vec())
    }

    pub fn decode_tokens(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for &id in ids {
            if let Some(Some(b)) = self.vocab.get(id as usize) {
                bytes.extend_from_slice(b);
            }
        }
        String::from_utf8_lossy(&bytes).trim().to_string()
    }
}

impl SpeechToText for WhisperStt {
    fn sample_rate(&self) -> u32 {
        16_000
    }
    fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
        let ids = self.run(pcm).map_err(err)?;
        Ok(self.decode_tokens(&ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_level_table_is_a_bijection() {
        let m = unicode_to_byte();
        assert_eq!(m.len(), 256);
        assert_eq!(m[&'Ġ'], b' ');
        assert_eq!(m[&'a'], b'a');
    }

    #[test]
    fn vocab_parse_and_decode() {
        let j = r#"{"model":{"vocab":{"Hello":0,"Ġworld":1,"!":2}},
                    "added_tokens":[{"id":3,"content":"<|endoftext|>"}]}"#;
        let (v, s) = load_vocab(j).unwrap();
        assert_eq!(s["<|endoftext|>"], 3);
        let bytes: Vec<u8> = [0usize, 1, 2]
            .iter()
            .flat_map(|&i| v[i].clone().unwrap())
            .collect();
        assert_eq!(String::from_utf8(bytes).unwrap(), "Hello world!");
    }
}

/// True when the greedy decoder is looping: the tail is the same short n-gram
/// (n = 1..=3) three times in a row, or a longer one (n = 4..=16) twice.
/// Returns the looping n-gram length so the caller can drop the repeated tail.
pub fn repeating(t: &[u32]) -> Option<usize> {
    let l = t.len();
    if let Some(n) = (4..=16).find(|&n| l >= 2 * n && t[l - n..] == t[l - 2 * n..l - n]) {
        return Some(n);
    }
    (1..=3).find(|&n| {
        l >= 3 * n && t[l - n..] == t[l - 2 * n..l - n] && t[l - n..] == t[l - 3 * n..l - 2 * n]
    })
}

#[cfg(test)]
mod loop_tests {
    use super::repeating;

    #[test]
    fn detects_loops_not_normal_text() {
        assert!(repeating(&[1, 2, 3, 9, 9, 9]).is_some());
        assert!(repeating(&[9, 1, 2, 3, 4, 1, 2, 3, 4]).is_some());
        assert!(repeating(&[5, 1, 2, 1, 2, 1, 2]).is_some());
        assert!(repeating(&[1, 2, 3, 4, 1, 2, 3, 5]).is_none());
        assert!(repeating(&[7, 7]).is_none());
    }
}
