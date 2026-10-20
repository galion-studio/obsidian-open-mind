// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! T3: the text-to-speech-token model of Chatterbox. A 30-layer Llama
//! (hidden 1024, 16 heads, SwiGLU 4096, RMSNorm, Llama-3 scaled RoPE) fed
//! with custom embeddings: [speaker, 32 perceiver tokens of the prompt speech,
//! emotion] + text (+ learned positions) + speech (+ learned positions).
//! Inference runs the conditional and the text-free branch as a batch of two
//! and mixes them with classifier-free guidance, then samples with
//! repetition penalty, temperature, min-p and top-p.

use candle_core::{Device, Module, Result, Tensor};
use candle_nn::{kv_cache::KvCache, Embedding, LayerNorm, Linear, RmsNorm, VarBuilder};

use crate::nnx::{heads, layer_norm, linear, merge_heads, sdpa};
use crate::rng::Rng;

pub const START_TEXT: u32 = 255;
pub const STOP_TEXT: u32 = 0;
pub const START_SPEECH: u32 = 6561;
pub const STOP_SPEECH: u32 = 6562;
pub const SPEECH_VOCAB: usize = 8194;
pub const TEXT_VOCAB: usize = 704;
pub const PROMPT_LEN: usize = 150;
const HIDDEN: usize = 1024;
const N_HEADS: usize = 16;
const HEAD_DIM: usize = 64;
const N_LAYERS: usize = 30;
const INTER: usize = 4096;

struct Layer {
    ln1: RmsNorm,
    q: Linear,
    k: Linear,
    v: Linear,
    o: Linear,
    ln2: RmsNorm,
    gate: Linear,
    up: Linear,
    down: Linear,
}

struct Perceiver {
    query: Tensor,
    norm: LayerNorm,
    to_q: Linear,
    to_k: Linear,
    to_v: Linear,
    proj: Linear,
}

impl Perceiver {
    fn attn(&self, x1: &Tensor, x2: &Tensor) -> Result<Tensor> {
        let (a, b) = (self.norm.forward(x1)?, self.norm.forward(x2)?);
        let q = heads(&self.to_q.forward(&a)?, 4)?;
        let k = heads(&self.to_k.forward(&b)?, 4)?;
        let v = heads(&self.to_v.forward(&b)?, 4)?;
        let h = sdpa(&q, &k, &v, 1.0 / (HIDDEN as f64 / 4.0).sqrt(), None)?;
        x1 + self.proj.forward(&merge_heads(&h)?)?
    }
    fn forward(&self, h: &Tensor) -> Result<Tensor> {
        let q = self
            .query
            .broadcast_as((h.dim(0)?, 32, HIDDEN))?
            .contiguous()?;
        let pre = self.attn(&q, h)?;
        self.attn(&pre, &pre)
    }
}

/// Speaker / prompt conditioning for T3.
#[derive(Clone)]
pub struct T3Cond {
    /// Voice-encoder speaker embedding, 256 values.
    pub speaker_emb: Vec<f32>,
    /// S3 speech tokens of the reference (at most 150 = 6 s).
    pub prompt_tokens: Vec<u32>,
    /// Emotion exaggeration (0.5 neutral).
    pub exaggeration: f32,
}

#[derive(Clone, Debug)]
pub struct SampleOptions {
    pub temperature: f32,
    pub cfg_weight: f32,
    pub repetition_penalty: f32,
    pub min_p: f32,
    pub top_p: f32,
    pub max_new_tokens: usize,
}

impl Default for SampleOptions {
    fn default() -> Self {
        SampleOptions {
            temperature: 0.8,
            cfg_weight: 0.5,
            repetition_penalty: 1.2,
            min_p: 0.05,
            top_p: 1.0,
            max_new_tokens: 1000,
        }
    }
}

pub struct T3 {
    layers: Vec<Layer>,
    norm: RmsNorm,
    text_emb: Embedding,
    speech_emb: Embedding,
    text_pos: Tensor,
    speech_pos: Tensor,
    speech_head: Linear,
    spkr_enc: Linear,
    emotion_fc: Linear,
    perceiver: Perceiver,
    cos: Tensor,
    sin: Tensor,
    dev: Device,
}

/// Llama-3 RoPE frequencies (factor 8, low 1, high 4, original 8192).
fn llama3_inv_freq() -> Vec<f32> {
    let (theta, factor, low, high, orig) = (500_000f64, 8.0f64, 1.0f64, 4.0f64, 8192.0f64);
    let (low_wl, high_wl) = (orig / low, orig / high);
    (0..HEAD_DIM / 2)
        .map(|i| {
            let f = 1.0 / theta.powf(2.0 * i as f64 / HEAD_DIM as f64);
            let wl = 2.0 * std::f64::consts::PI / f;
            let f = if wl < high_wl {
                f
            } else if wl > low_wl {
                f / factor
            } else {
                let s = (orig / wl - low) / (high - low);
                (1.0 - s) * f / factor + s * f
            };
            f as f32
        })
        .collect()
}

