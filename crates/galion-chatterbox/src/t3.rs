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
use candle_nn::{Embedding, LayerNorm, Linear, RmsNorm, VarBuilder};
use rayon::prelude::*;

use crate::nnx::{heads, layer_norm, linear, merge_heads, sdpa};
use crate::qlinear::{Proj, T3Weights};
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
    /// q, k and v stacked (1024 -> 3072).
    qkv: Proj,
    o: Proj,
    ln2: RmsNorm,
    /// gate and up stacked (1024 -> 8192).
    gate_up: Proj,
    down: Proj,
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
    speech_head: Proj,
    spkr_enc: Linear,
    emotion_fc: Linear,
    perceiver: Perceiver,
    /// RoPE tables, (max_pos, 32) row-major.
    rope_cos: Vec<f32>,
    rope_sin: Vec<f32>,
    max_pos: usize,
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
    /// `weights` picks f32 or int8 storage for the transformer projections
    /// and the speech head (see [`crate::qlinear`]).
    pub fn load(vb: VarBuilder, max_pos: usize, weights: T3Weights) -> Result<Self> {
        let dev = vb.device().clone();
        let t = vb.pp("tfmr");
        let mut layers = Vec::with_capacity(N_LAYERS);
        for i in 0..N_LAYERS {
            let l = t.pp(format!("layers.{i}"));
            let a = l.pp("self_attn");
            let m = l.pp("mlp");
            let lin = |p: &str, i: usize, o: usize| linear(&a.pp(p), i, o, false);
            let mlp = |p: &str, i: usize, o: usize| linear(&m.pp(p), i, o, false);
            let (q, k, v) = (
                lin("q_proj", HIDDEN, HIDDEN)?,
                lin("k_proj", HIDDEN, HIDDEN)?,
                lin("v_proj", HIDDEN, HIDDEN)?,
            );
            let (gate, up) = (
                mlp("gate_proj", HIDDEN, INTER)?,
                mlp("up_proj", HIDDEN, INTER)?,
            );
            layers.push(Layer {
                ln1: candle_nn::rms_norm(HIDDEN, 1e-5, l.pp("input_layernorm"))?,
                qkv: Proj::new(&[&q, &k, &v], weights)?,
                o: Proj::new(&[&lin("o_proj", HIDDEN, HIDDEN)?], weights)?,
                ln2: candle_nn::rms_norm(HIDDEN, 1e-5, l.pp("post_attention_layernorm"))?,
                gate_up: Proj::new(&[&gate, &up], weights)?,
                down: Proj::new(&[&mlp("down_proj", INTER, HIDDEN)?], weights)?,
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
            speech_head: Proj::new(
                &[&linear(&vb.pp("speech_head"), HIDDEN, SPEECH_VOCAB, false)?],
                weights,
            )?,
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
            rope_cos: freqs.cos()?.flatten_all()?.to_vec1()?,
            rope_sin: freqs.sin()?.flatten_all()?.to_vec1()?,
            max_pos,
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

    fn block(&self, x: &Tensor, pos: usize, caches: &mut [KvStore]) -> Result<Tensor> {
        let _p = crate::prof::span(if x.dim(1)? > 1 {
            "t3.prefill"
        } else {
            "t3.decode_step"
        });
        let (b, t, _) = x.dims3()?;
        let mut x = x.clone();
        for (l, cache) in self.layers.iter().zip(caches.iter_mut()) {
            let p = crate::prof::span("t3.l.qkv");
            let h = l.ln1.forward(&x)?;
            let qkv = l.qkv.forward(&h)?;
            drop(p);
            let p = crate::prof::span("t3.l.attn");
            let qkv = qkv.flatten_all()?.to_vec1::<f32>()?;
            let a = cache.attend(&qkv, b, t, pos, &self.rope_cos, &self.rope_sin);
            let a = Tensor::from_vec(a, (b, t, HIDDEN), &self.dev)?;
            drop(p);
            let p = crate::prof::span("t3.l.o");
            x = (x + l.o.forward(&a)?)?;
            drop(p);
            let p = crate::prof::span("t3.l.gate_up");
            let h = l.ln2.forward(&x)?;
            let gu = l.gate_up.forward(&h)?;
            drop(p);
            let _p = crate::prof::span("t3.l.down");
            let gate = crate::fastops::silu(&gu.narrow(2, 0, INTER)?)?;
            let m = (gate * gu.narrow(2, INTER, INTER)?)?;
            x = (x + l.down.forward(&m)?)?;
        }
        Ok(x)
    }

    fn logits_last(&self, h: &Tensor) -> Result<Vec<Vec<f32>>> {
        let _p = crate::prof::span("t3.logits");
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
        if max_len > self.max_pos {
            candle_core::bail!("sequence too long for RoPE table ({max_len})");
        }
        let mut caches: Vec<KvStore> = (0..N_LAYERS)
            .map(|_| KvStore::with_capacity(bsz, prefill + 256))
            .collect();
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
            let _p = crate::prof::span("t3.sample");
            let next = sample(&mut l, &generated, o, rng) as u32;
            drop(_p);
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

/// Key/value cache of one T3 layer plus its attention (voice-speed).
///
/// Layout `[position][batch][head * 64]`, so appending a step is a push at the
/// end. Attention for the decode step (one query per row) and the causal
/// prefill runs as plain Rust over (batch, query, head) in parallel; with
/// candle it was ~25 small ops per layer and 64 single-row gemm calls, which
/// cost more than the int8 projections themselves.
pub struct KvStore {
    k: Vec<f32>,
    v: Vec<f32>,
    len: usize,
}

impl KvStore {
    pub fn with_capacity(rows: usize, positions: usize) -> Self {
        KvStore {
            k: Vec::with_capacity(rows * positions * HIDDEN),
            v: Vec::with_capacity(rows * positions * HIDDEN),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `qkv`: (b, t, 3 * 1024) row-major for positions `pos .. pos + t`.
    /// Applies RoPE to q and k, appends k and v, and returns the attention
    /// output (b, t, 1024) with heads already merged.
    pub fn attend(
        &mut self,
        qkv: &[f32],
        b: usize,
        t: usize,
        pos: usize,
        cos: &[f32],
        sin: &[f32],
    ) -> Vec<f32> {
        const HALF: usize = HEAD_DIM / 2;
        let row = 3 * HIDDEN;
        let rope = |x: &mut [f32], p: usize| {
            let (c, s) = (
                &cos[p * HALF..(p + 1) * HALF],
                &sin[p * HALF..(p + 1) * HALF],
            );
            for h in x.as_chunks_mut::<HEAD_DIM>().0 {
                let (x1, x2) = h.split_at_mut(HALF);
                for i in 0..HALF {
                    let (a, bb) = (x1[i], x2[i]);
                    x1[i] = a * c[i] - bb * s[i];
                    x2[i] = bb * c[i] + a * s[i];
                }
            }
        };
        debug_assert_eq!(self.len, pos);
        // Append k (rotated) and v for every new position, batch-major inside.
        for ti in 0..t {
            for bi in 0..b {
                let r = &qkv[(bi * t + ti) * row..(bi * t + ti + 1) * row];
                let start = self.k.len();
                self.k.extend_from_slice(&r[HIDDEN..2 * HIDDEN]);
                rope(&mut self.k[start..], pos + ti);
                self.v.extend_from_slice(&r[2 * HIDDEN..]);
            }
        }
        self.len += t;
        let stride = b * HIDDEN;
        let scale = 1.0 / (HEAD_DIM as f32).sqrt();
        let (keys, vals) = (&self.k, &self.v);
        let mut out = vec![0f32; b * t * HIDDEN];
        out.par_chunks_mut(HEAD_DIM)
            .enumerate()
            .for_each(|(idx, o)| {
                let (bt, h) = (idx / N_HEADS, idx % N_HEADS);
                let (bi, ti) = (bt / t, bt % t);
                let r = (bi * t + ti) * row;
                let mut q = [0f32; HEAD_DIM];
                q.copy_from_slice(&qkv[r + h * HEAD_DIM..r + (h + 1) * HEAD_DIM]);
                rope(&mut q, pos + ti);
                let n = pos + ti + 1; // causal: keys 0 ..= pos + ti
                let off = bi * HIDDEN + h * HEAD_DIM;
                let mut s: Vec<f32> = (0..n)
                    .map(|j| {
                        let kj = &keys[j * stride + off..j * stride + off + HEAD_DIM];
                        let mut acc = [0f32; 8];
                        for (a, c) in q.as_chunks::<8>().0.iter().zip(kj.as_chunks::<8>().0) {
                            for i in 0..8 {
                                acc[i] += a[i] * c[i];
                            }
                        }
                        acc.iter().sum::<f32>() * scale
                    })
                    .collect();
                let m = s.iter().fold(f32::NEG_INFINITY, |a, &v| a.max(v));
                let mut sum = 0f32;
                for v in s.iter_mut() {
                    *v = crate::fastops::exp_f(*v - m);
                    sum += *v;
                }
                let inv = 1.0 / sum;
                for (j, p) in s.iter().enumerate() {
                    let vj = &vals[j * stride + off..j * stride + off + HEAD_DIM];
                    let w = p * inv;
                    for (y, v) in o.iter_mut().zip(vj) {
                        *y += w * v;
                    }
                }
            });
        out
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

    /// The reference attention with candle ops: heads, RoPE, causal sdpa.
    fn candle_attention(qkv: &Tensor, pos_all: usize, cos: &Tensor, sin: &Tensor) -> Tensor {
        let (b, t, _) = qkv.dims3().unwrap();
        let part = |i: usize| {
            heads(
                &qkv.narrow(2, i * HIDDEN, HIDDEN)
                    .unwrap()
                    .contiguous()
                    .unwrap(),
                N_HEADS,
            )
            .unwrap()
        };
        let c = cos.narrow(0, 0, pos_all).unwrap();
        let s = sin.narrow(0, 0, pos_all).unwrap();
        let q = candle_nn::rotary_emb::rope(&part(0), &c, &s).unwrap();
        let k = candle_nn::rotary_emb::rope(&part(1), &c, &s).unwrap();
        let m: Vec<f32> = (0..t)
            .flat_map(|i| (0..t).map(move |j| if j > i { f32::NEG_INFINITY } else { 0.0 }))
            .collect();
        let mask = Tensor::from_vec(m, (1, 1, t, t), &Device::Cpu).unwrap();
        let a = sdpa(&q, &k, &part(2), 0.125, Some(&mask)).unwrap();
        merge_heads(&a).unwrap().reshape((b, t, HIDDEN)).unwrap()
    }

    #[test]
    fn kv_store_attention_matches_candle() {
        let dev = Device::Cpu;
        let (b, t) = (2usize, 6usize);
        let qkv = Tensor::randn(0f32, 1.0, (b, t, 3 * HIDDEN), &dev).unwrap();
        let inv = Tensor::from_vec(llama3_inv_freq(), (1, HEAD_DIM / 2), &dev).unwrap();
        let pos: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let freqs = Tensor::from_vec(pos, (16, 1), &dev)
            .unwrap()
            .broadcast_mul(&inv)
            .unwrap();
        let (cos, sin) = (freqs.cos().unwrap(), freqs.sin().unwrap());
        let (cv, sv): (Vec<f32>, Vec<f32>) = (
            cos.flatten_all().unwrap().to_vec1().unwrap(),
            sin.flatten_all().unwrap().to_vec1().unwrap(),
        );
        let want = candle_attention(&qkv, t, &cos, &sin);
        // Prefill 4 positions, then decode the last 2 one at a time.
        let mut kv = KvStore::with_capacity(b, 2);
        let pre = qkv.narrow(1, 0, 4).unwrap().contiguous().unwrap();
        let mut got = vec![Tensor::from_vec(
            kv.attend(
                &pre.flatten_all().unwrap().to_vec1().unwrap(),
                b,
                4,
                0,
                &cv,
                &sv,
            ),
            (b, 4, HIDDEN),
            &dev,
        )
        .unwrap()];
        for p in 4..t {
            let step = qkv.narrow(1, p, 1).unwrap().contiguous().unwrap();
            let o = kv.attend(
                &step.flatten_all().unwrap().to_vec1().unwrap(),
                b,
                1,
                p,
                &cv,
                &sv,
            );
            got.push(Tensor::from_vec(o, (b, 1, HIDDEN), &dev).unwrap());
        }
        assert_eq!(kv.len(), t);
        let got = Tensor::cat(&got, 1).unwrap();
        let d = (got - want).unwrap().abs().unwrap().flatten_all().unwrap();
        let d = d.max(0).unwrap().to_scalar::<f32>().unwrap();
        assert!(d < 1e-4, "max diff {d}");
    }

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
