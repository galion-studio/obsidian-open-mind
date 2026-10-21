// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! CAM++ speaker encoder (x-vector, 192 dims) used by S3Gen: Kaldi fbank
//! (80 bins, mean-normalised) -> 2-D residual front end (FCM) -> TDNN ->
//! three densely connected CAM blocks (context-aware masking) with transit
//! layers -> statistics pooling (mean + std) -> dense + batch norm.

use candle_core::{Device, Result, Tensor, D};
use candle_nn::VarBuilder;

use crate::nnx::{BatchNorm, Conv1d};

struct Conv2dBn {
    w: Tensor,
    bn: BatchNorm,
    stride_h: usize,
    pad: usize,
}

impl Conv2dBn {
    fn load(conv: &VarBuilder, bn: &VarBuilder, stride_h: usize, pad: usize) -> Result<Self> {
        Ok(Conv2dBn {
            w: conv.get_unchecked("weight")?,
            bn: BatchNorm::load(bn, true)?,
            stride_h,
            pad,
        })
    }
    /// x: (B, C, H=freq, W=time). Stride only along H (stride (s, 1)).
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let y = x.conv2d(&self.w, self.pad, 1, 1, 1)?;
        let y = if self.stride_h > 1 {
            let h = y.dim(2)?;
            let idx: Vec<u32> = (0..h).step_by(self.stride_h).map(|i| i as u32).collect();
            let idx = Tensor::new(idx.as_slice(), y.device())?;
            y.index_select(&idx, 2)?
        } else {
            y
        };
        self.bn.forward(&y)
    }
}

struct ResBlock2d {
    c1: Conv2dBn,
    c2: Conv2dBn,
    short: Option<Conv2dBn>,
}

impl ResBlock2d {
    fn load(vb: &VarBuilder, stride: usize, has_short: bool) -> Result<Self> {
        Ok(ResBlock2d {
            c1: Conv2dBn::load(&vb.pp("conv1"), &vb.pp("bn1"), stride, 1)?,
            c2: Conv2dBn::load(&vb.pp("conv2"), &vb.pp("bn2"), 1, 1)?,
            short: if has_short {
                Some(Conv2dBn::load(
                    &vb.pp("shortcut.0"),
                    &vb.pp("shortcut.1"),
                    stride,
                    0,
                )?)
            } else {
                None
            },
        })
    }
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let o = self.c1.forward(x)?.relu()?;
        let o = self.c2.forward(&o)?;
        let s = match &self.short {
            Some(s) => s.forward(x)?,
            None => x.clone(),
        };
        (o + s)?.relu()
    }
}

struct DenseLayer {
    bn1: BatchNorm,
    lin1: Conv1d,
    bn2: BatchNorm,
    local: Conv1d,
    cam1: Conv1d,
    cam2: Conv1d,
}

impl DenseLayer {
    fn load(vb: &VarBuilder, dilation: usize) -> Result<Self> {
        let c = vb.pp("cam_layer");
        Ok(DenseLayer {
            bn1: BatchNorm::load(&vb.pp("nonlinear1.batchnorm"), true)?,
            lin1: Conv1d::load(&vb.pp("linear1"), false, 1, 1, 0, 0)?,
            bn2: BatchNorm::load(&vb.pp("nonlinear2.batchnorm"), true)?,
            local: Conv1d::load(
                &c.pp("linear_local"),
                false,
                1,
                dilation,
                dilation,
                dilation,
            )?,
            cam1: Conv1d::load(&c.pp("linear1"), true, 1, 1, 0, 0)?,
            cam2: Conv1d::load(&c.pp("linear2"), true, 1, 1, 0, 0)?,
        })
    }
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let h = self.lin1.forward(&self.bn1.forward(x)?.relu()?)?;
        let h = self.bn2.forward(&h)?.relu()?;
        let y = self.local.forward(&h)?;
        // Context: global mean + 100-frame segment means broadcast back.
        let t = h.dim(2)?;
        let global = h.mean_keepdim(D::Minus1)?;
        let mut segs = Vec::new();
        let mut s = 0;
        while s < t {
            let len = 100.min(t - s);
            let m = h.narrow(2, s, len)?.mean_keepdim(D::Minus1)?;
            segs.push(m.broadcast_as((h.dim(0)?, h.dim(1)?, len))?.contiguous()?);
            s += len;
        }
        let seg = Tensor::cat(&segs, 2)?;
        let ctx = seg.broadcast_add(&global)?;
        let m = candle_nn::ops::sigmoid(&self.cam2.forward(&self.cam1.forward(&ctx)?.relu()?)?)?;
        y * m
    }
}

struct Transit {
    bn: BatchNorm,
    lin: Conv1d,
}

