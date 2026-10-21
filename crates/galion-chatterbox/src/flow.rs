// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! S3Gen token-to-mel ("flow"): speech tokens (prompt + generated) are
//! embedded, run through an upsampling conformer encoder (6 blocks at 25 Hz,
//! x2 upsampling, 4 blocks at 50 Hz, ESPnet relative-position attention) and
//! projected to 80 dims (mu). A conditional flow-matching decoder (a causal
//! 1-D U-Net with transformer blocks) then integrates noise to a mel
//! spectrogram with 10 Euler steps on a cosine schedule and classifier-free
//! guidance 0.7, conditioned on mu, the prompt mel and the speaker x-vector.

use candle_core::{Device, Module, Result, Tensor};
use candle_nn::{Embedding, LayerNorm, Linear, VarBuilder};

use crate::fastops::{attention_qkv, gelu_erf, silu, softmax_scaled, ChannelLayerNorm};
use crate::nnx::{heads, layer_norm, leaky_relu, linear, merge_heads, mish, Conv1d};
use crate::rng::Rng;

const D_MODEL: usize = 512;
const N_HEAD: usize = 8;
const D_K: usize = 64;

/// ESPnet relative positional encoding table for length `t`: rows for
/// relative positions t-1 .. -(t-1), shape (1, 2t-1, d).
fn rel_pos_emb(t: usize, d: usize, dev: &Device) -> Result<Tensor> {
    let mut v = Vec::with_capacity((2 * t - 1) * d);
    for r in 0..(2 * t - 1) {
        let p = (t as f64 - 1.0) - r as f64;
        for i in 0..d / 2 {
            let div = (-(2.0 * i as f64) * (10000f64.ln() / d as f64)).exp();
            v.push((p * div).sin() as f32);
            v.push((p * div).cos() as f32);
        }
    }
    Tensor::from_vec(v, (1, 2 * t - 1, d), dev)
}

/// Transformer-XL shift: out[.., i, j] = x[.., i, t-1-i+j], keep j < t.
fn rel_shift(x: &Tensor) -> Result<Tensor> {
    let (b, h, t, p) = x.dims4()?;
    let x = x.pad_with_zeros(3, 1, 0)?; // (b,h,t,p+1)
    let x = x.reshape((b, h, p + 1, t))?;
    let x = x.narrow(2, 1, p)?.contiguous()?.reshape((b, h, t, p))?;
    x.narrow(3, 0, p / 2 + 1)
}

struct RelAttn {
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    pos: Linear,
    bias_u: Tensor,
    bias_v: Tensor,
}

impl RelAttn {
    fn load(vb: &VarBuilder) -> Result<Self> {
        Ok(RelAttn {
            q: linear(&vb.pp("linear_q"), D_MODEL, D_MODEL, true)?,
            k: linear(&vb.pp("linear_k"), D_MODEL, D_MODEL, true)?,
            v: linear(&vb.pp("linear_v"), D_MODEL, D_MODEL, true)?,
            out: linear(&vb.pp("linear_out"), D_MODEL, D_MODEL, true)?,
            pos: linear(&vb.pp("linear_pos"), D_MODEL, D_MODEL, false)?,
            bias_u: vb.get((N_HEAD, D_K), "pos_bias_u")?,
            bias_v: vb.get((N_HEAD, D_K), "pos_bias_v")?,
        })
    }

    fn forward(&self, x: &Tensor, pos_emb: &Tensor) -> Result<Tensor> {
        let (b, t, _) = x.dims3()?;
        let q = self.q.forward(x)?.reshape((b, t, N_HEAD, D_K))?;
        let k = heads(&self.k.forward(x)?, N_HEAD)?;
        let v = heads(&self.v.forward(x)?, N_HEAD)?;
        let p = heads(&self.pos.forward(pos_emb)?, N_HEAD)?; // (1,h,2t-1,dk)
        let qu = q
            .broadcast_add(&self.bias_u.unsqueeze(0)?.unsqueeze(0)?)?
            .transpose(1, 2)?
            .contiguous()?;
        let qv = q
            .broadcast_add(&self.bias_v.unsqueeze(0)?.unsqueeze(0)?)?
            .transpose(1, 2)?
            .contiguous()?;
        let ac = qu.matmul(&k.t()?.contiguous()?)?;
        let p = p.broadcast_as((b, N_HEAD, 2 * t - 1, D_K))?.contiguous()?;
        let bd = qv.matmul(&p.t()?.contiguous()?)?; // (b,h,t,2t-1)
        let bd = rel_shift(&bd)?;
        let s = (ac + bd)?;
        let a = softmax_scaled(&s, 1.0 / (D_K as f32).sqrt())?.matmul(&v)?;
        self.out.forward(&merge_heads(&a)?)
    }
}

