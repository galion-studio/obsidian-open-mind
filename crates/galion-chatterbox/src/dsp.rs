// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Signal processing written for this crate (scalar, portable): a mixed-radix
//! FFT for any length (1920 = 2^7*3*5 for the 24 kHz mel, 400 and 512 for the
//! 16 kHz front ends), STFT power spectra with reflect padding, Slaney mel
//! filters (the librosa default), Kaldi fbank (CAMPPlus front end), a
//! windowed-sinc resampler (same kernel construction as torchaudio's
//! `sinc_interp_hann`), and an energy-based silence trim.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    fn mul(self, o: C) -> C {
        C {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
    fn add(self, o: C) -> C {
        C {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
    pub fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}

/// Planned complex FFT of one length (forward, unnormalised).
pub struct Fft {
    n: usize,
    factors: Vec<usize>,
    tw: Vec<C>,
}

impl Fft {
    pub fn new(n: usize) -> Fft {
        assert!(n > 0);
        let mut factors = Vec::new();
        let mut m = n;
        for p in [4usize, 2, 3, 5] {
            while m.is_multiple_of(p) {
                factors.push(p);
                m /= p;
            }
        }
        let mut p = 7;
        while m > 1 {
            while m.is_multiple_of(p) {
                factors.push(p);
                m /= p;
            }
            p += 2;
        }
        let tw = (0..n)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / n as f64;
                C {
                    re: a.cos(),
                    im: a.sin(),
                }
            })
            .collect();
        Fft { n, factors, tw }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn forward(&self, x: &[C]) -> Vec<C> {
        assert_eq!(x.len(), self.n);
        let mut out = vec![C::default(); self.n];
        self.rec(x, 0, 1, self.n, 0, &mut out);
        out
    }

    fn rec(&self, x: &[C], off: usize, stride: usize, n: usize, fi: usize, out: &mut [C]) {
        if n == 1 {
            out[0] = x[off];
            return;
        }
        let p = self.factors[fi];
        let m = n / p;
        for r in 0..p {
            self.rec(
                x,
                off + r * stride,
                stride * p,
                m,
                fi + 1,
                &mut out[r * m..(r + 1) * m],
            );
        }
        let ts = self.n / n;
        let mut tmp = [C::default(); 16];
        let mut big = Vec::new();
        let buf: &mut [C] = if p <= 16 {
            &mut tmp[..p]
        } else {
            big.resize(p, C::default());
            &mut big
        };
        for k in 0..m {
            for (r, b) in buf.iter_mut().enumerate() {
                *b = out[r * m + k];
            }
            for q in 0..p {
                let kk = k + q * m;
                let mut acc = C::default();
                for (r, b) in buf.iter().enumerate() {
                    let e = (r * kk) % n;
                    acc = acc.add(b.mul(self.tw[e * ts]));
                }
                out[kk] = acc;
            }
        }
    }

    /// |X_k|^2 for k = 0..=n/2 of a real frame.
    pub fn power(&self, frame: &[f32]) -> Vec<f32> {
        let x: Vec<C> = frame
            .iter()
            .map(|&v| C {
                re: v as f64,
                im: 0.0,
            })
            .collect();
        self.forward(&x)[..self.n / 2 + 1]
            .iter()
            .map(|c| c.norm_sqr() as f32)
            .collect()
    }
}

/// Periodic Hann window (torch.hann_window / scipy get_window("hann")).
pub fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()) as f32)
        .collect()
}

/// Reflect padding (numpy/torch "reflect": the edge sample is not repeated).
pub fn reflect_pad(x: &[f32], left: usize, right: usize) -> Vec<f32> {
    let n = x.len();
    if n == 0 {
        return vec![0.0; left + right];
    }
    let at = |i: isize| -> f32 {
        if n == 1 {
            return x[0];
        }
        let period = 2 * (n as isize - 1);
        let mut j = i.rem_euclid(period);
        if j >= n as isize {
            j = period - j;
        }
        x[j as usize]
    };
    (-(left as isize)..(n + right) as isize).map(at).collect()
}

