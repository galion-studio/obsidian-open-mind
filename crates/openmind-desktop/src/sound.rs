// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The seven-tone desktop set. Every pitch is an integer ratio of 432 Hz.
//!
//! Each tone is one sine with a short fade. Generating one takes a few
//! thousand multiplies. There is no sample library and no network fetch.

/// Reference pitch for the desktop, in hertz.
pub const REFERENCE_HZ: u32 = 432;

/// PCM rate for the tones. 16 kHz is enough for these pitches and keeps
/// each clip under a few kilobytes.
pub const SAMPLE_RATE: u32 = 16_000;

/// Peak amplitude, well below full scale.
const AMPLITUDE: f32 = 0.32;

/// Fade in and out, in milliseconds.
const FADE_MS: u32 = 8;

/// One tone in the minimal set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tone {
    /// Short id used in the page (`data-tone`) and by [`wav`].
    pub id: &'static str,
    /// What the tone means on the control console.
    pub role: &'static str,
    /// Numerator of the ratio against [`REFERENCE_HZ`].
    pub num: u32,
    /// Denominator of that ratio. [`REFERENCE_HZ`] * `num` divides evenly.
    pub den: u32,
    /// Length in milliseconds.
    pub ms: u32,
}

impl Tone {
    /// Pitch in hertz. Exact, because every ratio divides 432.
    #[must_use]
    pub fn hz(self) -> u32 {
        REFERENCE_HZ * self.num / self.den
    }
}

/// Wake, ask, hear, notice, done, deny, close. Seven, no more.
pub const TONES: &[Tone] = &[
    Tone {
        id: "wake",
        role: "The console is ready",
        num: 1,
        den: 1,
        ms: 160,
    },
    Tone {
        id: "ask",
        role: "Voice armed, or a question",
        num: 5,
        den: 4,
        ms: 120,
    },
    Tone {
        id: "hear",
        role: "A file name was kept on the page",
        num: 9,
        den: 8,
        ms: 100,
    },
    Tone {
        id: "notice",
        role: "The stack wants a look",
        num: 4,
        den: 3,
        ms: 140,
    },
    Tone {
        id: "done",
        role: "A command was kept on the page",
        num: 3,
        den: 2,
        ms: 160,
    },
    Tone {
        id: "deny",
        role: "Refused. Nothing was sent",
        num: 3,
        den: 4,
        ms: 180,
    },
    Tone {
        id: "close",
        role: "Voice disarmed",
        num: 2,
        den: 3,
        ms: 140,
    },
];

/// The tone with this id, if it is one of the seven.
#[must_use]
pub fn tone(id: &str) -> Option<&'static Tone> {
    TONES.iter().find(|t| t.id == id)
}

/// 16-bit mono WAV for `id`, or `None` when `id` is not in the set.
#[must_use]
pub fn wav(id: &str) -> Option<Vec<u8>> {
    let tone = tone(id)?;
    Some(encode_pcm16(&samples(tone), SAMPLE_RATE))
}

/// One sine at the tone's pitch, faded in and out.
#[must_use]
pub fn samples(tone: &Tone) -> Vec<f32> {
    let n = (u64::from(tone.ms) * u64::from(SAMPLE_RATE) / 1000) as usize;
    let fade = (u64::from(FADE_MS) * u64::from(SAMPLE_RATE) / 1000) as usize;
    let fade = fade.min(n / 2);
    let step = std::f32::consts::TAU * tone.hz() as f32 / SAMPLE_RATE as f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let env = envelope(i, n, fade);
        out.push((step * i as f32).sin() * AMPLITUDE * env);
    }
    out
}

fn envelope(i: usize, n: usize, fade: usize) -> f32 {
    if fade == 0 {
        return 1.0;
    }
    let x = if i < fade {
        i as f32 / fade as f32
    } else if i + fade >= n {
        (n - 1 - i) as f32 / fade as f32
    } else {
        return 1.0;
    };
    0.5 - 0.5 * (std::f32::consts::PI * x).cos()
}

fn encode_pcm16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// Sum of squares of the correlation with a sine and cosine at `hz`.
#[must_use]
pub fn band_energy(samples: &[f32], hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz / SAMPLE_RATE as f32;
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for (n, s) in samples.iter().enumerate() {
        let p = w * n as f32;
        re += s * p.cos();
        im += s * p.sin();
    }
    re * re + im * im
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_tones_from_432() {
        assert_eq!(TONES.len(), 7);
        let mut ids = Vec::new();
        for tone in TONES {
            assert_eq!(
                REFERENCE_HZ * tone.num % tone.den,
                0,
                "{} ratio does not divide 432",
                tone.id
            );
            assert!(tone.hz() >= 200 && tone.hz() <= 800, "{}", tone.id);
            assert!(tone.ms >= 80 && tone.ms <= 200, "{}", tone.id);
            ids.push(tone.id);
        }
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 7);
        assert_eq!(tone("wake").unwrap().hz(), 432);
        assert!(tone("nope").is_none());
        assert!(wav("nope").is_none());
    }

    #[test]
    fn wake_is_432_not_440() {
        let wake = samples(tone("wake").unwrap());
        let at = |hz: f32| band_energy(&wake, hz);
        assert!(at(432.0) > at(440.0) * 4.0, "432 vs concert 440");
        let grid = [288.0, 324.0, 432.0, 440.0, 486.0, 540.0, 576.0, 648.0];
        for tone in TONES {
            let clip = samples(tone);
            let own = band_energy(&clip, tone.hz() as f32);
            for hz in grid {
                if (hz - tone.hz() as f32).abs() < 0.5 {
                    continue;
                }
                let other = band_energy(&clip, hz);
                assert!(
                    own > other * 2.0,
                    "{} peaks at {} not {hz} ({own} vs {other})",
                    tone.id,
                    tone.hz()
                );
            }
        }
    }

    #[test]
    fn wav_is_pcm16_mono() {
        let w = wav("done").unwrap();
        assert!(w.starts_with(b"RIFF"));
        assert_eq!(&w[8..12], b"WAVE");
        let rate = u32::from_le_bytes(w[24..28].try_into().unwrap());
        assert_eq!(rate, SAMPLE_RATE);
        let data = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
        assert_eq!(w.len(), 44 + data);
        assert_eq!(data % 2, 0);
        let samples = samples(tone("done").unwrap());
        for s in &samples {
            assert!(s.abs() <= AMPLITUDE + 0.001);
        }
    }
}
