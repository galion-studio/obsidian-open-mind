// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! PCM frames and framing.

/// Sample rate and frame length of a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameSpec {
    pub sample_rate: u32,
    pub frame_samples: usize,
}

impl FrameSpec {
    /// Moshi / PersonaPlex native: 24 kHz mono, 80 ms model frames.
    pub const MOSHI: FrameSpec = FrameSpec {
        sample_rate: 24_000,
        frame_samples: 1_920,
    };
    /// Whisper-class STT: 16 kHz mono, 20 ms frames.
    pub const STT16K: FrameSpec = FrameSpec {
        sample_rate: 16_000,
        frame_samples: 320,
    };

    /// Frame duration in milliseconds (rounded down).
    pub fn frame_ms(&self) -> u32 {
        (self.frame_samples as u64 * 1000 / self.sample_rate.max(1) as u64) as u32
    }
}

/// One mono PCM frame with a per-stream sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub seq: u64,
    pub samples: Vec<i16>,
}

impl Frame {
    pub fn silence(seq: u64, n: usize) -> Self {
        Frame {
            seq,
            samples: vec![0; n],
        }
    }

    /// Root-mean-square level, 0.0 ..= 1.0 of full scale.
    pub fn rms(&self) -> f32 {
        rms(&self.samples)
    }
}

/// RMS of i16 samples normalised to full scale. Plain scalar loop (portable).
pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut acc: f64 = 0.0;
    for &s in samples {
        let v = s as f64 / 32768.0;
        acc += v * v;
    }
    (acc / samples.len() as f64).sqrt() as f32
}

/// RMS in dBFS, floored at -120.
pub fn dbfs(rms: f32) -> f32 {
    if rms <= 1e-6 {
        -120.0
    } else {
        (20.0 * rms.log10()).max(-120.0)
    }
}

/// Cuts arbitrary-length PCM chunks into fixed frames.
#[derive(Debug)]
pub struct Framer {
    spec: FrameSpec,
    pending: Vec<i16>,
    next_seq: u64,
}

impl Framer {
    pub fn new(spec: FrameSpec) -> Self {
        Framer {
            spec,
            pending: Vec::with_capacity(spec.frame_samples * 2),
            next_seq: 0,
        }
    }

    pub fn spec(&self) -> FrameSpec {
        self.spec
    }

    /// Append samples; returns every complete frame now available.
    pub fn push(&mut self, chunk: &[i16]) -> Vec<Frame> {
        self.pending.extend_from_slice(chunk);
        let n = self.spec.frame_samples.max(1);
        let mut out = Vec::new();
        while self.pending.len() >= n {
            let rest = self.pending.split_off(n);
            let samples = std::mem::replace(&mut self.pending, rest);
            out.push(Frame {
                seq: self.next_seq,
                samples,
            });
            self.next_seq += 1;
        }
        out
    }

    /// Samples buffered but not yet a full frame.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

/// Deterministic synthetic signals for tests and loopback (no RNG crate).
pub mod synth {
    /// Sine tone at `freq` Hz, amplitude `amp` (0..1 of full scale).
    pub fn tone(sample_rate: u32, freq: f32, amp: f32, n: usize, phase0: usize) -> Vec<i16> {
        (0..n)
            .map(|i| {
                let t = (i + phase0) as f32 / sample_rate as f32;
                (amp * 32767.0 * (2.0 * std::f32::consts::PI * freq * t).sin()) as i16
            })
            .collect()
    }

    /// Low-level pseudo-random noise (xorshift32), amplitude `amp`.
    pub fn noise(seed: u32, amp: f32, n: usize) -> Vec<i16> {
        let mut x = seed.max(1);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                let u = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
                (u * amp * 32767.0) as i16
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framer_cuts_exact_frames_and_keeps_remainder() {
        let mut f = Framer::new(FrameSpec {
            sample_rate: 1000,
            frame_samples: 10,
        });
        assert!(f.push(&[1; 7]).is_empty());
        let out = f.push(&[2; 25]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].seq, 0);
        assert_eq!(out[2].seq, 2);
        assert_eq!(&out[0].samples[..7], &[1; 7]);
        assert_eq!(f.pending(), 2);
    }

    #[test]
    fn frame_ms_and_levels() {
        assert_eq!(FrameSpec::MOSHI.frame_ms(), 80);
        assert_eq!(FrameSpec::STT16K.frame_ms(), 20);
        assert_eq!(rms(&[]), 0.0);
        let full = synth::tone(24_000, 440.0, 1.0, 2400, 0);
        let r = rms(&full);
        assert!((r - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "{r}");
        assert!((dbfs(r) + 3.0).abs() < 0.1);
        assert_eq!(dbfs(0.0), -120.0);
    }
}