/// STFT power (or magnitude when `power == false`) frames, each n_fft/2+1
/// bins. `center` pads n_fft/2 on both sides with reflection (torch/librosa).
pub fn stft(
    x: &[f32],
    n_fft: usize,
    hop: usize,
    window: &[f32],
    center: bool,
    power: bool,
) -> Vec<Vec<f32>> {
    let padded;
    let sig: &[f32] = if center {
        padded = reflect_pad(x, n_fft / 2, n_fft / 2);
        &padded
    } else {
        x
    };
    if sig.len() < n_fft {
        return Vec::new();
    }
    let frames = 1 + (sig.len() - n_fft) / hop;
    let fft = Fft::new(n_fft);
    let mut wpad = vec![0f32; n_fft];
    let off = (n_fft - window.len()) / 2;
    wpad[off..off + window.len()].copy_from_slice(window);
    let mut buf = vec![0f32; n_fft];
    (0..frames)
        .map(|f| {
            let s = &sig[f * hop..f * hop + n_fft];
            for i in 0..n_fft {
                buf[i] = s[i] * wpad[i];
            }
            let p = fft.power(&buf);
            if power {
                p
            } else {
                p.into_iter().map(f32::sqrt).collect()
            }
        })
        .collect()
}

fn hz_to_mel_slaney(f: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = (6.4f64).ln() / 27.0;
    if f >= min_log_hz {
        min_log_mel + (f / min_log_hz).ln() / logstep
    } else {
        f / f_sp
    }
}

fn mel_to_hz_slaney(m: f64) -> f64 {
    let f_sp = 200.0 / 3.0;
    let min_log_hz = 1000.0;
    let min_log_mel = min_log_hz / f_sp;
    let logstep = (6.4f64).ln() / 27.0;
    if m >= min_log_mel {
        min_log_hz * (logstep * (m - min_log_mel)).exp()
    } else {
        f_sp * m
    }
}

/// librosa.filters.mel(sr, n_fft, n_mels, fmin, fmax) with the defaults
/// htk=False, norm="slaney". Row-major (n_mels, n_fft/2+1).
pub fn mel_filters_slaney(sr: f64, n_fft: usize, n_mels: usize, fmin: f64, fmax: f64) -> Vec<f32> {
    let nb = n_fft / 2 + 1;
    let fft_f: Vec<f64> = (0..nb).map(|i| i as f64 * sr / n_fft as f64).collect();
    let (lo, hi) = (hz_to_mel_slaney(fmin), hz_to_mel_slaney(fmax));
    let mel_f: Vec<f64> = (0..n_mels + 2)
        .map(|i| mel_to_hz_slaney(lo + (hi - lo) * i as f64 / (n_mels + 1) as f64))
        .collect();
    let mut w = vec![0f32; n_mels * nb];
    for m in 0..n_mels {
        let fd0 = mel_f[m + 1] - mel_f[m];
        let fd1 = mel_f[m + 2] - mel_f[m + 1];
        let enorm = 2.0 / (mel_f[m + 2] - mel_f[m]);
        for (j, &f) in fft_f.iter().enumerate() {
            let lower = (f - mel_f[m]) / fd0;
            let upper = (mel_f[m + 2] - f) / fd1;
            w[m * nb + j] = (lower.min(upper).max(0.0) * enorm) as f32;
        }
    }
    w
}

/// Multiply (n_mels x nb) filters with one power frame.
pub fn apply_mel(filters: &[f32], n_mels: usize, frame: &[f32]) -> Vec<f32> {
    let nb = frame.len();
    (0..n_mels)
        .map(|m| {
            filters[m * nb..(m + 1) * nb]
                .iter()
                .zip(frame)
                .map(|(a, b)| a * b)
                .sum()
        })
        .collect()
}

