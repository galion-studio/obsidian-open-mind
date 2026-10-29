// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Audio frame conversion and a small RIFF/WAVE reader for tests and the
//! benchmark CLI. The wire format is 16 kHz mono signed 16-bit little-endian
//! PCM; Moonshine wants f32 in [-1, 1].

/// Wire sample rate.
pub const SAMPLE_RATE: u32 = 16_000;
/// Smallest accepted frame: 10 ms (protocol asks for 20-100 ms; a little slack).
pub const MIN_FRAME_SAMPLES: usize = 160;
/// Largest accepted frame: 250 ms.
pub const MAX_FRAME_SAMPLES: usize = 4_000;

/// Why a binary frame was rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    OddLength(usize),
    TooShort(usize),
    TooLong(usize),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::OddLength(n) => {
                write!(f, "frame has odd byte length {n}; expected PCM16LE")
            }
            FrameError::TooShort(n) => write!(
                f,
                "frame of {n} samples is shorter than {MIN_FRAME_SAMPLES} (10 ms at 16 kHz)"
            ),
            FrameError::TooLong(n) => write!(
                f,
                "frame of {n} samples is longer than {MAX_FRAME_SAMPLES} (250 ms at 16 kHz)"
            ),
        }
    }
}

/// Convert one PCM16LE frame to f32 samples (`/ 32768`).
pub fn pcm16le_to_f32(bytes: &[u8]) -> Result<Vec<f32>, FrameError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(FrameError::OddLength(bytes.len()));
    }
    let n = bytes.len() / 2;
    if n < MIN_FRAME_SAMPLES {
        return Err(FrameError::TooShort(n));
    }
    if n > MAX_FRAME_SAMPLES {
        return Err(FrameError::TooLong(n));
    }
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0)
        .collect())
}

/// Convert f32 samples back to PCM16LE (clamped); used by tests and the probe.
pub fn f32_to_pcm16le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Samples to milliseconds at the wire rate.
pub fn samples_to_ms(n: u64) -> u64 {
    n * 1000 / u64::from(SAMPLE_RATE)
}

/// Decoded WAV: mono f32 samples and the file's sample rate.
#[derive(Debug)]
pub struct Wav {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// Parse a PCM16 WAV file (mono, or the first channel of multi-channel).
pub fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(len).min(bytes.len());
        let body = &bytes[body_start..body_end];
        if id == b"fmt " {
            if body.len() < 16 {
                return Err("short fmt chunk".into());
            }
            let format = u16::from_le_bytes([body[0], body[1]]);
            let channels = u16::from_le_bytes([body[2], body[3]]);
            let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            fmt = Some((format, channels, rate, bits));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or("data chunk before fmt chunk")?;
            // 1 = PCM, 0xFFFE = WAVE_FORMAT_EXTENSIBLE (PCM sub-format assumed).
            if !(format == 1 || format == 0xFFFE) || bits != 16 || channels == 0 {
                return Err(format!(
                    "unsupported WAV: format {format}, {bits} bits, {channels} channels (need PCM16)"
                ));
            }
            let step = usize::from(channels) * 2;
            let samples = body
                .chunks_exact(step)
                .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0)
                .collect();
            return Ok(Wav {
                sample_rate: rate,
                samples,
            });
        }
        pos = body_start + len + (len & 1);
    }
    Err("no data chunk".into())
}

/// Build a minimal mono PCM16 WAV (tests).
pub fn build_wav(sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let data = f32_to_pcm16le(samples);
    let mut v = Vec::with_capacity(44 + data.len());
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v.extend_from_slice(&data);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_extremes_and_zero() {
        let mut b = Vec::new();
        for v in [0i16, i16::MAX, i16::MIN, -1, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.resize(MIN_FRAME_SAMPLES * 2, 0);
        let f = pcm16le_to_f32(&b).unwrap();
        assert_eq!(f.len(), MIN_FRAME_SAMPLES);
        assert_eq!(f[0], 0.0);
        assert!((f[1] - 0.999_969_5).abs() < 1e-6);
        assert_eq!(f[2], -1.0);
        assert_eq!(f[3], -1.0 / 32768.0);
        assert_eq!(f[4], 1.0 / 32768.0);
        assert!(f.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn little_endian_byte_order() {
        let mut b = vec![0x34, 0x12];
        b.resize(MIN_FRAME_SAMPLES * 2, 0);
        let f = pcm16le_to_f32(&b).unwrap();
        assert_eq!(f[0], f32::from(0x1234i16) / 32768.0);
    }

    #[test]
    fn frame_bounds() {
        assert_eq!(pcm16le_to_f32(&[0u8; 641]), Err(FrameError::OddLength(641)));
        assert_eq!(pcm16le_to_f32(&[0u8; 100]), Err(FrameError::TooShort(50)));
        assert!(pcm16le_to_f32(&[0u8; 640]).is_ok()); // 20 ms
        assert!(pcm16le_to_f32(&[0u8; 3200]).is_ok()); // 100 ms
        assert_eq!(
            pcm16le_to_f32(&vec![0u8; 8002]),
            Err(FrameError::TooLong(4001))
        );
    }

    #[test]
    fn round_trip_pcm() {
        let s: Vec<f32> = (0..320).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
        let back = pcm16le_to_f32(&f32_to_pcm16le(&s)).unwrap();
        for (a, b) in s.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn wav_round_trip() {
        let s: Vec<f32> = (0..1600)
            .map(|i| if i % 2 == 0 { 0.25 } else { -0.25 })
            .collect();
        let w = parse_wav(&build_wav(16_000, &s)).unwrap();
        assert_eq!(w.sample_rate, 16_000);
        assert_eq!(w.samples.len(), 1600);
        assert!((w.samples[0] - 0.25).abs() < 1e-3);
    }

    #[test]
    fn wav_rejects_garbage() {
        assert!(parse_wav(b"hello").is_err());
        assert!(parse_wav(b"RIFF\0\0\0\0WAVE").is_err());
    }

    #[test]
    fn ms_conversion() {
        assert_eq!(samples_to_ms(16_000), 1000);
        assert_eq!(samples_to_ms(320), 20);
    }
}