pub struct CamPlus {
    head_conv1: Conv2dBn,
    layer1: Vec<ResBlock2d>,
    layer2: Vec<ResBlock2d>,
    head_conv2: Conv2dBn,
    tdnn: Conv1d,
    tdnn_bn: BatchNorm,
    blocks: Vec<(Vec<DenseLayer>, Transit)>,
    out_bn: BatchNorm,
    dense: Conv1d,
    dense_bn: BatchNorm,
    dev: Device,
}

impl CamPlus {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let dev = vb.device().clone();
        let h = vb.pp("head");
        let x = vb.pp("xvector");
        let mut blocks = Vec::new();
        for (bi, (n, dil)) in [(12usize, 1usize), (24, 2), (16, 2)].iter().enumerate() {
            let b = x.pp(format!("block{}", bi + 1));
            let layers = (0..*n)
                .map(|i| DenseLayer::load(&b.pp(format!("tdnnd{}", i + 1)), *dil))
                .collect::<Result<_>>()?;
            let t = x.pp(format!("transit{}", bi + 1));
            blocks.push((
                layers,
                Transit {
                    bn: BatchNorm::load(&t.pp("nonlinear.batchnorm"), true)?,
                    lin: Conv1d::load(&t.pp("linear"), false, 1, 1, 0, 0)?,
                },
            ));
        }
        Ok(CamPlus {
            head_conv1: Conv2dBn::load(&h.pp("conv1"), &h.pp("bn1"), 1, 1)?,
            layer1: vec![
                ResBlock2d::load(&h.pp("layer1.0"), 2, true)?,
                ResBlock2d::load(&h.pp("layer1.1"), 1, false)?,
            ],
            layer2: vec![
                ResBlock2d::load(&h.pp("layer2.0"), 2, true)?,
                ResBlock2d::load(&h.pp("layer2.1"), 1, false)?,
            ],
            head_conv2: Conv2dBn::load(&h.pp("conv2"), &h.pp("bn2"), 2, 1)?,
            tdnn: Conv1d::load(&x.pp("tdnn.linear"), false, 2, 1, 2, 2)?,
            tdnn_bn: BatchNorm::load(&x.pp("tdnn.nonlinear.batchnorm"), true)?,
            blocks,
            out_bn: BatchNorm::load(&x.pp("out_nonlinear.batchnorm"), true)?,
            dense: Conv1d::load(&x.pp("dense.linear"), false, 1, 1, 0, 0)?,
            dense_bn: BatchNorm::load(&x.pp("dense.nonlinear.batchnorm"), false)?,
            dev,
        })
    }

    /// 192-dim x-vector of a 16 kHz clip.
    pub fn embed(&self, wav16: &[f32]) -> Result<Vec<f32>> {
        let fb = crate::dsp::kaldi_fbank(wav16, 16000.0, 80);
        let t = fb.len();
        if t < 4 {
            candle_core::bail!("reference clip too short for the speaker encoder");
        }
        let mut mean = [0f32; 80];
        for f in &fb {
            for (m, v) in mean.iter_mut().zip(f) {
                *m += v / t as f32;
            }
        }
        // (1, 1, 80, T): freq as H, time as W.
        let mut x = vec![0f32; 80 * t];
        for (fi, f) in fb.iter().enumerate() {
            for m in 0..80 {
                x[m * t + fi] = f[m] - mean[m];
            }
        }
        let x = Tensor::from_vec(x, (1, 1, 80, t), &self.dev)?;
        let mut h = self.head_conv1.forward(&x)?.relu()?;
        for b in self.layer1.iter().chain(&self.layer2) {
            h = b.forward(&h)?;
        }
        let h = self.head_conv2.forward(&h)?.relu()?; // (1, 32, 10, T)
        let (b, c, f, w) = h.dims4()?;
        let mut x = h.reshape((b, c * f, w))?;
        x = self.tdnn_bn.forward(&self.tdnn.forward(&x)?)?.relu()?;
        for (layers, tr) in &self.blocks {
            for l in layers {
                let y = l.forward(&x)?;
                x = Tensor::cat(&[&x, &y], 1)?;
            }
            x = tr.lin.forward(&tr.bn.forward(&x)?.relu()?)?;
        }
        let x = self.out_bn.forward(&x)?.relu()?;
        // Statistics pooling: mean and unbiased std over time.
        let n = x.dim(2)? as f64;
        let mean = x.mean_keepdim(D::Minus1)?;
        let var = (x.broadcast_sub(&mean)?.sqr()?.sum_keepdim(D::Minus1)? / (n - 1.0).max(1.0))?;
        let stats = Tensor::cat(&[&mean, &var.sqrt()?], 1)?; // (1, 1024, 1)
        let e = self.dense_bn.forward(&self.dense.forward(&stats)?)?;
        e.flatten_all()?.to_vec1::<f32>()
    }
}