/// torchaudio.compliance.kaldi.fbank(wav, num_mel_bins) with its defaults:
/// 16 kHz, 25 ms / 10 ms frames, snip_edges, no dither, DC removal,
/// pre-emphasis 0.97, Povey window, 512-point power spectrum, mel bins from
/// 20 Hz to Nyquist, natural log. Returns frames x n_mels.
pub fn kaldi_fbank(wav: &[f32], sr: f64, n_mels: usize) -> Vec<Vec<f32>> {
    let flen = (sr * 0.025) as usize;
    let fshift = (sr * 0.010) as usize;
    if wav.len() < flen {
        return Vec::new();
    }
    let nfft = flen.next_power_of_two();
    let frames = 1 + (wav.len() - flen) / fshift;
    let win: Vec<f64> = (0..flen)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / (flen - 1) as f64).cos()).powf(0.85))
        .collect();
    let mel = |f: f64| 1127.0 * (1.0 + f / 700.0).ln();
    let (lo, hi) = (mel(20.0), mel(sr / 2.0));
    let delta = (hi - lo) / (n_mels + 1) as f64;
    let half = nfft / 2;
    let bw = sr / nfft as f64;
    let mut banks = vec![vec![0f64; half + 1]; n_mels];
    for (b, row) in banks.iter_mut().enumerate() {
        let (l, c, r) = (
            lo + b as f64 * delta,
            lo + (b + 1) as f64 * delta,
            lo + (b + 2) as f64 * delta,
        );
        for (j, w) in row.iter_mut().enumerate().take(half) {
            let m = mel(bw * j as f64);
            let up = (m - l) / (c - l);
            let down = (r - m) / (r - c);
            *w = up.min(down).max(0.0);
        }
    }
    let fft = Fft::new(nfft);
    let eps = f32::EPSILON as f64;
    (0..frames)
        .map(|f| {
            let s = &wav[f * fshift..f * fshift + flen];
            let mean = s.iter().map(|&v| v as f64).sum::<f64>() / flen as f64;
            let mut x: Vec<f64> = s.iter().map(|&v| v as f64 - mean).collect();
            for i in (1..flen).rev() {
                x[i] -= 0.97 * x[i - 1];
            }
            x[0] -= 0.97 * x[0];
            let mut buf = vec![C::default(); nfft];
            for i in 0..flen {
                buf[i].re = x[i] * win[i];
            }
            let spec = fft.forward(&buf);
            let pw: Vec<f64> = spec[..=half].iter().map(|c| c.norm_sqr()).collect();
            banks
                .iter()
                .map(|row| {
                    let e: f64 = row.iter().zip(&pw).map(|(a, b)| a * b).sum();
                    e.max(eps).ln() as f32
                })
                .collect()
        })
        .collect()
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Band-limited resampling with a Hann-windowed sinc (lowpass width 6,
/// rolloff 0.99), built the same way as torchaudio's `sinc_interp_hann`.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let g = gcd(from as u64, to as u64);
    let (orig, new) = ((from as u64 / g) as usize, (to as u64 / g) as usize);
    let lpw = 6.0f64;
    let base = orig.min(new) as f64 * 0.99;
    let width = (lpw * orig as f64 / base).ceil() as usize;
    let klen = 2 * width + orig;
    let scale = base / orig as f64;
    let kernels: Vec<Vec<f32>> = (0..new)
        .map(|j| {
            (0..klen)
                .map(|k| {
                    let idx = (k as f64 - width as f64) / orig as f64;
                    let mut t = (-(j as f64) / new as f64 + idx) * base;
                    t = t.clamp(-lpw, lpw);
                    let w = (t * PI / lpw / 2.0).cos().powi(2);
                    let t = t * PI;
                    let s = if t == 0.0 { 1.0 } else { t.sin() / t };
                    (s * w * scale) as f32
                })
                .collect()
        })
        .collect();
    let mut padded = vec![0f32; width];
    padded.extend_from_slice(x);
    padded.extend(std::iter::repeat_n(0f32, width + orig));
    let target = (new as u64 * x.len() as u64).div_ceil(orig as u64) as usize;
    let frames = (padded.len() - klen) / orig + 1;
    let mut out = Vec::with_capacity(frames * new);
    for f in 0..frames {
        let s = &padded[f * orig..f * orig + klen];
        for k in &kernels {
            out.push(s.iter().zip(k).map(|(a, b)| a * b).sum());
        }
    }
    out.truncate(target);
    out
}