struct ConformerLayer {
    norm_mha: LayerNorm,
    attn: RelAttn,
    norm_ff: LayerNorm,
    w1: Linear,
    w2: Linear,
}

impl ConformerLayer {
    fn load(vb: &VarBuilder) -> Result<Self> {
        Ok(ConformerLayer {
            norm_mha: layer_norm(&vb.pp("norm_mha"), D_MODEL, 1e-12)?,
            attn: RelAttn::load(&vb.pp("self_attn"))?,
            norm_ff: layer_norm(&vb.pp("norm_ff"), D_MODEL, 1e-12)?,
            w1: linear(&vb.pp("feed_forward").pp("w_1"), D_MODEL, 2048, true)?,
            w2: linear(&vb.pp("feed_forward").pp("w_2"), 2048, D_MODEL, true)?,
        })
    }
    fn forward(&self, x: &Tensor, pos: &Tensor) -> Result<Tensor> {
        let x = (x + self.attn.forward(&self.norm_mha.forward(x)?, pos)?)?;
        let h = silu(&self.w1.forward(&self.norm_ff.forward(&x)?)?)?;
        x + self.w2.forward(&h)?
    }
}

struct LinearEmbed {
    lin: Linear,
    ln: LayerNorm,
}

impl LinearEmbed {
    fn load(vb: &VarBuilder) -> Result<Self> {
        Ok(LinearEmbed {
            lin: linear(&vb.pp("out.0"), D_MODEL, D_MODEL, true)?,
            ln: layer_norm(&vb.pp("out.1"), D_MODEL, 1e-5)?,
        })
    }
    /// Returns (x * sqrt(d), relative position table).
    fn forward(&self, x: &Tensor) -> Result<(Tensor, Tensor)> {
        let x = self.ln.forward(&self.lin.forward(x)?)?;
        let t = x.dim(1)?;
        let pos = rel_pos_emb(t, D_MODEL, x.device())?;
        Ok(((x * (D_MODEL as f64).sqrt())?, pos))
    }
}

pub struct Encoder {
    embed: LinearEmbed,
    look1: Conv1d,
    look2: Conv1d,
    encoders: Vec<ConformerLayer>,
    up_conv: Conv1d,
    up_embed: LinearEmbed,
    up_encoders: Vec<ConformerLayer>,
    after_norm: LayerNorm,
}

impl Encoder {
    fn load(vb: &VarBuilder) -> Result<Self> {
        let pl = vb.pp("pre_lookahead_layer");
        Ok(Encoder {
            embed: LinearEmbed::load(&vb.pp("embed"))?,
            look1: Conv1d::load(&pl.pp("conv1"), true, 1, 1, 0, 3)?,
            look2: Conv1d::load(&pl.pp("conv2"), true, 1, 1, 2, 0)?,
            encoders: (0..6)
                .map(|i| ConformerLayer::load(&vb.pp(format!("encoders.{i}"))))
                .collect::<Result<_>>()?,
            up_conv: Conv1d::load(&vb.pp("up_layer").pp("conv"), true, 1, 1, 4, 0)?,
            up_embed: LinearEmbed::load(&vb.pp("up_embed"))?,
            up_encoders: (0..4)
                .map(|i| ConformerLayer::load(&vb.pp(format!("up_encoders.{i}"))))
                .collect::<Result<_>>()?,
            after_norm: layer_norm(&vb.pp("after_norm"), D_MODEL, 1e-5)?,
        })
    }

    /// x: (B, T, 512) -> (B, 2T, 512)
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (mut x, pos) = self.embed.forward(x)?;
        // Pre-lookahead: conv over 3 future frames, leaky ReLU, causal conv.
        let c = x.transpose(1, 2)?.contiguous()?;
        let h = leaky_relu(&self.look1.forward(&c)?, 0.01)?;
        let h = self.look2.forward(&h)?.transpose(1, 2)?;
        x = (x + h)?;
        for l in &self.encoders {
            x = l.forward(&x, &pos)?;
        }
        // Nearest x2 upsampling, causal conv (pad 4 left, kernel 5).
        let t = x.dim(1)?;
        let c = x.transpose(1, 2)?.contiguous()?.upsample_nearest1d(2 * t)?;
        let c = self.up_conv.forward(&c)?;
        let (mut x, pos) = self.up_embed.forward(&c.transpose(1, 2)?.contiguous()?)?;
        for l in &self.up_encoders {
            x = l.forward(&x, &pos)?;
        }
        self.after_norm.forward(&x)
    }
}

