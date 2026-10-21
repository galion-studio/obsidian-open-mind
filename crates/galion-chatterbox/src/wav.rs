// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! RIFF/WAVE reading (PCM 8/16/24/32-bit and IEEE float 32/64, any rate,
//! channels averaged to mono) and 16-bit PCM writing.

pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub fn read(b: &[u8]) -> crate::Result<Audio> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let u16le = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32le = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, usize, u32, usize)> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32le(pos + 4) as usize;
        let body = pos + 8;
        let end = body.saturating_add(len).min(b.len());
        if id == b"fmt " && len >= 16 && body + 16 <= b.len() {
            let mut tag = u16le(body);
            if tag == 0xFFFE && len >= 26 && body + 26 <= b.len() {
                tag = u16le(body + 24); // WAVE_FORMAT_EXTENSIBLE sub-format
            }
            fmt = Some((
                tag,
                u16le(body + 2) as usize,
                u32le(body + 4),
                u16le(body + 14) as usize,
            ));
        } else if id == b"data" {
            let (tag, ch, rate, bits) = fmt.ok_or("data chunk before fmt chunk")?;
            if ch == 0 || bits == 0 || rate == 0 {
                return Err("bad fmt chunk".into());
            }
            let bps = bits.div_ceil(8);
            let data = &b[body..end];
            let n = data.len() / (bps * ch);
            let sample = |i: usize| -> crate::Result<f32> {
                let s = &data[i * bps..(i + 1) * bps];
                Ok(match (tag, bits) {
                    (1, 8) => (s[0] as f32 - 128.0) / 128.0,
                    (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                    (1, 24) => {
                        (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0
                    }
                    (1, 32) => {
                        i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0
                    }
                    (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    (3, 64) => {
                        f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32
                    }
                    _ => return Err(format!("unsupported WAV format tag {tag} with {bits} bits")),
                })
            };
            let mut samples = Vec::with_capacity(n);
            for f in 0..n {
                let mut acc = 0f32;
                for c in 0..ch {
                    acc += sample(f * ch + c)?;
                }
                samples.push(acc / ch as f32);
            }
            return Ok(Audio {
                sample_rate: rate,
                samples,
            });
        }
        pos = body + len + (len & 1);
    }
    Err("no data chunk".into())
}

/// 16-bit PCM mono WAV: samples scaled by 32767 and clamped, as the legacy
/// server did (`(wav * 32767).clamp(-32768, 32767)`).
pub fn encode_pcm16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let pcm = to_pcm16(samples);
    let data = pcm.len() as u32;
    let mut b = Vec::with_capacity(44 + pcm.len());
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.extend_from_slice(&pcm);
    b
}

/// Raw little-endian s16 PCM (the worker.py frame payload).
pub fn to_pcm16(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_pcm16() {
        let x = [0.0f32, 0.5, -0.5, 1.0, -1.0];
        let w = encode_pcm16(&x, 24_000);
        assert_eq!(w.len(), 44 + 10);
        let a = read(&w).unwrap();
        assert_eq!(a.sample_rate, 24_000);
        for (p, q) in a.samples.iter().zip(x) {
            assert!((p - q).abs() < 1e-3);
        }
        assert!(read(b"nope").is_err());
    }
}
