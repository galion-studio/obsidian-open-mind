// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Layer helpers on top of candle: linear / layer norm loaders, 1-D
//! convolution with asymmetric padding and weight-norm parametrisation,
//! folded batch norm, activations (Mish, Snake, ELU) and multi-head
//! attention used by several sub-models.

use candle_core::{DType, Device, Module, Result, Tensor, D};
use candle_nn::{LayerNorm, Linear, VarBuilder};

/// Load safetensors files (memory-mapped) as an f32 CPU VarBuilder.
pub fn load_vb(paths: &[std::path::PathBuf], dev: &Device) -> Result<VarBuilder<'static>> {
    // SAFETY: the model files are opened read-only and are not modified while
    // the process runs; candle copies every tensor out of the map on load.
    #[allow(unsafe_code)]
    unsafe {
        VarBuilder::from_mmaped_safetensors(paths, DType::F32, dev)
    }
}

pub fn linear(vb: &VarBuilder, i: usize, o: usize, bias: bool) -> Result<Linear> {
    if bias {
        candle_nn::linear(i, o, vb.clone())
    } else {
        candle_nn::linear_no_bias(i, o, vb.clone())
    }
}

pub fn layer_norm(vb: &VarBuilder, dim: usize, eps: f64) -> Result<LayerNorm> {
    candle_nn::layer_norm(
        dim,
        candle_nn::LayerNormConfig {
            eps,
            ..Default::default()
        },
        vb.clone(),
    )
}

/// Weight-norm parametrised weight: `g * v / ||v||` (norm over all dims but 0).
pub fn wn_weight(vb: &VarBuilder) -> Result<Tensor> {
    let p = vb.pp("parametrizations").pp("weight");
    let g = p.get_unchecked("original0")?;
    let v = p.get_unchecked("original1")?;
    let norm = v.sqr()?.sum_keepdim(1)?.sum_keepdim(2)?.sqrt()?;
    v.broadcast_mul(&g.broadcast_div(&norm)?)
}

/// 1-D convolution on (B, C, T) with explicit left/right zero padding.
#[derive(Clone)]
pub struct Conv1d {
    pub w: Tensor,
    pub b: Option<Tensor>,
    pub stride: usize,
    pub dilation: usize,
    pub pad_l: usize,
    pub pad_r: usize,
}

impl Conv1d {
    /// Plain `weight` (+ optional `bias`) conv.
    pub fn load(
        vb: &VarBuilder,
        bias: bool,
        stride: usize,
        dilation: usize,
        pad_l: usize,
        pad_r: usize,
    ) -> Result<Self> {
        let w = vb.get_unchecked("weight")?;
        let b = if bias {
            Some(vb.get_unchecked("bias")?)
        } else {
            None
        };
        Ok(Conv1d {
            w,
            b,
            stride,
            dilation,
            pad_l,
            pad_r,
        })
    }

    /// Weight-norm conv (`parametrizations.weight.original{0,1}` + `bias`).
    pub fn load_wn(
        vb: &VarBuilder,
        stride: usize,
        dilation: usize,
        pad_l: usize,
        pad_r: usize,
    ) -> Result<Self> {
        Ok(Conv1d {
            w: wn_weight(vb)?,
            b: Some(vb.get_unchecked("bias")?),
            stride,
            dilation,
            pad_l,
            pad_r,
        })
    }

    /// Symmetric "same" padding for an odd kernel.
    pub fn same(vb: &VarBuilder, bias: bool, k: usize, dilation: usize) -> Result<Self> {
        let p = (k - 1) / 2 * dilation;
        Self::load(vb, bias, 1, dilation, p, p)
    }

    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let x = if self.pad_l == self.pad_r {
            x.conv1d(&self.w, self.pad_l, self.stride, self.dilation, 1)?
        } else {
            x.pad_with_zeros(2, self.pad_l, self.pad_r)?.conv1d(
                &self.w,
                0,
                self.stride,
                self.dilation,
                1,
            )?
        };
        match &self.b {
            Some(b) => x.broadcast_add(&b.reshape((1, (), 1))?),
            None => Ok(x),
        }
    }
}

/// Transposed 1-D convolution (weight-norm), PyTorch semantics.
pub struct ConvT1d {
    w: Tensor,
    b: Tensor,
    stride: usize,
    padding: usize,
}

