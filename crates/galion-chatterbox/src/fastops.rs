// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Multi-threaded CPU kernels for the element-wise hot spots of the mouth
//! (voice-speed). candle's CPU unary ops run on one thread and call libm per
//! element; on the flow decoder's shapes `gelu_erf` alone took 25 % of a step.
//! These kernels are plain portable Rust (no intrinsics, no asm, no
//! target-cpu flags): branch-free polynomial `exp`/`erf` that the compiler can
//! auto-vectorise for the baseline target, split across threads with rayon.
//!
//! Accuracy (checked by the unit tests against `f64` libm):
//! `exp` relative error < 2e-7 on [-87, 88]; `erf` absolute error < 5e-7 (1.5e-7 formula + f32 rounding);
//! gelu/mish/silu error < 1e-6 * max(1, |x|) for |x| <= 10.

use candle_core::{CpuStorage, CustomOp1, Layout, Result, Shape, Tensor};
use rayon::prelude::*;

const CHUNK: usize = 16 * 1024;

/// e^x in f32 (Cephes `expf` reduction + degree-6 polynomial), branch-free.
#[inline(always)]
pub fn exp_f(x: f32) -> f32 {
    let x = x.clamp(-87.33654, 88.72283);
    // n = round(x / ln 2) via the 1.5 * 2^23 trick (no libm floor/round call).
    const MAGIC: f32 = 12_582_912.0;
    let n = (x * std::f32::consts::LOG2_E + MAGIC) - MAGIC;
    let r = x - n * 0.693_359_4 + n * 2.121_944_4e-4;
    let p = 1.987_569_1e-4_f32;
    let p = p * r + 1.398_199_9e-3;
    let p = p * r + 8.333_452e-3;
    let p = p * r + 4.166_579_6e-2;
    let p = p * r + 1.666_666_5e-1;
    let p = p * r + 5e-1;
    let y = p * r * r + r + 1.0;
    // 2^n: n in [-126, 128) after the clamp; n = 128 only at the clamp edge.
    let ni = (n as i32).min(127);
    let scale = f32::from_bits(((ni + 127) as u32) << 23);
    y * scale
}

/// erf(x), Abramowitz & Stegun 7.1.26 (|error| <= 1.5e-7), branch-free.
#[inline(always)]
pub fn erf_f(x: f32) -> f32 {
    let a = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * a);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_8 + t * (-1.453_152_1 + t * 1.061_405_4))));
    let y = 1.0 - poly * exp_f(-a * a);
    y.copysign(x)
}

#[inline(always)]
pub fn gelu_erf_f(x: f32) -> f32 {
    0.5 * x * (1.0 + erf_f(x * std::f32::consts::FRAC_1_SQRT_2))
}

/// x * tanh(softplus(x)) = x * n / (n + 2) with n = e^x (e^x + 2).
#[inline(always)]
pub fn mish_f(x: f32) -> f32 {
    let e = exp_f(x.min(20.0));
    let n = e * (e + 2.0);
    x * n / (n + 2.0)
}

#[inline(always)]
pub fn silu_f(x: f32) -> f32 {
    x / (1.0 + exp_f(-x))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unary {
    GeluErf,
    Mish,
    Silu,
}

impl Unary {
    fn apply(self, src: &[f32], dst: &mut [f32]) {
        match self {
            Unary::GeluErf => src.iter().zip(dst).for_each(|(s, d)| *d = gelu_erf_f(*s)),
            Unary::Mish => src.iter().zip(dst).for_each(|(s, d)| *d = mish_f(*s)),
            Unary::Silu => src.iter().zip(dst).for_each(|(s, d)| *d = silu_f(*s)),
        }
    }
}

fn f32_slice<'a>(s: &'a CpuStorage, l: &Layout, op: &str) -> Result<&'a [f32]> {
    let CpuStorage::F32(v) = s else {
        candle_core::bail!("{op}: f32 only")
    };
    match l.contiguous_offsets() {
        Some((a, b)) => Ok(&v[a..b]),
        None => candle_core::bail!("{op}: input must be contiguous"),
    }
}