/// librosa.effects.trim(y, top_db, frame_length=2048, hop_length=512):
/// returns the (start, end) sample range that is louder than `top_db` below
/// the loudest frame (RMS, centred frames, zero padding).
pub fn trim_range(y: &[f32], top_db: f64) -> (usize, usize) {
    let (fl, hop) = (2048usize, 512usize);
    if y.is_empty() {
        return (0, 0);
    }
    let n_frames = 1 + y.len() / hop;
    let ms: Vec<f64> = (0..n_frames)
        .map(|t| {
            let c = (t * hop) as isize;
            let mut acc = 0f64;
            for i in c - (fl / 2) as isize..c + (fl / 2) as isize {
                if i >= 0 && (i as usize) < y.len() {
                    let v = y[i as usize] as f64;
                    acc += v * v;
                }
            }
            acc / fl as f64
        })
        .collect();
    let peak = ms.iter().cloned().fold(0f64, f64::max).max(1e-10);
    let db: Vec<f64> = ms
        .iter()
        .map(|&m| 10.0 * m.max(1e-10).log10() - 10.0 * peak.log10())
        .collect();
    let loud: Vec<usize> = (0..n_frames).filter(|&i| db[i] > -top_db).collect();
    match (loud.first(), loud.last()) {
        (Some(&a), Some(&b)) => (a * hop, ((b + 1) * hop).min(y.len())),
        _ => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dft(x: &[C]) -> Vec<C> {
        let n = x.len();
        (0..n)
            .map(|k| {
                let mut acc = C::default();
                for (j, v) in x.iter().enumerate() {
                    let a = -2.0 * PI * (j * k) as f64 / n as f64;
                    acc = acc.add(v.mul(C {
                        re: a.cos(),
                        im: a.sin(),
                    }));
                }
                acc
            })
            .collect()
    }

    #[test]
    fn fft_matches_dft_for_mixed_lengths() {
        for n in [1usize, 2, 3, 8, 12, 30, 49, 400, 77] {
            let x: Vec<C> = (0..n)
                .map(|i| C {
                    re: ((i * 7 + 3) % 11) as f64 - 5.0,
                    im: ((i * 3) % 5) as f64,
                })
                .collect();
            let (a, b) = (Fft::new(n).forward(&x), dft(&x));
            for (p, q) in a.iter().zip(&b) {
                assert!(
                    (p.re - q.re).abs() < 1e-6 && (p.im - q.im).abs() < 1e-6,
                    "n={n}"
                );
            }
        }
    }

    #[test]
    fn reflect_and_stft_shapes() {
        assert_eq!(
            reflect_pad(&[1.0, 2.0, 3.0], 2, 2),
            vec![3.0, 2.0, 1.0, 2.0, 3.0, 2.0, 1.0]
        );
        let x: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.05).sin()).collect();
        let s = stft(&x, 400, 160, &hann(400), true, true);
        assert_eq!(s.len(), 1 + 16000 / 160);
        assert_eq!(s[0].len(), 201);
    }

    #[test]
    fn slaney_rows_and_kaldi_shape() {
        let m = mel_filters_slaney(16000.0, 400, 128, 0.0, 8000.0);
        assert_eq!(m.len(), 128 * 201);
        assert!(m.iter().all(|v| *v >= 0.0));
        // Every filter has some weight.
        for r in 0..128 {
            assert!(
                m[r * 201..(r + 1) * 201].iter().sum::<f32>() > 0.0,
                "row {r}"
            );
        }
        let x: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.07).sin() * 0.3).collect();
        let f = kaldi_fbank(&x, 16000.0, 80);
        assert_eq!(f.len(), 1 + (16000 - 400) / 160);
        assert_eq!(f[0].len(), 80);
    }

    #[test]
    fn resample_tone_keeps_frequency_and_length() {
        let x: Vec<f32> = (0..24000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 24000.0).sin())
            .collect();
        let y = resample(&x, 24000, 16000);
        assert_eq!(y.len(), 16000);
        for i in [4000usize, 8000, 12000] {
            let want = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16000.0).sin();
            assert!((y[i] - want).abs() < 0.02, "{i}: {} vs {want}", y[i]);
        }
    }

    #[test]
    fn trim_finds_the_loud_part() {
        let mut y = vec![0f32; 48000];
        for (i, v) in y.iter_mut().enumerate().take(30000).skip(10000) {
            *v = (i as f32 * 0.1).sin() * 0.5;
        }
        let (a, b) = trim_range(&y, 20.0);
        assert!(a > 8000 && a <= 10000, "{a}");
        assert!((30000..32000).contains(&b), "{b}");
    }
}