impl T3 {
    pub fn load(vb: VarBuilder, max_pos: usize) -> Result<Self> {
        let dev = vb.device().clone();
        let t = vb.pp("tfmr");
        let mut layers = Vec::with_capacity(N_LAYERS);
        for i in 0..N_LAYERS {
            let l = t.pp(format!("layers.{i}"));
            let a = l.pp("self_attn");
            let m = l.pp("mlp");
            layers.push(Layer {
                ln1: candle_nn::rms_norm(HIDDEN, 1e-5, l.pp("input_layernorm"))?,
                q: linear(&a.pp("q_proj"), HIDDEN, HIDDEN, false)?,
                k: linear(&a.pp("k_proj"), HIDDEN, HIDDEN, false)?,
                v: linear(&a.pp("v_proj"), HIDDEN, HIDDEN, false)?,
                o: linear(&a.pp("o_proj"), HIDDEN, HIDDEN, false)?,
                ln2: candle_nn::rms_norm(HIDDEN, 1e-5, l.pp("post_attention_layernorm"))?,
                gate: linear(&m.pp("gate_proj"), HIDDEN, INTER, false)?,
                up: linear(&m.pp("up_proj"), HIDDEN, INTER, false)?,
                down: linear(&m.pp("down_proj"), INTER, HIDDEN, false)?,
            });
        }
        let c = vb.pp("cond_enc");
        let p = c.pp("perceiver");
        let inv = llama3_inv_freq();
        let pos: Vec<f32> = (0..max_pos).map(|i| i as f32).collect();
        let pos = Tensor::from_vec(pos, (max_pos, 1), &dev)?;
        let inv = Tensor::from_vec(inv, (1, HEAD_DIM / 2), &dev)?;
        let freqs = pos.broadcast_mul(&inv)?;
        Ok(T3 {
            layers,
            norm: candle_nn::rms_norm(HIDDEN, 1e-5, t.pp("norm"))?,
            text_emb: candle_nn::embedding(TEXT_VOCAB, HIDDEN, vb.pp("text_emb"))?,
            speech_emb: candle_nn::embedding(SPEECH_VOCAB, HIDDEN, vb.pp("speech_emb"))?,
            text_pos: vb
                .pp("text_pos_emb")
                .pp("emb")
                .get((2050, HIDDEN), "weight")?,
            speech_pos: vb
                .pp("speech_pos_emb")
                .pp("emb")
                .get((4100, HIDDEN), "weight")?,
            speech_head: linear(&vb.pp("speech_head"), HIDDEN, SPEECH_VOCAB, false)?,
            spkr_enc: linear(&c.pp("spkr_enc"), 256, HIDDEN, true)?,
            emotion_fc: linear(&c.pp("emotion_adv_fc"), 1, HIDDEN, false)?,
            perceiver: Perceiver {
                query: p.get((1, 32, HIDDEN), "pre_attention_query")?,
                norm: layer_norm(&p.pp("attn").pp("norm"), HIDDEN, 1e-5)?,
                to_q: linear(&p.pp("attn").pp("to_q"), HIDDEN, HIDDEN, true)?,
                to_k: linear(&p.pp("attn").pp("to_k"), HIDDEN, HIDDEN, true)?,
                to_v: linear(&p.pp("attn").pp("to_v"), HIDDEN, HIDDEN, true)?,
                proj: linear(&p.pp("attn").pp("proj_out"), HIDDEN, HIDDEN, true)?,
            },
            cos: freqs.cos()?.contiguous()?,
            sin: freqs.sin()?.contiguous()?,
            dev,
        })
    }

    fn pos_rows(table: &Tensor, start: usize, len: usize) -> Result<Tensor> {
        table.narrow(0, start, len)?.unsqueeze(0)
    }

    /// (1, 1 + 32 + 1, 1024): speaker, perceiver(prompt speech), emotion.
    fn conditioning(&self, c: &T3Cond) -> Result<Tensor> {
        let spk = Tensor::from_vec(c.speaker_emb.clone(), (1, 256), &self.dev)?;
        let spk = self.spkr_enc.forward(&spk)?.unsqueeze(1)?;
        let mut parts = vec![spk];
        if !c.prompt_tokens.is_empty() {
            let n = c.prompt_tokens.len();
            let ids = Tensor::new(c.prompt_tokens.as_slice(), &self.dev)?.unsqueeze(0)?;
            let emb = self
                .speech_emb
                .forward(&ids)?
                .broadcast_add(&Self::pos_rows(&self.speech_pos, 0, n)?)?;
            parts.push(self.perceiver.forward(&emb)?);
        }
        let ex = Tensor::from_vec(vec![c.exaggeration], (1, 1, 1), &self.dev)?;
        parts.push(self.emotion_fc.forward(&ex)?);
        Tensor::cat(&parts, 1)
    }

