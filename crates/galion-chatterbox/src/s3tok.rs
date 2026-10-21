// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S3 speech tokenizer v2 (25 Hz, 6561 codes): 128-bin log-mel (16 kHz,
//! 100 frames/s) -> two stride-2 convs -> 6 transformer blocks with RoPE and
//! an FSMN memory branch (depthwise conv, kernel 31) -> FSQ (8 dims, 3
//! levels each). Used to tokenise the reference clip for T3 and S3Gen.

use candle_core::{Device, Module, Result, Tensor};
use candle_nn::{LayerNorm, Linear, VarBuilder};

use crate::nnx::{heads, layer_norm, linear, merge_heads, Conv1d};

const STATE: usize = 1280;
const N_HEAD: usize = 20;
const HD: usize = 64;

struct Block {
    attn_ln: LayerNorm,
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    fsmn: Tensor, // (1280, 31)
    mlp_ln: LayerNorm,
    fc1: Linear,
    fc2: Linear,
}

pub struct S3Tokenizer {
    filters: Vec<f32>,
    conv1: Conv1d,
    conv2: Conv1d,
    blocks: Vec<Block>,
    proj_down: Linear,
    dev: Device,
}

impl S3Tokenizer {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let dev = vb.device().clone();
        let filters = vb
            .get((128, 201), "_mel_filters")?
            .flatten_all()?
            .to_vec1::<f32>()?;
        let e = vb.pp("encoder");
        let mut blocks = Vec::new();
        for i in 0..6 {
            let b = e.pp(format!("blocks.{i}"));
            let a = b.pp("attn");
            blocks.push(Block {
                attn_ln: layer_norm(&b.pp("attn_ln"), STATE, 1e-5)?,
                q: linear(&a.pp("query"), STATE, STATE, true)?,
                k: linear(&a.pp("key"), STATE, STATE, false)?,
                v: linear(&a.pp("value"), STATE, STATE, true)?,
                out: linear(&a.pp("out"), STATE, STATE, true)?,
                fsmn: a
                    .pp("fsmn_block")
                    .get((STATE, 1, 31), "weight")?
                    .squeeze(1)?,
                mlp_ln: layer_norm(&b.pp("mlp_ln"), STATE, 1e-5)?,
                fc1: linear(&b.pp("mlp.0"), STATE, 4 * STATE, true)?,
                fc2: linear(&b.pp("mlp.2"), 4 * STATE, STATE, true)?,
            });
        }
        Ok(S3Tokenizer {
            filters,
            conv1: Conv1d::load(&e.pp("conv1"), true, 2, 1, 1, 1)?,
            conv2: Conv1d::load(&e.pp("conv2"), true, 2, 1, 1, 1)?,
            blocks,
            proj_down: linear(&vb.pp("quantizer._codebook.project_down"), STATE, 8, true)?,
            dev,
        })
    }

    /// Whisper-style log-mel: (128, frames) row-major, frames = len/160.
    pub fn log_mel(&self, wav: &[f32]) -> (Vec<f32>, usize) {
        let mut spec = crate::dsp::stft(wav, 400, 160, &crate::dsp::hann(400), true, true);
        spec.pop(); // drop the last frame, as the reference does
        let frames = spec.len();
        let mut m = vec![0f32; 128 * frames];
        for (f, p) in spec.iter().enumerate() {
            let mel = crate::dsp::apply_mel(&self.filters, 128, p);
            for (b, v) in mel.iter().enumerate() {
                m[b * frames + f] = v.max(1e-10).log10();
            }
        }
        let max = m.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        for v in m.iter_mut() {
            *v = ((*v).max(max - 8.0) + 4.0) / 4.0;
        }
        (m, frames)
    }

    fn rope_tables(&self, t: usize) -> Result<(Tensor, Tensor)> {
        let mut c = Vec::with_capacity(t * HD / 2);
        let mut s = Vec::with_capacity(t * HD / 2);
        for p in 0..t {
            for i in 0..HD / 2 {
                let f = 1.0 / 10000f64.powf(2.0 * i as f64 / HD as f64);
                c.push((p as f64 * f).cos() as f32);
                s.push((p as f64 * f).sin() as f32);
            }
        }
        Ok((
            Tensor::from_vec(c, (t, HD / 2), &self.dev)?,
            Tensor::from_vec(s, (t, HD / 2), &self.dev)?,
        ))
    }

    /// Tokenise a 16 kHz clip. `max_tokens` cuts the mel to 4 frames/token.
    pub fn tokenize(&self, wav16: &[f32], max_tokens: Option<usize>) -> Result<Vec<u32>> {
        let (mel, frames) = self.log_mel(wav16);
        let keep = max_tokens.map_or(frames, |m| frames.min(m * 4));
        if keep == 0 {
            return Ok(Vec::new());
        }
        let x = Tensor::from_vec(mel, (1, 128, frames), &self.dev)?
            .narrow(2, 0, keep)?
            .contiguous()?;
        let x = self.conv1.forward(&x)?.gelu_erf()?;
        let x = self.conv2.forward(&x)?.gelu_erf()?;
        let mut x = x.transpose(1, 2)?.contiguous()?; // (1, T, 1280)
        let t = x.dim(1)?;
        let (cos, sin) = self.rope_tables(t)?;
        let scale = (HD as f64).powf(-0.25);
        for b in &self.blocks {
            let h = b.attn_ln.forward(&x)?;
            let q = heads(&b.q.forward(&h)?, N_HEAD)?;
            let k = heads(&b.k.forward(&h)?, N_HEAD)?;
            let v_lin = b.v.forward(&h)?; // (1, T, 1280)
            let q = candle_nn::rotary_emb::rope(&q, &cos, &sin)?;
            let k = candle_nn::rotary_emb::rope(&k, &cos, &sin)?;
            // FSMN memory: depthwise conv (kernel 31, pad 15/15) over v, plus v.
            let vt = v_lin
                .transpose(1, 2)?
                .contiguous()?
                .pad_with_zeros(2, 15, 15)?; // (1,1280,T+30)
            let mut mem = v_lin.transpose(1, 2)?.contiguous()?;
            for j in 0..31 {
                let w = b.fsmn.narrow(1, j, 1)?.unsqueeze(0)?; // (1,1280,1)
                mem = (mem + vt.narrow(2, j, t)?.broadcast_mul(&w)?)?;
            }
            let mem = mem.transpose(1, 2)?;
            let v = heads(&v_lin, N_HEAD)?;
            let qs = (q * scale)?;
            let ks = (k * scale)?;
            let a = crate::nnx::sdpa(&qs, &ks, &v, 1.0, None)?;
            let a = b.out.forward(&merge_heads(&a)?)?;
            x = (x + (a + mem)?)?;
            let h = b
                .fc2
                .forward(&b.fc1.forward(&b.mlp_ln.forward(&x)?)?.gelu_erf()?)?;
            x = (x + h)?;
        }
        let h = self
            .proj_down
            .forward(&x)?
            .tanh()?
            .squeeze(0)?
            .to_vec2::<f32>()?;
        Ok(h.iter()
            .map(|d| {
                let mut idx = 0u32;
                let mut p = 1u32;
                for &v in d.iter() {
                    let q = (v * 0.999).round() + 1.0;
                    idx += q as u32 * p;
                    p *= 3;
                }
                idx
            })
            .collect())
    }
}