impl CustomOp1 for Unary {
    fn name(&self) -> &'static str {
        match self {
            Unary::GeluErf => "fast-gelu-erf",
            Unary::Mish => "fast-mish",
            Unary::Silu => "fast-silu",
        }
    }
    fn cpu_fwd(&self, s: &CpuStorage, l: &Layout) -> Result<(CpuStorage, Shape)> {
        let src = f32_slice(s, l, self.name())?;
        let mut dst = vec![0f32; src.len()];
        src.par_chunks(CHUNK)
            .zip(dst.par_chunks_mut(CHUNK))
            .for_each(|(s, d)| self.apply(s, d));
        Ok((CpuStorage::F32(dst), l.shape().clone()))
    }
}

/// Apply a fast element-wise op (f32, any shape; copies a strided input).
pub fn unary(x: &Tensor, op: Unary) -> Result<Tensor> {
    x.contiguous()?.apply_op1_no_bwd(&op)
}

pub fn gelu_erf(x: &Tensor) -> Result<Tensor> {
    unary(x, Unary::GeluErf)
}
pub fn mish(x: &Tensor) -> Result<Tensor> {
    unary(x, Unary::Mish)
}
pub fn silu(x: &Tensor) -> Result<Tensor> {
    unary(x, Unary::Silu)
}

/// softmax(scale * x) over the last dim, rows in parallel.
pub struct SoftmaxScaled(pub f32);

impl CustomOp1 for SoftmaxScaled {
    fn name(&self) -> &'static str {
        "fast-softmax-scaled"
    }
    fn cpu_fwd(&self, s: &CpuStorage, l: &Layout) -> Result<(CpuStorage, Shape)> {
        let src = f32_slice(s, l, self.name())?;
        let n = *l.shape().dims().last().unwrap_or(&1);
        let mut dst = vec![0f32; src.len()];
        if n == 0 {
            return Ok((CpuStorage::F32(dst), l.shape().clone()));
        }
        let scale = self.0;
        let rows_per = (CHUNK / n).max(1) * n;
        src.par_chunks(rows_per)
            .zip(dst.par_chunks_mut(rows_per))
            .for_each(|(s, d)| {
                for (s, d) in s.chunks_exact(n).zip(d.chunks_exact_mut(n)) {
                    softmax_row(s, d, scale);
                }
            });
        Ok((CpuStorage::F32(dst), l.shape().clone()))
    }
}

/// One row: 8 independent lanes for the max and the sum so the compiler can
/// vectorise both reductions (a single running f32 sum would stay scalar).
fn softmax_row(s: &[f32], d: &mut [f32], scale: f32) {
    d.copy_from_slice(s);
    softmax_inplace(d, scale);
}

fn softmax_inplace(d: &mut [f32], scale: f32) {
    const L: usize = 8;
    let mut mx = [f32::NEG_INFINITY; L];
    let (heads, rest) = d.as_chunks::<L>();
    for c in heads {
        for i in 0..L {
            mx[i] = if c[i] > mx[i] { c[i] } else { mx[i] };
        }
    }
    let mut m = rest.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    for v in mx {
        m = m.max(v);
    }
    let m = m * scale;
    let mut acc = [0f32; L];
    let (heads, rest) = d.as_chunks_mut::<L>();
    for o in heads {
        for i in 0..L {
            o[i] = exp_f(o[i] * scale - m);
            acc[i] += o[i];
        }
    }
    let mut sum: f32 = acc.iter().sum();
    for y in rest {
        *y = exp_f(*y * scale - m);
        sum += *y;
    }
    let inv = 1.0 / sum;
    d.iter_mut().for_each(|y| *y *= inv);
}

/// softmax over the last dim of `scale * x` (scale must be > 0).
pub fn softmax_scaled(x: &Tensor, scale: f32) -> Result<Tensor> {
    x.contiguous()?.apply_op1_no_bwd(&SoftmaxScaled(scale))
}

/// Rows of queries per attention task (the score block stays in L2).
const ATT_ROWS: usize = 64;

struct SendPtr(*mut f32);
// SAFETY: only used to hand disjoint output regions to rayon tasks (see
// `attention_qkv`); the pointee outlives the parallel loop.
#[allow(unsafe_code)]
unsafe impl Send for SendPtr {}
#[allow(unsafe_code)]
unsafe impl Sync for SendPtr {}

