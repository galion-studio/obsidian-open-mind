// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Minimal RIFF/WAVE reader: PCM 16-bit, any rate, channels mixed to mono.

#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

pub fn parse(b: &[u8]) -> Result<Wav, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let u16le = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32le = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32le(pos + 4) as usize;
        let body = pos + 8;
        let end = body.checked_add(len).ok_or("chunk overflow")?;
        if end > b.len() {
            return Err("truncated chunk".into());
        }
        if id == b"fmt " && len >= 16 {
            fmt = Some((
                u16le(body),
                u16le(body + 2),
                u32le(body + 4),
                u16le(body + 14),
            ));
        } else if id == b"data" {
            let (format, ch, rate, bits) = fmt.ok_or("data before fmt")?;
            if format != 1 || bits != 16 || ch == 0 {
                return Err(format!(
                    "unsupported wav: format {format}, {bits} bit, {ch} ch"
                ));
            }
            let ch = ch as usize;
            let samples = b[body..end]
                .chunks_exact(2 * ch)
                .map(|f| {
                    let sum: i32 = f
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|s| i16::from_le_bytes(*s) as i32)
                        .sum();
                    (sum / ch as i32) as i16
                })
                .collect();
            return Ok(Wav {
                sample_rate: rate,
                samples,
            });
        }
        pos = end + (len & 1);
    }
    Err("no data chunk".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(ch: u16, data: &[i16]) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(ch.to_le_bytes());
        v.extend(16_000u32.to_le_bytes());
        v.extend((16_000u32 * 2 * ch as u32).to_le_bytes());
        v.extend((2 * ch).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend((data.len() as u32 * 2).to_le_bytes());
        for s in data {
            v.extend(s.to_le_bytes());
        }
        v
    }

    #[test]
    fn parses_mono_and_mixes_stereo() {
        let w = parse(&wav(1, &[1, -2, 3])).unwrap();
        assert_eq!((w.sample_rate, w.samples), (16_000, vec![1, -2, 3]));
        let w = parse(&wav(2, &[10, 20, -10, -30])).unwrap();
        assert_eq!(w.samples, vec![15, -20]);
        assert!(parse(b"nope").is_err());
        let mut t = wav(1, &[1, 2]);
        t.truncate(t.len() - 1);
        assert!(parse(&t).is_err());
    }
}