// ---- conditional flow-matching decoder (U-Net estimator) ----

struct CausalBlock {
    conv: Conv1d,
    ln: ChannelLayerNorm,
}

impl CausalBlock {
    fn load(vb: &VarBuilder, dim_out: usize) -> Result<Self> {
        Ok(CausalBlock {
            conv: Conv1d::load(&vb.pp("block.0"), true, 1, 1, 2, 0)?,
            ln: {
                let l = vb.pp("block.2");
                ChannelLayerNorm::from_tensors(
                    &l.get(dim_out, "weight")?,
                    &l.get(dim_out, "bias")?,
                    1e-5,
                )?
            },
        })
    }
    // The reference multiplies by the padding mask before and after; at
    // inference the batch is one utterance (+ its CFG copy) of one length,
    // so the mask is all ones and those products are skipped (voice-speed).
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let h = self.conv.forward(x)?;
        mish(&self.ln.forward(&h)?)
    }
}

struct Resnet {
    b1: CausalBlock,
    b2: CausalBlock,
    mlp: Linear,
    res: Conv1d,
}

impl Resnet {
    fn load(vb: &VarBuilder, dim_out: usize) -> Result<Self> {
        Ok(Resnet {
            b1: CausalBlock::load(&vb.pp("block1"), dim_out)?,
            b2: CausalBlock::load(&vb.pp("block2"), dim_out)?,
            mlp: linear(&vb.pp("mlp.1"), 1024, dim_out, true)?,
            res: Conv1d::load(&vb.pp("res_conv"), true, 1, 1, 0, 0)?,
        })
    }
    fn forward(&self, x: &Tensor, temb: &Tensor) -> Result<Tensor> {
        let _p = crate::prof::span("flow.resnet");
        let h = self.b1.forward(x)?;
        let t = self.mlp.forward(&mish(temb)?)?.unsqueeze(2)?;
        let h = h.broadcast_add(&t)?;
        let h = self.b2.forward(&h)?;
        h + self.res.forward(x)?
    }
}

struct TBlock {
    norm1: LayerNorm,
    /// to_q, to_k, to_v stacked (256 -> 1536, no bias).
    qkv: Linear,
    out: Linear,
    norm3: LayerNorm,
    ff1: Linear,
    ff2: Linear,
}

impl TBlock {
    fn load(vb: &VarBuilder) -> Result<Self> {
        let a = vb.pp("attn1");
        Ok(TBlock {
            norm1: layer_norm(&vb.pp("norm1"), 256, 1e-5)?,
            qkv: Linear::new(
                Tensor::cat(
                    &[
                        a.pp("to_q").get((512, 256), "weight")?,
                        a.pp("to_k").get((512, 256), "weight")?,
                        a.pp("to_v").get((512, 256), "weight")?,
                    ],
                    0,
                )?,
                None,
            ),
            out: linear(&a.pp("to_out.0"), 512, 256, true)?,
            norm3: layer_norm(&vb.pp("norm3"), 256, 1e-5)?,
            ff1: linear(&vb.pp("ff.net.0.proj"), 256, 1024, true)?,
            ff2: linear(&vb.pp("ff.net.2"), 1024, 256, true)?,
        })
    }
    /// x: (B, T, 256)
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let p = crate::prof::span("flow.tblock.attn");
        let h = self.norm1.forward(x)?;
        let a = attention_qkv(&self.qkv.forward(&h)?, 8, 64, 1.0 / 8.0)?;
        let x = (x + self.out.forward(&a)?)?;
        drop(p);
        let _p = crate::prof::span("flow.tblock.ff");
        let h = gelu_erf(&self.ff1.forward(&self.norm3.forward(&x)?)?)?;
        x + self.ff2.forward(&h)?
    }
}

struct Stage {
    resnet: Resnet,
    tblocks: Vec<TBlock>,
    post: Option<Conv1d>,
}