/// Full (non-causal) multi-head attention straight from a fused projection.
///
/// `qkv`: (B, T, 3 * H * D) with q, k, v side by side (heads inside each).
/// Returns (B, T, H * D) with heads merged. Equivalent to splitting heads,
/// `softmax(scale * q k^T) v` and merging heads, but each task handles one
/// (batch, head, 64-query block): its score block (64 x T) is produced by a
/// single-threaded gemm, normalised in cache and multiplied with v directly
/// into the output, so neither the (B, H, T, T) score tensor nor any head
/// transpose is ever materialised.
pub fn attention_qkv(qkv: &Tensor, heads: usize, head_dim: usize, scale: f32) -> Result<Tensor> {
    let (b, t, c3) = qkv.dims3()?;
    let hd = heads * head_dim;
    if c3 != 3 * hd {
        candle_core::bail!("attention_qkv: last dim {c3} != 3 * {heads} * {head_dim}");
    }
    qkv.contiguous()?.apply_op1_no_bwd(&AttentionQkv {
        heads,
        head_dim,
        scale,
        b,
        t,
    })
}

struct AttentionQkv {
    heads: usize,
    head_dim: usize,
    scale: f32,
    b: usize,
    t: usize,
}

impl CustomOp1 for AttentionQkv {
    fn name(&self) -> &'static str {
        "fast-attention-qkv"
    }
    fn cpu_fwd(&self, st: &CpuStorage, l: &Layout) -> Result<(CpuStorage, Shape)> {
        let src = f32_slice(st, l, self.name())?;
        let (b, t, heads, head_dim) = (self.b, self.t, self.heads, self.head_dim);
        let (hd, c3, scale) = (heads * head_dim, 3 * heads * head_dim, self.scale);
        Ok((
            CpuStorage::F32(attention_slice(src, b, t, heads, head_dim, hd, c3, scale)),
            Shape::from_dims(&[b, t, hd]),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn attention_slice(
    src: &[f32],
    b: usize,
    t: usize,
    heads: usize,
    head_dim: usize,
    hd: usize,
    c3: usize,
    scale: f32,
) -> Vec<f32> {
    let mut out = vec![0f32; b * t * hd];
    let blocks = t.div_ceil(ATT_ROWS);
    let optr = SendPtr(out.as_mut_ptr());
    let optr = &optr;
    (0..b * heads * blocks).into_par_iter().for_each(|task| {
        let (bh, blk) = (task / blocks, task % blocks);
        let (bi, h) = (bh / heads, bh % heads);
        let t0 = blk * ATT_ROWS;
        let rows = ATT_ROWS.min(t - t0);
        let base = bi * t * c3;
        let q = base + t0 * c3 + h * head_dim;
        let k = base + hd + h * head_dim;
        let v = base + 2 * hd + h * head_dim;
        let mut s = vec![0f32; rows * t];
        let o = (bi * t + t0) * hd + h * head_dim;
        // SAFETY: every pointer stays inside its buffer: q/k/v address
        // `rows` (resp. `t`) rows of stride c3 and width head_dim starting at
        // in-bounds offsets of `src` (b * t * c3 values); `s` is rows x t;
        // the output block (rows x head_dim at row stride hd) is written by
        // this task only, because (bi, h, blk) is unique per task.
        #[allow(unsafe_code)]
        unsafe {
            let sp = src.as_ptr();
            // s (rows x t) = q (rows x d) * k^T (d x t)
            gemm::gemm(
                rows,
                t,
                head_dim,
                s.as_mut_ptr(),
                1,
                t as isize,
                false,
                sp.add(q),
                1,
                c3 as isize,
                sp.add(k),
                c3 as isize,
                1,
                0.0,
                1.0,
                false,
                false,
                false,
                gemm::Parallelism::None,
            );
            for r in s.chunks_exact_mut(t) {
                softmax_inplace(r, scale);
            }
            // out (rows x d) = s (rows x t) * v (t x d)
            gemm::gemm(
                rows,
                head_dim,
                t,
                optr.0.add(o),
                1,
                hd as isize,
                false,
                s.as_ptr(),
                1,
                t as isize,
                sp.add(v),
                1,
                c3 as isize,
                0.0,
                1.0,
                false,
                false,
                false,
                gemm::Parallelism::None,
            );
        }
    });
    out
}

/// 1-D convolution (stride 1) as one gemm per kernel tap.
///
/// x: (B, Cin, T), w: (Cout, Cin, K), zero padding `pad_l` / `pad_r`,
/// dilation `dil`; returns (B, Cout, T + pad_l + pad_r - dil * (K - 1)).
/// `y = sum_k W[:, :, k] * X_pad[:, k * dil ..]`: the shifted input is a
/// strided view of the padded input, so unlike candle's im2col path no
/// (T, Cin * K) matrix is written (43 MB per call in HiFT's last stage).
pub fn conv1d(
    x: &Tensor,
    w: &Tensor,
    bias: Option<&Tensor>,
    dil: usize,
    pad_l: usize,
    pad_r: usize,
) -> Result<Tensor> {
    let (b, cin, t) = x.dims3()?;
    let (cout, cin_w, k) = w.dims3()?;
    if cin != cin_w {
        candle_core::bail!("conv1d: input has {cin} channels, weight {cin_w}");
    }
    let tp = t + pad_l + pad_r;
    let span = dil * (k - 1) + 1;
    if tp < span {
        candle_core::bail!("conv1d: input too short ({tp} < {span})");
    }
    let lout = tp - span + 1;
    let xv = x.contiguous()?.flatten_all()?.to_vec1::<f32>()?;
    let wv = w.contiguous()?.flatten_all()?.to_vec1::<f32>()?;
    let bv: Option<Vec<f32>> = match bias {
        Some(b) => Some(b.to_vec1()?),
        None => None,
    };
    // Padded input, one (Cin, Tp) block per batch.
    let mut xp = vec![0f32; b * cin * tp];
    xp.par_chunks_mut(tp).enumerate().for_each(|(row, d)| {
        d[pad_l..pad_l + t].copy_from_slice(&xv[row * t..(row + 1) * t]);
    });
    let mut y = vec![0f32; b * cout * lout];
    if let Some(bv) = &bv {
        y.par_chunks_mut(lout).enumerate().for_each(|(row, d)| {
            d.fill(bv[row % cout]);
        });
    }
    let threads = rayon::current_num_threads();
    for bi in 0..b {
        for tap in 0..k {
            let read = tap > 0 || bv.is_some();
            // SAFETY: dst is the (Cout, Lout) block of batch bi inside `y`;
            // lhs element (o, i) = wv[o * Cin * K + i * K + tap] < Cout*Cin*K;
            // rhs element (i, l) = xp[bi * Cin * Tp + i * Tp + tap * dil + l]
            // with l < Lout, so tap * dil + l <= dil * (K - 1) + Lout - 1 < Tp.
            #[allow(unsafe_code)]
            unsafe {
                gemm::gemm(
                    cout,
                    lout,
                    cin,
                    y.as_mut_ptr().add(bi * cout * lout),
                    1,
                    lout as isize,
                    read,
                    wv.as_ptr().add(tap),
                    k as isize,
                    (cin * k) as isize,
                    xp.as_ptr().add(bi * cin * tp + tap * dil),
                    1,
                    tp as isize,
                    1.0,
                    1.0,
                    false,
                    false,
                    false,
                    gemm::Parallelism::Rayon(threads),
                );
            }
        }
    }
    Tensor::from_vec(y, (b, cout, lout), x.device())
}

/// Layer norm over the channel dim of a (B, C, T) tensor, written back as
/// (B, C, T): the same as `ln(x.transpose(1, 2)).transpose(1, 2)` without the
/// two strided copies.
pub struct ChannelLayerNorm {
    pub w: Vec<f32>,
    pub b: Vec<f32>,
    pub eps: f32,
}

impl ChannelLayerNorm {
    pub fn from_tensors(w: &Tensor, b: &Tensor, eps: f64) -> Result<Self> {
        Ok(ChannelLayerNorm {
            w: w.to_vec1()?,
            b: b.to_vec1()?,
            eps: eps as f32,
        })
    }
    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        x.contiguous()?.apply_op1_no_bwd(self)
    }
}

impl CustomOp1 for ChannelLayerNorm {
    fn name(&self) -> &'static str {
        "fast-channel-layer-norm"
    }
    fn cpu_fwd(&self, s: &CpuStorage, l: &Layout) -> Result<(CpuStorage, Shape)> {
        let src = f32_slice(s, l, self.name())?;
        let (b, c, t) = l.shape().dims3()?;
        if c != self.w.len() || c != self.b.len() {
            candle_core::bail!(
                "channel layer norm: {c} channels vs {} weights",
                self.w.len()
            );
        }
        // Pass 1: mean and 1/std per (batch, frame); frames in parallel blocks.
        const TB: usize = 256;
        let blocks: Vec<(usize, usize)> = (0..b)
            .flat_map(|bi| (0..t).step_by(TB).map(move |t0| (bi, t0)))
            .collect();
        let stats: Vec<(usize, usize, Vec<f32>, Vec<f32>)> = blocks
            .par_iter()
            .map(|&(bi, t0)| {
                let len = TB.min(t - t0);
                let mut mean = vec![0f32; len];
                for ci in 0..c {
                    let row = &src[(bi * c + ci) * t + t0..][..len];
                    mean.iter_mut().zip(row).for_each(|(m, v)| *m += v);
                }
                mean.iter_mut().for_each(|m| *m /= c as f32);
                let mut var = vec![0f32; len];
                for ci in 0..c {
                    let row = &src[(bi * c + ci) * t + t0..][..len];
                    var.iter_mut()
                        .zip(row)
                        .zip(&mean)
                        .for_each(|((s, v), m)| *s += (v - m) * (v - m));
                }
                let rstd = var
                    .iter()
                    .map(|v| 1.0 / (v / c as f32 + self.eps).sqrt())
                    .collect();
                (bi, t0, mean, rstd)
            })
            .collect();
        let mut mean = vec![0f32; b * t];
        let mut rstd = vec![0f32; b * t];
        for (bi, t0, m, r) in stats {
            mean[bi * t + t0..][..m.len()].copy_from_slice(&m);
            rstd[bi * t + t0..][..r.len()].copy_from_slice(&r);
        }
        // Pass 2: normalise each (batch, channel) row.
        let mut dst = vec![0f32; src.len()];
        dst.par_chunks_mut(t.max(1))
            .enumerate()
            .for_each(|(row, d)| {
                let (bi, ci) = (row / c, row % c);
                let s = &src[row * t..][..t];
                let (m, r) = (&mean[bi * t..][..t], &rstd[bi * t..][..t]);
                let (w, bb) = (self.w[ci], self.b[ci]);
                for i in 0..t {
                    d[i] = (s[i] - m[i]) * r[i] * w + bb;
                }
            });
        Ok((CpuStorage::F32(dst), l.shape().clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Device;

    fn grid() -> Vec<f32> {
        (-2000..=2000).map(|i| i as f32 * 0.005).collect()
    }

    #[test]
    fn exp_matches_libm() {
        for i in -8700..=8800 {
            let x = i as f32 * 0.01;
            let want = (x as f64).exp();
            let got = exp_f(x) as f64;
            assert!(
                ((got - want) / want).abs() < 2e-7,
                "exp({x}) = {got} vs {want}"
            );
        }
        assert_eq!(exp_f(-1000.0), exp_f(-87.33654));
        assert!(exp_f(1000.0).is_finite());
    }

    #[test]
    fn erf_gelu_mish_silu_match_reference() {
        for x in grid() {
            let xd = x as f64;
            // erf via the f64 series of the reference: use libm-free identity
            // through gelu computed in f64 with a high-precision erf.
            let erf = erf64(xd);
            let d = (erf_f(x) as f64 - erf).abs();
            assert!(d < 5e-7, "erf({x}) off by {d}");
            let tol = 1e-6 * xd.abs().max(1.0);
            let gelu = 0.5 * xd * (1.0 + erf64(xd / std::f64::consts::SQRT_2));
            assert!((gelu_erf_f(x) as f64 - gelu).abs() < tol, "gelu({x})");
            let mish = xd * (1.0 + xd.exp()).ln().tanh();
            assert!((mish_f(x) as f64 - mish).abs() < tol, "mish({x})");
            let silu = xd / (1.0 + (-xd).exp());
            assert!((silu_f(x) as f64 - silu).abs() < tol, "silu({x})");
        }
        assert_eq!(mish_f(100.0), 100.0);
        assert!(mish_f(-100.0).abs() < 1e-30);
    }

    /// erf in f64 by its Taylor series (|x| < 3) / continued fraction tail.
    fn erf64(x: f64) -> f64 {
        let a = x.abs();
        let v = if a < 3.0 {
            let mut sum = 0.0;
            let mut term = a;
            let mut n = 0.0;
            while term.abs() > 1e-17 {
                sum += term / (2.0 * n + 1.0);
                n += 1.0;
                term *= -a * a / n;
            }
            sum * 2.0 / std::f64::consts::PI.sqrt()
        } else {
            // erfc continued fraction (Lentz), accurate far beyond f32 here.
            let mut f = a;
            for k in (1..60).rev() {
                f = a + (k as f64 / 2.0) / f;
            }
            1.0 - (-a * a).exp() / (f * std::f64::consts::PI.sqrt())
        };
        v.copysign(x)
    }

    #[test]
    fn tensor_ops_match_candle() {
        let dev = Device::Cpu;
        let x = Tensor::randn(0f32, 2.0, (2, 7, 33), &dev).unwrap();
        let close = |a: &Tensor, b: &Tensor, tol: f32| {
            let d = (a - b)
                .unwrap()
                .abs()
                .unwrap()
                .flatten_all()
                .unwrap()
                .max(0)
                .unwrap()
                .to_scalar::<f32>()
                .unwrap();
            assert!(d < tol, "max diff {d}");
        };
        close(&gelu_erf(&x).unwrap(), &x.gelu_erf().unwrap(), 1e-5);
        close(&silu(&x).unwrap(), &x.silu().unwrap(), 1e-5);
        close(
            &mish(&x).unwrap(),
            &crate::nnx::mish_reference(&x).unwrap(),
            1e-5,
        );
        close(
            &softmax_scaled(&x, 0.125).unwrap(),
            &candle_nn::ops::softmax_last_dim(&(&x * 0.125).unwrap()).unwrap(),
            1e-6,
        );
        // Strided input is accepted (copied first).
        let xt = x.transpose(1, 2).unwrap();
        close(&silu(&xt).unwrap(), &xt.silu().unwrap(), 1e-5);
        // Blocked attention == heads + sdpa + merge (T crosses the 64-row block).
        for t in [5usize, 130] {
            let qkv = Tensor::randn(0f32, 1.0, (2, t, 3 * 4 * 16), &dev).unwrap();
            let part = |i: usize| {
                crate::nnx::heads(&qkv.narrow(2, i * 64, 64).unwrap().contiguous().unwrap(), 4)
                    .unwrap()
            };
            let a = crate::nnx::sdpa(&part(0), &part(1), &part(2), 0.25, None).unwrap();
            let want = crate::nnx::merge_heads(&a).unwrap();
            close(&attention_qkv(&qkv, 4, 16, 0.25).unwrap(), &want, 1e-5);
        }
        // Tap-wise conv1d == candle conv1d (dilation, asymmetric padding, bias).
        for (k, dil, pl, pr, bias) in [
            (3usize, 1usize, 2usize, 0usize, true),
            (7, 3, 9, 9, true),
            (1, 1, 0, 0, false),
        ] {
            let x = Tensor::randn(0f32, 1.0, (2, 5, 40), &dev).unwrap();
            let w = Tensor::randn(0f32, 0.3, (6, 5, k), &dev).unwrap();
            let bb = Tensor::randn(0f32, 0.3, 6, &dev).unwrap();
            let want = x
                .pad_with_zeros(2, pl, pr)
                .unwrap()
                .conv1d(&w, 0, 1, dil, 1)
                .unwrap();
            let want = if bias {
                want.broadcast_add(&bb.reshape((1, 6, 1)).unwrap()).unwrap()
            } else {
                want
            };
            let got = conv1d(&x, &w, bias.then_some(&bb), dil, pl, pr).unwrap();
            assert_eq!(got.dims(), want.dims());
            close(&got, &want, 1e-4);
        }
        // Channel layer norm == layer norm over the transposed tensor.
        let w = Tensor::randn(1f32, 0.3, 7, &dev).unwrap();
        let b = Tensor::randn(0f32, 0.3, 7, &dev).unwrap();
        let ln = candle_nn::LayerNorm::new(w.clone(), b.clone(), 1e-5);
        let want = candle_core::Module::forward(&ln, &x.transpose(1, 2).unwrap())
            .unwrap()
            .transpose(1, 2)
            .unwrap();
        let cln = ChannelLayerNorm::from_tensors(&w, &b, 1e-5).unwrap();
        close(&cln.forward(&x).unwrap(), &want, 1e-5);
        // Long time axis crosses the 256-frame block boundary.
        let x = Tensor::randn(0f32, 1.0, (2, 7, 600), &dev).unwrap();
        let want = candle_core::Module::forward(&ln, &x.transpose(1, 2).unwrap())
            .unwrap()
            .transpose(1, 2)
            .unwrap();
        close(&cln.forward(&x).unwrap(), &want, 1e-5);
    }
}
