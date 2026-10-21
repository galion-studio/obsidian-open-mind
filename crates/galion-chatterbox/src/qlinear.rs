// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Weight-only int8 linear layers for T3 (voice-speed).
//!
//! T3 decodes one speech token at a time for a batch of two rows (the
//! conditional and the classifier-free-guidance branch). Each step multiplies
//! two 1024-wide vectors with all 0.5 B weights, so the step is bound by
//! memory bandwidth: in f32 that is 2.1 GB read per token (~84 ms on the
//! build box). Stored as int8 with one f32 scale per 32 weights (the
//! llama.cpp `Q8_0` layout, 1.125 bytes per weight) the same step reads
//! 0.6 GB. Activations and accumulation stay f32.
//!
//! The kernel is plain portable Rust (no intrinsics, no asm, no target-cpu
//! flags) written so the compiler auto-vectorises the int8 -> f32 conversion
//! and the dot products for the baseline target; rows are split across
//! threads with rayon. For more than [`GEMV_MAX_ROWS`] input rows (the
//! one-off prompt prefill) the weight is expanded back to f32 and multiplied
//! with candle's matmul.

use candle_core::{Device, Module, Result, Tensor};
use candle_nn::Linear;
use rayon::prelude::*;

/// Weights per scale.
pub const GROUP: usize = 32;
/// Up to this many input rows use the int8 kernel; more use f32 matmul.
pub const GEMV_MAX_ROWS: usize = 8;
const ROWS_PER_TASK: usize = 32;

/// How the T3 transformer weights are held in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum T3Weights {
    /// The checkpoint's f32 weights (2.1 GB, reference numerics).
    F32,
    /// Int8 + per-32 f32 scale (0.6 GB, ~3x faster decoding).
    #[default]
    Q8,
}

impl T3Weights {
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "f32" => Ok(T3Weights::F32),
            "q8" | "int8" => Ok(T3Weights::Q8),
            other => Err(format!("unknown T3 weights '{other}' (f32 or q8)")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            T3Weights::F32 => "f32",
            T3Weights::Q8 => "q8",
        }
    }
}

/// `y = x W^T (+ b)` with W (out, in) quantised to int8 per group of 32.
pub struct QLinear {
    out_dim: usize,
    in_dim: usize,
    q: Vec<i8>,
    scales: Vec<f32>,
    bias: Option<Vec<f32>>,
    dev: Device,
}

impl QLinear {
    /// Quantise a (out, in) f32 weight; `in` must be a multiple of 32.
    pub fn quantize(w: &Tensor, bias: Option<&Tensor>) -> Result<Self> {
        let (out_dim, in_dim) = w.dims2()?;
        if in_dim % GROUP != 0 {
            candle_core::bail!("qlinear: in dim {in_dim} not a multiple of {GROUP}");
        }
        let wv = w.flatten_all()?.to_vec1::<f32>()?;
        let groups = in_dim / GROUP;
        let mut q = vec![0i8; out_dim * in_dim];
        let mut scales = vec![0f32; out_dim * groups];
        q.par_chunks_mut(in_dim)
            .zip(scales.par_chunks_mut(groups))
            .zip(wv.par_chunks(in_dim))
            .for_each(|((qr, sr), wr)| {
                for ((qg, s), wg) in qr
                    .as_chunks_mut::<GROUP>()
                    .0
                    .iter_mut()
                    .zip(sr.iter_mut())
                    .zip(wr.as_chunks::<GROUP>().0)
                {
                    let amax = wg.iter().fold(0f32, |a, v| a.max(v.abs()));
                    let d = amax / 127.0;
                    *s = d;
                    let inv = if d > 0.0 { 1.0 / d } else { 0.0 };
                    for (o, v) in qg.iter_mut().zip(wg) {
                        *o = (v * inv).round().clamp(-127.0, 127.0) as i8;
                    }
                }
            });
        let bias = match bias {
            Some(b) => Some(b.to_vec1::<f32>()?),
            None => None,
        };
        Ok(QLinear {
            out_dim,
            in_dim,
            q,
            scales,
            bias,
            dev: w.device().clone(),
        })
    }

    pub fn from_linear(l: &Linear) -> Result<Self> {
        Self::quantize(l.weight(), l.bias())
    }