impl Stage {
    fn load(vb: &VarBuilder, with_post: bool) -> Result<Self> {
        Ok(Stage {
            resnet: Resnet::load(&vb.pp("0"), 256)?,
            tblocks: (0..4)
                .map(|i| TBlock::load(&vb.pp(format!("1.{i}"))))
                .collect::<Result<_>>()?,
            post: if with_post {
                Some(Conv1d::load(&vb.pp("2"), true, 1, 1, 2, 0)?)
            } else {
                None
            },
        })
    }
    fn forward(&self, x: &Tensor, temb: &Tensor) -> Result<(Tensor, Tensor)> {
        let x = self.resnet.forward(x, temb)?;
        let mut h = x.transpose(1, 2)?.contiguous()?;
        for b in &self.tblocks {
            h = b.forward(&h)?;
        }
        let skip = h.transpose(1, 2)?.contiguous()?;
        let out = match &self.post {
            Some(c) => c.forward(&skip)?,
            None => skip.clone(),
        };
        Ok((skip, out))
    }
}

pub struct Estimator {
    t1: Linear,
    t2: Linear,
    down: Stage,
    mids: Vec<Stage>,
    up: Stage,
    final_block: CausalBlock,
    final_proj: Conv1d,
}

fn sinusoidal(t: f32, dim: usize) -> Vec<f32> {
    let half = dim / 2;
    let e = (10000f64).ln() / (half as f64 - 1.0);
    let mut v = vec![0f32; dim];
    for i in 0..half {
        let a = 1000.0 * t as f64 * (-(i as f64) * e).exp();
        v[i] = a.sin() as f32;
        v[i + half] = a.cos() as f32;
    }
    v
}

impl Estimator {
    fn load(vb: &VarBuilder) -> Result<Self> {
        Ok(Estimator {
            t1: linear(&vb.pp("time_mlp.linear_1"), 320, 1024, true)?,
            t2: linear(&vb.pp("time_mlp.linear_2"), 1024, 1024, true)?,
            down: Stage::load(&vb.pp("down_blocks.0"), true)?,
            mids: (0..12)
                .map(|i| Stage::load(&vb.pp(format!("mid_blocks.{i}")), false))
                .collect::<Result<_>>()?,
            up: Stage::load(&vb.pp("up_blocks.0"), true)?,
            final_block: CausalBlock::load(&vb.pp("final_block"), 256)?,
            final_proj: Conv1d::load(&vb.pp("final_proj"), true, 1, 1, 0, 0)?,
        })
    }

    /// x, mu, cond: (B, 80, T); spks (B, 80); t scalar. No padding mask: all
    /// rows have the same length (see [`CausalBlock::forward`]).
    fn forward(
        &self,
        x: &Tensor,
        mu: &Tensor,
        t: f32,
        spks: &Tensor,
        cond: &Tensor,
    ) -> Result<Tensor> {
        let b = x.dim(0)?;
        let tt = x.dim(2)?;
        let temb = Tensor::from_vec(sinusoidal(t, 320), (1, 320), x.device())?;
        let temb = self.t2.forward(&self.t1.forward(&temb)?.silu()?)?;
        let temb = temb.broadcast_as((b, 1024))?.contiguous()?;
        let s = spks.unsqueeze(2)?.broadcast_as((b, 80, tt))?;
        let h = Tensor::cat(&[x, mu, &s.contiguous()?, cond], 1)?;
        let (skip, mut h) = self.down.forward(&h, &temb)?;
        for m in &self.mids {
            h = m.forward(&h, &temb)?.1;
        }
        let h = Tensor::cat(&[&h.narrow(2, 0, skip.dim(2)?)?, &skip], 1)?;
        let (_, h) = self.up.forward(&h, &temb)?;
        let h = self.final_block.forward(&h)?;
        self.final_proj.forward(&h)
    }
}

pub struct Flow {
    input_embedding: Embedding,
    spk_affine: Linear,
    encoder: Encoder,
    encoder_proj: Linear,
    estimator: Estimator,
    dev: Device,
}

/// Prompt (reference) conditioning for S3Gen.
#[derive(Clone)]
pub struct GenCond {
    pub prompt_tokens: Vec<u32>,
    /// (frames, 80) log-mel of the reference at 24 kHz, row-major.
    pub prompt_feat: Vec<f32>,
    pub prompt_frames: usize,
    /// CAMPPlus x-vector, 192 values.
    pub embedding: Vec<f32>,
}