impl ConvT1d {
    pub fn load_wn(vb: &VarBuilder, stride: usize, padding: usize) -> Result<Self> {
        Ok(ConvT1d {
            w: wn_weight(vb)?,
            b: vb.get_unchecked("bias")?,
            stride,
            padding,
        })
    }
    pub fn load(vb: &VarBuilder, stride: usize, padding: usize) -> Result<Self> {
        Ok(ConvT1d {
            w: vb.get_unchecked("weight")?,
            b: vb.get_unchecked("bias")?,
            stride,
            padding,
        })
    }
    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        x.conv_transpose1d(&self.w, self.padding, 0, self.stride, 1, 1)?
            .broadcast_add(&self.b.reshape((1, (), 1))?)
    }
}

/// Eval-mode batch norm folded into `x * scale + shift` over dim 1.
pub struct BatchNorm {
    scale: Tensor,
    shift: Tensor,
}

impl BatchNorm {
    pub fn load(vb: &VarBuilder, affine: bool) -> Result<Self> {
        let mean = vb.get_unchecked("running_mean")?;
        let var = vb.get_unchecked("running_var")?;
        let inv = var.affine(1.0, 1e-5)?.sqrt()?.recip()?;
        let (scale, shift) = if affine {
            let w = vb.get_unchecked("weight")?;
            let b = vb.get_unchecked("bias")?;
            let s = (w * &inv)?;
            let sh = (b - (&mean * &s)?)?;
            (s, sh)
        } else {
            let sh = (&mean * &inv)?.neg()?;
            (inv, sh)
        };
        Ok(BatchNorm { scale, shift })
    }

    /// x: (B, C, ...) with any number of trailing dims.
    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mut shape = vec![1usize, self.scale.dim(0)?];
        shape.extend(std::iter::repeat_n(1, x.rank() - 2));
        x.broadcast_mul(&self.scale.reshape(shape.as_slice())?)?
            .broadcast_add(&self.shift.reshape(shape.as_slice())?)
    }
}

pub fn mish(x: &Tensor) -> Result<Tensor> {
    // softplus(x) = ln(1 + e^x); overflow to +inf still gives tanh(inf) = 1.
    x * x.exp()?.affine(1.0, 1.0)?.log()?.tanh()?
}

/// Snake: x + 1/alpha * sin^2(alpha * x), alpha per channel, x: (B, C, T).
pub fn snake(x: &Tensor, alpha: &Tensor) -> Result<Tensor> {
    let a = alpha.reshape((1, (), 1))?;
    let s = x.broadcast_mul(&a)?.sin()?.sqr()?;
    x + s.broadcast_div(&a.affine(1.0, 1e-9)?)?
}

pub fn leaky_relu(x: &Tensor, slope: f64) -> Result<Tensor> {
    // max(x, slope * x) for 0 < slope < 1.
    x.maximum(&x.affine(slope, 0.0)?)
}

/// Linear over the last dim of any-rank input.
pub fn lin(l: &Linear, x: &Tensor) -> Result<Tensor> {
    l.forward(x)
}

/// Scaled dot-product attention: q (B, H, Tq, D), k/v (B, H, Tk, D),
/// optional additive bias broadcastable to (B, H, Tq, Tk).
pub fn sdpa(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    scale: f64,
    bias: Option<&Tensor>,
) -> Result<Tensor> {
    let mut s = (q.contiguous()?.matmul(&k.t()?.contiguous()?)? * scale)?;
    if let Some(b) = bias {
        s = s.broadcast_add(b)?;
    }
    let p = candle_nn::ops::softmax_last_dim(&s)?;
    p.matmul(&v.contiguous()?)
}

/// (B, T, H*D) -> (B, H, T, D)
pub fn heads(x: &Tensor, h: usize) -> Result<Tensor> {
    let (b, t, c) = x.dims3()?;
    x.reshape((b, t, h, c / h))?.transpose(1, 2)?.contiguous()
}

/// (B, H, T, D) -> (B, T, H*D)
pub fn merge_heads(x: &Tensor) -> Result<Tensor> {
    let (b, h, t, d) = x.dims4()?;
    x.transpose(1, 2)?.contiguous()?.reshape((b, t, h * d))
}

/// L2-normalise along the last dim.
pub fn l2norm(x: &Tensor) -> Result<Tensor> {
    let n = x
        .sqr()?
        .sum_keepdim(D::Minus1)?
        .sqrt()?
        .affine(1.0, 1e-12)?;
    x.broadcast_div(&n)
}
