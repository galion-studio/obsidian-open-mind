// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! HiFT vocoder (HiFTNet: neural source filter + iSTFTNet) as used by S3Gen:
//! mel (80, 50 Hz) -> F0 (conv predictor) -> harmonic sine source at 24 kHz
//! -> source STFT fused into a HiFi-GAN upsampler (x8, x5, x3) with Snake
//! residual blocks -> 18 channels = magnitude/phase of a 16-point STFT with
//! hop 4 -> inverse STFT -> waveform clamped to +-0.99.

use candle_core::{Device, Module, Result, Tensor};
use candle_nn::{Linear, VarBuilder};

use crate::dsp::{Fft, C};
use crate::nnx::{leaky_relu, linear, snake, Conv1d, ConvT1d};
use crate::rng::Rng;

const N_FFT: usize = 16;
const HOP: usize = 4;
const UP_RATES: [usize; 3] = [8, 5, 3];
const UP_KERNELS: [usize; 3] = [16, 11, 7];
const SR: f64 = 24_000.0;

struct ResBlock {
    c1: Vec<Conv1d>,
    c2: Vec<Conv1d>,
    a1: Vec<Tensor>,
    a2: Vec<Tensor>,
}

impl ResBlock {
    fn load(vb: &VarBuilder, k: usize, dil: [usize; 3]) -> Result<Self> {
        let mut r = ResBlock {
            c1: vec![],
            c2: vec![],
            a1: vec![],
            a2: vec![],
        };
        for (i, d) in dil.iter().enumerate() {
            let p1 = (k * d - d) / 2;
            let p2 = (k - 1) / 2;
            r.c1.push(Conv1d::load_wn(
                &vb.pp(format!("convs1.{i}")),
                1,
                *d,
                p1,
                p1,
            )?);
            r.c2.push(Conv1d::load_wn(
                &vb.pp(format!("convs2.{i}")),
                1,
                1,
                p2,
                p2,
            )?);
            r.a1.push(vb.pp(format!("activations1.{i}")).get_unchecked("alpha")?);
            r.a2.push(vb.pp(format!("activations2.{i}")).get_unchecked("alpha")?);
        }
        Ok(r)
    }
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mut x = x.clone();
        for i in 0..self.c1.len() {
            let p = crate::prof::span("hift.snake");
            let s1 = snake(&x, &self.a1[i])?;
            drop(p);
            let p = crate::prof::span("hift.resconv");
            let h = self.c1[i].forward(&s1)?;
            drop(p);
            let p = crate::prof::span("hift.snake");
            let s2 = snake(&h, &self.a2[i])?;
            drop(p);
            let _p = crate::prof::span("hift.resconv");
            let h = self.c2[i].forward(&s2)?;
            x = (x + h)?;
        }
        Ok(x)
    }
}

pub struct HiFT {
    f0_convs: Vec<Conv1d>,
    f0_cls: Linear,
    src_linear: Linear,
    conv_pre: Conv1d,
    ups: Vec<ConvT1d>,
    src_downs: Vec<Conv1d>,
    src_res: Vec<ResBlock>,
    res: Vec<ResBlock>,
    conv_post: Conv1d,
    dev: Device,
}