    fn block(&self, x: &Tensor, pos: usize, caches: &mut [KvCache]) -> Result<Tensor> {
        let (b, t, _) = x.dims3()?;
        let cos = self.cos.narrow(0, pos, t)?;
        let sin = self.sin.narrow(0, pos, t)?;
        let mask = if t > 1 {
            let total = pos + t;
            let m: Vec<f32> = (0..t)
                .flat_map(|i| {
                    (0..total).map(move |j| if j > pos + i { f32::NEG_INFINITY } else { 0.0 })
                })
                .collect();
            Some(Tensor::from_vec(m, (1, 1, t, total), &self.dev)?)
        } else {
            None
        };
        let mut x = x.clone();
        for (l, cache) in self.layers.iter().zip(caches.iter_mut()) {
            let h = l.ln1.forward(&x)?;
            let q = heads(&l.q.forward(&h)?, N_HEADS)?;
            let k = heads(&l.k.forward(&h)?, N_HEADS)?;
            let v = heads(&l.v.forward(&h)?, N_HEADS)?;
            let q = candle_nn::rotary_emb::rope(&q, &cos, &sin)?;
            let k = candle_nn::rotary_emb::rope(&k, &cos, &sin)?;
            let (k, v) = cache.append(&k, &v)?;
            let a = sdpa(&q, &k, &v, 1.0 / (HEAD_DIM as f64).sqrt(), mask.as_ref())?;
            let a = merge_heads(&a)?.reshape((b, t, HIDDEN))?;
            x = (x + l.o.forward(&a)?)?;
            let h = l.ln2.forward(&x)?;
            let m = (l.gate.forward(&h)?.silu()? * l.up.forward(&h)?)?;
            x = (x + l.down.forward(&m)?)?;
        }
        Ok(x)
    }

    fn logits_last(&self, h: &Tensor) -> Result<Vec<Vec<f32>>> {
        let t = h.dim(1)?;
        let last = h.narrow(1, t - 1, 1)?;
        let last = self.norm.forward(&last)?;
        self.speech_head
            .forward(&last)?
            .squeeze(1)?
            .to_vec2::<f32>()
    }

    /// Generate S3 speech tokens (without start/stop) for `text_tokens`
    /// (already wrapped in START_TEXT / STOP_TEXT).
    pub fn generate(
        &self,
        cond: &T3Cond,
        text_tokens: &[u32],
        o: &SampleOptions,
        rng: &mut Rng,
    ) -> Result<Vec<u32>> {
        let cfg = o.cfg_weight > 0.0;
        let bsz = if cfg { 2 } else { 1 };
        let cond_emb = self.conditioning(cond)?; // (1, Lc, D)
        let lt = text_tokens.len();
        let ids = Tensor::new(text_tokens, &self.dev)?.unsqueeze(0)?;
        let text = self.text_emb.forward(&ids)?; // (1, Lt, D)
        let tpos = Self::pos_rows(&self.text_pos, 0, lt)?;
        let sp0 = Self::pos_rows(&self.speech_pos, 0, 1)?;
        let start = Tensor::new(&[START_SPEECH], &self.dev)?.unsqueeze(0)?;
        let start = self.speech_emb.forward(&start)?.broadcast_add(&sp0)?; // (1,1,D)
        let mut rows = Vec::with_capacity(bsz);
        rows.push(Tensor::cat(
            &[&cond_emb, &text.broadcast_add(&tpos)?, &start, &start],
            1,
        )?);
        if cfg {
            // Unconditional branch: text embeddings zeroed, positions kept.
            let blank = tpos.clone();
            rows.push(Tensor::cat(&[&cond_emb, &blank, &start, &start], 1)?);
        }
        let x = Tensor::cat(&rows, 0)?;
        let prefill = x.dim(1)?;
        let max_len = prefill + o.max_new_tokens + 2;
        if max_len > self.cos.dim(0)? {
            candle_core::bail!("sequence too long for RoPE table ({max_len})");
        }
        let mut caches: Vec<KvCache> = (0..N_LAYERS).map(|_| KvCache::new(2, max_len)).collect();
        let h = self.block(&x, 0, &mut caches)?;
        let mut logits = self.logits_last(&h)?;
        let mut generated: Vec<u32> = vec![START_SPEECH];
        let mut out = Vec::new();
        for i in 0..o.max_new_tokens {
            let pos = prefill + i;
            let mut l: Vec<f32> = if cfg {
                logits[0]
                    .iter()
                    .zip(&logits[1])
                    .map(|(c, u)| c + o.cfg_weight * (c - u))
                    .collect()
            } else {
                logits[0].clone()
            };
            let next = sample(&mut l, &generated, o, rng) as u32;
            generated.push(next);
            if next == STOP_SPEECH {
                break;
            }
            out.push(next);
            let id = Tensor::new(&[next], &self.dev)?.unsqueeze(0)?;
            let e = self
                .speech_emb
                .forward(&id)?
                .broadcast_add(&Self::pos_rows(&self.speech_pos, (i + 1).min(4099), 1)?)?;
            let e = if cfg { Tensor::cat(&[&e, &e], 0)? } else { e };
            let h = self.block(&e, pos, &mut caches)?;
            logits = self.logits_last(&h)?;
        }
        Ok(out)
    }