impl Flow {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let dev = vb.device().clone();
        Ok(Flow {
            input_embedding: candle_nn::embedding(6561, 512, vb.pp("input_embedding"))?,
            spk_affine: linear(&vb.pp("spk_embed_affine_layer"), 192, 80, true)?,
            encoder: Encoder::load(&vb.pp("encoder"))?,
            encoder_proj: linear(&vb.pp("encoder_proj"), 512, 80, true)?,
            estimator: Estimator::load(&vb.pp("decoder").pp("estimator"))?,
            dev,
        })
    }

    /// Generated tokens -> mel (1, 80, 2 * tokens.len()).
    pub fn inference(
        &self,
        tokens: &[u32],
        c: &GenCond,
        n_steps: usize,
        rng: &mut Rng,
    ) -> Result<Tensor> {
        let dev = &self.dev;
        let emb = Tensor::from_vec(c.embedding.clone(), (1, 192), dev)?;
        let spks = self.spk_affine.forward(&crate::nnx::l2norm(&emb)?)?; // (1,80)
        let mut all = c.prompt_tokens.clone();
        all.extend_from_slice(tokens);
        let ids = Tensor::new(all.as_slice(), dev)?.unsqueeze(0)?;
        let x = self.input_embedding.forward(&ids)?;
        let p = crate::prof::span("flow.encoder");
        let h = self.encoder.forward(&x)?;
        drop(p);
        let mu = self
            .encoder_proj
            .forward(&h)?
            .transpose(1, 2)?
            .contiguous()?; // (1,80,T)
        let t_all = mu.dim(2)?;
        let mel1 = c.prompt_frames.min(t_all);
        let mel2 = t_all - mel1;
        let mut condv = vec![0f32; 80 * t_all];
        for f in 0..mel1 {
            for m in 0..80 {
                condv[m * t_all + f] = c.prompt_feat[f * 80 + m];
            }
        }
        let cond = Tensor::from_vec(condv, (1, 80, t_all), dev)?;
        let mut x = Tensor::from_vec(rng.normals(80 * t_all), (1, 80, t_all), dev)?;
        let mu_in = Tensor::cat(&[&mu, &mu.zeros_like()?], 0)?;
        let spk_in = Tensor::cat(&[&spks, &spks.zeros_like()?], 0)?;
        let cond_in = Tensor::cat(&[&cond, &cond.zeros_like()?], 0)?;
        let ts: Vec<f32> = (0..=n_steps)
            .map(|i| {
                let l = i as f64 / n_steps as f64;
                (1.0 - (l * 0.5 * std::f64::consts::PI).cos()) as f32
            })
            .collect();
        for w in ts.windows(2) {
            let (t, r) = (w[0], w[1]);
            let _p = crate::prof::span("flow.step");
            let x_in = Tensor::cat(&[&x, &x], 0)?;
            let d = self
                .estimator
                .forward(&x_in, &mu_in, t, &spk_in, &cond_in)?;
            let dc = d.narrow(0, 0, 1)?;
            let du = d.narrow(0, 1, 1)?;
            let dxdt = ((dc * 1.7)? - (du * 0.7)?)?;
            x = (x + (dxdt * (r - t) as f64)?)?;
        }
        x.narrow(2, mel1, mel2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_shift_picks_relative_positions() {
        let dev = Device::Cpu;
        let t = 3;
        // x[i][k] = 10*i + k
        let v: Vec<f32> = (0..t)
            .flat_map(|i| (0..2 * t - 1).map(move |k| (10 * i + k) as f32))
            .collect();
        let x = Tensor::from_vec(v, (1, 1, t, 2 * t - 1), &dev).unwrap();
        let y = rel_shift(&x)
            .unwrap()
            .squeeze(0)
            .unwrap()
            .squeeze(0)
            .unwrap();
        let y = y.to_vec2::<f32>().unwrap();
        for (i, row) in y.iter().enumerate() {
            for (j, &val) in row.iter().enumerate() {
                assert_eq!(val, (10 * i + (t - 1 - i + j)) as f32, "{i},{j}");
            }
        }
    }

    #[test]
    fn rel_pos_table_centre_is_position_zero() {
        let p = rel_pos_emb(4, 8, &Device::Cpu).unwrap().squeeze(0).unwrap();
        let p = p.to_vec2::<f32>().unwrap();
        assert_eq!(p.len(), 7);
        assert_eq!(p[3][0], 0.0); // sin(0)
        assert_eq!(p[3][1], 1.0); // cos(0)
        assert!((p[0][0] - 3f32.sin()).abs() < 1e-6);
        let s = sinusoidal(0.5, 320);
        assert!((s[0] - 500f32.sin()).abs() < 1e-3);
    }
}