impl HiFT {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let dev = vb.device().clone();
        let f = vb.pp("f0_predictor");
        let f0_convs = [0, 2, 4, 6, 8]
            .iter()
            .map(|i| Conv1d::load_wn(&f.pp(format!("condnet.{i}")), 1, 1, 1, 1))
            .collect::<Result<_>>()?;
        let ups = (0..3)
            .map(|i| {
                ConvT1d::load_wn(
                    &vb.pp(format!("ups.{i}")),
                    UP_RATES[i],
                    (UP_KERNELS[i] - UP_RATES[i]) / 2,
                )
            })
            .collect::<Result<_>>()?;
        // Source downsampling: cumulative rates (15, 3, 1).
        let src_downs = vec![
            Conv1d::load(&vb.pp("source_downs.0"), true, 15, 1, 7, 7)?,
            Conv1d::load(&vb.pp("source_downs.1"), true, 3, 1, 1, 1)?,
            Conv1d::load(&vb.pp("source_downs.2"), true, 1, 1, 0, 0)?,
        ];
        let src_res = [(7usize, 0usize), (7, 1), (11, 2)]
            .iter()
            .map(|&(k, i)| ResBlock::load(&vb.pp(format!("source_resblocks.{i}")), k, [1, 3, 5]))
            .collect::<Result<_>>()?;
        let mut res = Vec::new();
        for i in 0..3 {
            for (j, k) in [3usize, 7, 11].iter().enumerate() {
                res.push(ResBlock::load(
                    &vb.pp(format!("resblocks.{}", i * 3 + j)),
                    *k,
                    [1, 3, 5],
                )?);
            }
        }
        Ok(HiFT {
            f0_convs,
            f0_cls: linear(&f.pp("classifier"), 512, 1, true)?,
            src_linear: linear(&vb.pp("m_source").pp("l_linear"), 9, 1, true)?,
            conv_pre: Conv1d::load_wn(&vb.pp("conv_pre"), 1, 1, 3, 3)?,
            ups,
            src_downs,
            src_res,
            res,
            conv_post: Conv1d::load_wn(&vb.pp("conv_post"), 1, 1, 3, 3)?,
            dev,
        })
    }

    /// F0 in Hz per mel frame. mel: (1, 80, T)
    fn f0(&self, mel: &Tensor) -> Result<Vec<f32>> {
        let mut x = mel.clone();
        for c in &self.f0_convs {
            x = c.forward(&x)?.elu(1.0)?;
        }
        let y = self.f0_cls.forward(&x.transpose(1, 2)?)?.abs()?;
        y.flatten_all()?.to_vec1::<f32>()
    }

    /// Harmonic-plus-noise source at 24 kHz (SineGen + linear + tanh).
    fn source(&self, f0: &[f32], rng: &mut Rng) -> Result<Vec<f32>> {
        let up = UP_RATES.iter().product::<usize>() * HOP; // 480
        let n = f0.len() * up;
        let harm = 9;
        let w = self.src_linear.weight().flatten_all()?.to_vec1::<f32>()?;
        let b = self
            .src_linear
            .bias()
            .map(|b| b.to_vec1::<f32>())
            .transpose()?
            .map_or(0.0, |v| v[0]);
        let (sine_amp, noise_std) = (0.1f32, 0.003f32);
        // Random initial phase per harmonic except the fundamental.
        let phase: Vec<f64> = (0..harm)
            .map(|h| {
                if h == 0 {
                    0.0
                } else {
                    (rng.uniform() * 2.0 - 1.0) * std::f64::consts::PI
                }
            })
            .collect();
        let mut cum = vec![0f64; harm];
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let f = f0[i / up] as f64;
            let uv = if f > 10.0 { 1.0f32 } else { 0.0 };
            let mut acc = b;
            for h in 0..harm {
                cum[h] = (cum[h] + f * (h + 1) as f64 / SR).fract();
                let s = sine_amp * (2.0 * std::f64::consts::PI * cum[h] + phase[h]).sin() as f32;
                let namp = uv * noise_std + (1.0 - uv) * sine_amp / 3.0;
                let v = s * uv + namp * rng.normal();
                acc += w[h] * v;
            }
            out.push(acc.tanh());
        }
        Ok(out)
    }

    /// Centered STFT (reflect pad 8) of the source -> (1, 18, frames).
    fn source_stft(&self, s: &[f32]) -> Result<Tensor> {
        let win = crate::dsp::hann(N_FFT);
        let padded = crate::dsp::reflect_pad(s, N_FFT / 2, N_FFT / 2);
        let frames = 1 + (padded.len() - N_FFT) / HOP;
        let fft = Fft::new(N_FFT);
        let nb = N_FFT / 2 + 1;
        let mut re = vec![0f32; nb * frames];
        let mut im = vec![0f32; nb * frames];
        let mut buf = vec![C::default(); N_FFT];
        for f in 0..frames {
            for i in 0..N_FFT {
                buf[i] = C {
                    re: (padded[f * HOP + i] * win[i]) as f64,
                    im: 0.0,
                };
            }
            let spec = fft.forward(&buf);
            for k in 0..nb {
                re[k * frames + f] = spec[k].re as f32;
                im[k * frames + f] = spec[k].im as f32;
            }
        }
        re.extend_from_slice(&im);
        Tensor::from_vec(re, (1, 2 * nb, frames), &self.dev)
    }

    /// Inverse STFT (torch.istft, center=True, Hann, overlap-add with window
    /// envelope normalisation).
    fn istft(mag: &[f32], phase: &[f32], frames: usize) -> Vec<f32> {
        let nb = N_FFT / 2 + 1;
        let win = crate::dsp::hann(N_FFT);
        let fft = Fft::new(N_FFT);
        let len = N_FFT + HOP * (frames - 1);
        let mut y = vec![0f32; len];
        let mut env = vec![0f32; len];
        let mut spec = vec![C::default(); N_FFT];
        for f in 0..frames {
            for k in 0..nb {
                let m = mag[k * frames + f].min(100.0) as f64;
                let p = phase[k * frames + f] as f64;
                spec[k] = C {
                    re: m * p.cos(),
                    im: m * p.sin(),
                };
            }
            // Hermitian completion; inverse via conjugate trick.
            for k in nb..N_FFT {
                let c = spec[N_FFT - k];
                spec[k] = C {
                    re: c.re,
                    im: -c.im,
                };
            }
            let conj: Vec<C> = spec
                .iter()
                .map(|c| C {
                    re: c.re,
                    im: -c.im,
                })
                .collect();
            let t = fft.forward(&conj);
            for i in 0..N_FFT {
                let v = (t[i].re / N_FFT as f64) as f32;
                y[f * HOP + i] += v * win[i];
                env[f * HOP + i] += win[i] * win[i];
            }
        }
        let start = N_FFT / 2;
        let end = len - N_FFT / 2;
        (start..end)
            .map(|i| if env[i] > 1e-11 { y[i] / env[i] } else { y[i] })
            .collect()
    }

    /// mel (1, 80, T) -> 24 kHz samples (T * 480).
    pub fn inference(&self, mel: &Tensor, rng: &mut Rng) -> Result<Vec<f32>> {
        let p = crate::prof::span("hift.f0_source_stft");
        let f0 = self.f0(mel)?;
        let s = self.source(&f0, rng)?;
        let s_stft = self.source_stft(&s)?;
        drop(p);
        let mut x = self.conv_pre.forward(mel)?;
        for i in 0..3 {
            x = leaky_relu(&x, 0.1)?;
            let p = crate::prof::span("hift.ups");
            x = self.ups[i].forward(&x)?;
            drop(p);
            if i == 2 {
                // ReflectionPad1d((1, 0)).
                let t = x.dim(2)?;
                let first = x.narrow(2, 1, 1)?;
                x = Tensor::cat(&[&first, &x.narrow(2, 0, t)?], 2)?;
            }
            let si = self.src_res[i].forward(&self.src_downs[i].forward(&s_stft)?)?;
            let t = x.dim(2)?.min(si.dim(2)?);
            x = (x.narrow(2, 0, t)? + si.narrow(2, 0, t)?)?;
            let mut xs = self.res[i * 3].forward(&x)?;
            for j in 1..3 {
                xs = (xs + self.res[i * 3 + j].forward(&x)?)?;
            }
            x = (xs / 3.0)?;
        }
        let x = leaky_relu(&x, 0.01)?;
        let x = self.conv_post.forward(&x)?; // (1, 18, F)
        let frames = x.dim(2)?;
        let nb = N_FFT / 2 + 1;
        let mag = x.narrow(1, 0, nb)?.exp()?.flatten_all()?.to_vec1::<f32>()?;
        let phase = x
            .narrow(1, nb, nb)?
            .sin()?
            .flatten_all()?
            .to_vec1::<f32>()?;
        let _p = crate::prof::span("hift.istft");
        let y = Self::istft(&mag, &phase, frames);
        Ok(y.into_iter().map(|v| v.clamp(-0.99, 0.99)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn istft_inverts_stft() {
        // Build an STFT of a tone with the same framing and invert it.
        let n = 400;
        let s: Vec<f32> = (0..n).map(|i| (i as f32 * 0.3).sin() * 0.5).collect();
        let win = crate::dsp::hann(N_FFT);
        let padded = crate::dsp::reflect_pad(&s, 8, 8);
        let frames = 1 + (padded.len() - N_FFT) / HOP;
        let fft = Fft::new(N_FFT);
        let nb = 9;
        let (mut mag, mut ph) = (vec![0f32; nb * frames], vec![0f32; nb * frames]);
        for f in 0..frames {
            let buf: Vec<C> = (0..N_FFT)
                .map(|i| C {
                    re: (padded[f * HOP + i] * win[i]) as f64,
                    im: 0.0,
                })
                .collect();
            let sp = fft.forward(&buf);
            for k in 0..nb {
                mag[k * frames + f] = sp[k].norm_sqr().sqrt() as f32;
                ph[k * frames + f] = sp[k].im.atan2(sp[k].re) as f32;
            }
        }
        let y = HiFT::istft(&mag, &ph, frames);
        assert_eq!(y.len(), n);
        for i in 10..n - 10 {
            assert!((y[i] - s[i]).abs() < 1e-3, "{i}: {} {}", y[i], s[i]);
        }
    }
}