    /// Several layers that read the same input, stacked along the output
    /// dim (e.g. q, k, v): one pass over the input, outputs concatenated.
    pub fn stacked(ls: &[&Linear]) -> Result<Self> {
        let ws: Vec<&Tensor> = ls.iter().map(|l| l.weight()).collect();
        let w = Tensor::cat(&ws, 0)?;
        let bias = if ls.iter().all(|l| l.bias().is_some()) {
            let bs: Vec<&Tensor> = ls.iter().filter_map(|l| l.bias()).collect();
            Some(Tensor::cat(&bs, 0)?)
        } else if ls.iter().any(|l| l.bias().is_some()) {
            candle_core::bail!("qlinear: stacked layers must all or none have a bias")
        } else {
            None
        };
        Self::quantize(&w, bias.as_ref())
    }

    pub fn out_dim(&self) -> usize {
        self.out_dim
    }

    /// Bytes held (weights + scales).
    pub fn bytes(&self) -> usize {
        self.q.len() + 4 * self.scales.len()
    }

    /// The weight expanded back to f32, (out, in).
    pub fn dequantize(&self) -> Result<Tensor> {
        let mut w = vec![0f32; self.out_dim * self.in_dim];
        let groups = self.in_dim / GROUP;
        w.par_chunks_mut(self.in_dim)
            .zip(self.q.par_chunks(self.in_dim))
            .zip(self.scales.par_chunks(groups))
            .for_each(|((wr, qr), sr)| dequant_row(qr, sr, wr));
        Tensor::from_vec(w, (self.out_dim, self.in_dim), &self.dev)
    }

    fn gemv(&self, x: &[f32], m: usize) -> Vec<f32> {
        let (n, k) = (self.out_dim, self.in_dim);
        let groups = k / GROUP;
        // Computed as (n, m) so each task owns a contiguous block of rows.
        let mut yt = vec![0f32; n * m];
        yt.par_chunks_mut(ROWS_PER_TASK * m)
            .enumerate()
            .for_each(|(ti, out)| {
                // Expand one row to f32 in L1, then one dot per input row: measured
                // 2.5-3x faster than converting inside a two-row dot (that form
                // does not auto-vectorise for the baseline target).
                let mut row = vec![0f32; k];
                let r0 = ti * ROWS_PER_TASK;
                for (ri, o) in out.chunks_exact_mut(m).enumerate() {
                    let r = r0 + ri;
                    dequant_row(
                        &self.q[r * k..(r + 1) * k],
                        &self.scales[r * groups..(r + 1) * groups],
                        &mut row,
                    );
                    for (mi, y) in o.iter_mut().enumerate() {
                        *y = dot(&row, &x[mi * k..(mi + 1) * k]);
                    }
                }
            });
        let mut y = vec![0f32; m * n];
        for r in 0..n {
            for mi in 0..m {
                y[mi * n + r] = yt[r * m + mi];
            }
        }
        if let Some(b) = &self.bias {
            for yr in y.chunks_exact_mut(n) {
                yr.iter_mut().zip(b).for_each(|(v, b)| *v += b);
            }
        }
        y
    }
}

#[inline]
fn dequant_row(q: &[i8], s: &[f32], out: &mut [f32]) {
    for ((qg, d), og) in q
        .as_chunks::<GROUP>()
        .0
        .iter()
        .zip(s)
        .zip(out.as_chunks_mut::<GROUP>().0)
    {
        for (o, v) in og.iter_mut().zip(qg) {
            *o = *v as f32 * d;
        }
    }
}

/// Dot product with 16 independent lanes (vectorisable without fast-math).
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    const L: usize = 16;
    let mut acc = [0f32; L];
    let ((ac, ar), (bc, br)) = (a.as_chunks::<L>(), b.as_chunks::<L>());
    let tail: f32 = ar.iter().zip(br).map(|(x, y)| x * y).sum();
    for (x, y) in ac.iter().zip(bc) {
        for i in 0..L {
            acc[i] += x[i] * y[i];
        }
    }
    acc.iter().sum::<f32>() + tail
}

impl Module for QLinear {
    /// x: (..., in) -> (..., out).
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let dims = x.dims().to_vec();
        let k = *dims.last().unwrap_or(&0);
        if k != self.in_dim {
            candle_core::bail!("qlinear: input dim {k} vs weight {}", self.in_dim);
        }
        let m = x.elem_count() / k.max(1);
        let mut out_shape = dims.clone();
        if let Some(last) = out_shape.last_mut() {
            *last = self.out_dim;
        }
        if m > GEMV_MAX_ROWS {
            let w = self.dequantize()?;
            let y = x.reshape((m, k))?.matmul(&w.t()?)?;
            let y = match &self.bias {
                Some(b) => y.broadcast_add(&Tensor::new(b.as_slice(), &self.dev)?)?,
                None => y,
            };
            return y.reshape(out_shape);
        }
        let xv = x.flatten_all()?.to_vec1::<f32>()?;
        let y = self.gemv(&xv, m);
        Tensor::from_vec(y, out_shape, &self.dev)
    }
}