    /// Text/speech token ids helper for tests.
    pub fn device(&self) -> &Device {
        &self.dev
    }
}

/// Logit processing as in the reference loop: repetition penalty over the
/// tokens generated so far, temperature, min-p, top-p, then a draw.
pub fn sample(l: &mut [f32], generated: &[u32], o: &SampleOptions, rng: &mut Rng) -> usize {
    if o.repetition_penalty != 1.0 {
        let mut seen = generated.to_vec();
        seen.sort_unstable();
        seen.dedup();
        for &t in &seen {
            if let Some(v) = l.get_mut(t as usize) {
                *v = if *v < 0.0 {
                    *v * o.repetition_penalty
                } else {
                    *v / o.repetition_penalty
                };
            }
        }
    }
    if o.temperature > 0.0 && o.temperature != 1.0 {
        for v in l.iter_mut() {
            *v /= o.temperature;
        }
    }
    let max = l.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let mut p: Vec<f32> = l.iter().map(|&v| (v - max).exp()).collect();
    let sum: f32 = p.iter().sum();
    for v in p.iter_mut() {
        *v /= sum;
    }
    if o.min_p > 0.0 {
        let top = p.iter().cloned().fold(0f32, f32::max);
        let thr = o.min_p * top;
        for v in p.iter_mut() {
            if *v < thr {
                *v = 0.0;
            }
        }
    }
    if o.top_p < 1.0 {
        let mut idx: Vec<usize> = (0..p.len()).collect();
        idx.sort_by(|&a, &b| p[b].total_cmp(&p[a]));
        let total: f32 = p.iter().sum();
        let mut acc = 0f32;
        let mut keep = 0;
        for (n, &i) in idx.iter().enumerate() {
            acc += p[i] / total;
            keep = n + 1;
            if acc >= o.top_p {
                break;
            }
        }
        for &i in &idx[keep..] {
            p[i] = 0.0;
        }
    }
    if o.temperature <= 0.0 {
        return p
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(i, _)| i);
    }
    rng.categorical(&p)
}

/// Keep the tokens between a start and a stop marker and drop ids >= 6561.
pub fn drop_invalid(tokens: &[u32]) -> Vec<u32> {
    let s = tokens
        .iter()
        .position(|&t| t == START_SPEECH)
        .map_or(0, |p| p + 1);
    let e = tokens[s..]
        .iter()
        .position(|&t| t == STOP_SPEECH)
        .map_or(tokens.len(), |p| s + p);
    tokens[s..e]
        .iter()
        .copied()
        .filter(|&t| t < crate::SPEECH_VOCAB)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_scaling_bounds() {
        let f = llama3_inv_freq();
        assert_eq!(f.len(), 32);
        assert!((f[0] - 1.0).abs() < 1e-6);
        // The lowest frequencies are divided by the factor 8.
        let plain = 1.0 / 500_000f64.powf(62.0 / 64.0);
        assert!(((f[31] as f64) - plain / 8.0).abs() / (plain / 8.0) < 1e-4);
    }

    #[test]
    fn sampling_rules() {
        let o = SampleOptions {
            temperature: 1.0,
            min_p: 0.5,
            ..Default::default()
        };
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            let mut l = vec![0.0, 5.0, 4.9, -3.0];
            let t = sample(&mut l, &[], &o, &mut rng);
            assert!(t == 1 || t == 2);
        }
        // Repetition penalty pushes a repeated token below its rival.
        let greedy = SampleOptions {
            temperature: 0.0,
            repetition_penalty: 2.0,
            min_p: 0.0,
            ..Default::default()
        };
        let mut l = vec![3.0, 2.0];
        assert_eq!(sample(&mut l, &[0], &greedy, &mut rng), 1);
        assert_eq!(drop_invalid(&[6561, 5, 6563, 7, 6562, 9]), vec![5, 7]);
        assert_eq!(drop_invalid(&[1, 2]), vec![1, 2]);
    }
}