/// A T3 projection: the checkpoint's f32 layer or its int8 copy.
pub enum Proj {
    F32(Linear),
    Q8(QLinear),
}

impl Proj {
    pub fn new(ls: &[&Linear], w: T3Weights) -> Result<Self> {
        match w {
            T3Weights::Q8 => Ok(Proj::Q8(QLinear::stacked(ls)?)),
            T3Weights::F32 if ls.len() == 1 => Ok(Proj::F32(ls[0].clone())),
            T3Weights::F32 => {
                let ws: Vec<&Tensor> = ls.iter().map(|l| l.weight()).collect();
                let bias = if ls.iter().all(|l| l.bias().is_some()) {
                    let bs: Vec<&Tensor> = ls.iter().filter_map(|l| l.bias()).collect();
                    Some(Tensor::cat(&bs, 0)?)
                } else {
                    None
                };
                Ok(Proj::F32(Linear::new(Tensor::cat(&ws, 0)?, bias)))
            }
        }
    }
    pub fn bytes(&self) -> usize {
        match self {
            Proj::F32(l) => 4 * l.weight().elem_count(),
            Proj::Q8(q) => q.bytes(),
        }
    }
}

impl Module for Proj {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        match self {
            Proj::F32(l) => l.forward(x),
            Proj::Q8(q) => q.forward(x),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max_rel(a: &Tensor, b: &Tensor) -> f32 {
        let d = (a - b).unwrap().abs().unwrap().flatten_all().unwrap();
        let d = d.max(0).unwrap().to_scalar::<f32>().unwrap();
        let s = b.abs().unwrap().flatten_all().unwrap();
        d / s.max(0).unwrap().to_scalar::<f32>().unwrap()
    }

    #[test]
    fn q8_matches_f32_linear() {
        let dev = Device::Cpu;
        let w = Tensor::randn(0f32, 0.05, (200, 128), &dev).unwrap();
        let b = Tensor::randn(0f32, 0.1, 200, &dev).unwrap();
        let lin = Linear::new(w.clone(), Some(b.clone()));
        let q = QLinear::from_linear(&lin).unwrap();
        assert_eq!(q.bytes(), 200 * 128 + 4 * 200 * 4);
        // Dequantised weights are within half a step of the originals.
        let dw = (q.dequantize().unwrap() - &w).unwrap().abs().unwrap();
        let worst = dw
            .flatten_all()
            .unwrap()
            .max(0)
            .unwrap()
            .to_scalar::<f32>()
            .unwrap();
        let amax = w.abs().unwrap().flatten_all().unwrap().max(0).unwrap();
        assert!(worst <= amax.to_scalar::<f32>().unwrap() / 127.0 * 0.5 + 1e-7);
        // gemv path (2 rows) and matmul path (9 rows) agree with f32.
        for rows in [1usize, 2, 9] {
            let x = Tensor::randn(0f32, 1.0, (rows, 1, 128), &dev).unwrap();
            let want = lin.forward(&x).unwrap();
            let got = q.forward(&x).unwrap();
            assert_eq!(got.dims(), want.dims());
            assert!(max_rel(&got, &want) < 0.02, "rows {rows}");
        }
    }

    #[test]
    fn stacked_equals_concatenated_outputs() {
        let dev = Device::Cpu;
        let a = Linear::new(Tensor::randn(0f32, 0.05, (64, 96), &dev).unwrap(), None);
        let b = Linear::new(Tensor::randn(0f32, 0.05, (32, 96), &dev).unwrap(), None);
        let x = Tensor::randn(0f32, 1.0, (2, 1, 96), &dev).unwrap();
        let want = Tensor::cat(&[a.forward(&x).unwrap(), b.forward(&x).unwrap()], 2).unwrap();
        for w in [T3Weights::F32, T3Weights::Q8] {
            let p = Proj::new(&[&a, &b], w).unwrap();
            let got = p.forward(&x).unwrap();
            assert_eq!(got.dims(), &[2, 1, 96]);
            assert!(max_rel(&got, &want) < 0.02, "{w:?}");
        }
        assert_eq!(T3Weights::parse("Q8").unwrap(), T3Weights::Q8);
        assert_eq!(T3Weights::parse("f32").unwrap(), T3Weights::F32);
        assert!(T3Weights::parse("q4").is_err());
    }
}
