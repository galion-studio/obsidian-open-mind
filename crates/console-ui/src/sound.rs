// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal Galion sound set.
//!
//! Every pitch is `432 Hz` times a just-intonation ratio (small whole
//! numbers). Admin cues are two-note lines. Bell strikes are a separate
//! timbre and are only for a window opening, closing, minimising or settling.
//! Resonance (a long tonic, or the bell's own fundamental) is on unless a
//! caller turns it off. Silence is not a tuning: it is an opt-in that
//! replaces the recording with digital silence of the same length.

use std::sync::OnceLock;

/// Tonic. Just ratios multiply this, they do not replace it.
pub const TONIC_HZ: f64 = 432.0;
pub const SAMPLE_RATE: u32 = 22_050;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: u16,
    pub den: u16,
}

impl Ratio {
    pub const fn new(num: u16, den: u16) -> Self {
        Self { num, den }
    }

    /// Frequency in hertz. `432 * num / den`.
    pub fn hz(self) -> f64 {
        TONIC_HZ * f64::from(self.num) / f64::from(self.den)
    }
}

pub const UNISON: Ratio = Ratio::new(1, 1);
/// Octave below the tonic.
pub const OCTAVE_BELOW: Ratio = Ratio::new(1, 2);
pub const MINOR_SECOND: Ratio = Ratio::new(16, 15);
pub const MAJOR_THIRD: Ratio = Ratio::new(5, 4);
pub const FOURTH: Ratio = Ratio::new(4, 3);
pub const FIFTH: Ratio = Ratio::new(3, 2);
pub const OCTAVE: Ratio = Ratio::new(2, 1);

/// Partials of a bell, each a just ratio of that bell's fundamental.
const BELL_PARTIALS: &[Ratio] = &[
    Ratio::new(1, 1),
    Ratio::new(3, 2),
    Ratio::new(2, 1),
    Ratio::new(5, 2),
    Ratio::new(3, 1),
    Ratio::new(4, 1),
];
const BELL_GAIN: &[f64] = &[1.0, 0.58, 0.36, 0.2, 0.12, 0.07];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    ApprovalNeeded = 0,
    TaskDone = 1,
    Error = 2,
    BellOpen = 3,
    BellClose = 4,
    BellMinimise = 5,
    BellSettle = 6,
}

impl Sound {
    pub const ALL: [Sound; 7] = [
        Sound::ApprovalNeeded,
        Sound::TaskDone,
        Sound::Error,
        Sound::BellOpen,
        Sound::BellClose,
        Sound::BellMinimise,
        Sound::BellSettle,
    ];

    pub fn is_bell(self) -> bool {
        matches!(
            self,
            Sound::BellOpen | Sound::BellClose | Sound::BellMinimise | Sound::BellSettle
        )
    }

    pub fn seconds(self) -> f64 {
        match self {
            Sound::ApprovalNeeded => 1.05,
            Sound::TaskDone => 1.10,
            Sound::Error => 0.72,
            Sound::BellOpen => 1.25,
            Sound::BellClose => 1.05,
            Sound::BellMinimise => 0.48,
            Sound::BellSettle => 1.55,
        }
    }

    /// URL path of the resonant recording (silence is a playback choice).
    pub fn path(self) -> &'static str {
        match self {
            Sound::ApprovalNeeded => "/_ui/sound/approval.wav",
            Sound::TaskDone => "/_ui/sound/done.wav",
            Sound::Error => "/_ui/sound/error.wav",
            Sound::BellOpen => "/_ui/sound/bell-open.wav",
            Sound::BellClose => "/_ui/sound/bell-close.wav",
            Sound::BellMinimise => "/_ui/sound/bell-minimise.wav",
            Sound::BellSettle => "/_ui/sound/bell-settle.wav",
        }
    }

    pub fn from_path(path: &str) -> Option<Sound> {
        Self::ALL.into_iter().find(|s| s.path() == path)
    }
}

/// A window may strike a bell. Nothing else may.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    Open,
    Close,
    Minimise,
    Settle,
}

pub fn bell(window: Window) -> Sound {
    match window {
        Window::Open => Sound::BellOpen,
        Window::Close => Sound::BellClose,
        Window::Minimise => Sound::BellMinimise,
        Window::Settle => Sound::BellSettle,
    }
}

/// The three cues the admin desk plays. None of them is a bell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admin {
    ApprovalNeeded,
    TaskDone,
    Error,
}

pub fn admin(kind: Admin) -> Sound {
    match kind {
        Admin::ApprovalNeeded => Sound::ApprovalNeeded,
        Admin::TaskDone => Sound::TaskDone,
        Admin::Error => Sound::Error,
    }
}

/// Melody notes of an admin cue, as just ratios of the tonic.
pub fn cue_notes(sound: Sound) -> &'static [(Ratio, f64, f64)] {
    match sound {
        // Unresolved fourth, then fifth: the decision is still open.
        Sound::ApprovalNeeded => &[(FOURTH, 0.0, 0.32), (FIFTH, 0.28, 0.78)],
        // Fifth resolves up to the octave.
        Sound::TaskDone => &[(FIFTH, 0.0, 0.24), (OCTAVE, 0.22, 0.82)],
        // Minor second against the tonic, twice. Not a strike.
        Sound::Error => &[(MINOR_SECOND, 0.02, 0.16), (MINOR_SECOND, 0.22, 0.36)],
        _ => &[],
    }
}

/// Resonance on, silence off. Callers opt into silence; they do not opt into resonance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voice {
    pub resonance: bool,
    pub silence: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            resonance: true,
            silence: false,
        }
    }
}

impl Voice {
    pub const fn silent() -> Self {
        Self {
            resonance: true,
            silence: true,
        }
    }
}

struct Shape {
    fundamental: Ratio,
    decay: f64,
}

fn shape(sound: Sound) -> Option<Shape> {
    match sound {
        Sound::BellOpen => Some(Shape {
            fundamental: UNISON,
            decay: 0.42,
        }),
        Sound::BellClose => Some(Shape {
            fundamental: OCTAVE_BELOW,
            decay: 0.36,
        }),
        Sound::BellMinimise => Some(Shape {
            fundamental: OCTAVE,
            decay: 0.16,
        }),
        Sound::BellSettle => Some(Shape {
            fundamental: MAJOR_THIRD,
            decay: 0.55,
        }),
        _ => None,
    }
}

fn sine(hz: f64, t: f64) -> f64 {
    (t * hz * std::f64::consts::TAU).sin()
}

fn note_amp(t: f64, start: f64, end: f64) -> f64 {
    if t < start || t >= end {
        return 0.0;
    }
    let up = ((t - start) / 0.012).clamp(0.0, 1.0);
    let down = ((end - t) / 0.045).clamp(0.0, 1.0);
    up * down
}

/// Instantaneous partials: `(hertz, linear amplitude)` before the end fade.
fn partials(sound: Sound, t: f64, resonance: bool) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    if let Some(bell) = shape(sound) {
        let env = (t / 0.003).clamp(0.0, 1.0) * (-t / bell.decay).exp();
        for (partial, gain) in BELL_PARTIALS.iter().zip(BELL_GAIN.iter()) {
            let hz = bell.fundamental.hz() * f64::from(partial.num) / f64::from(partial.den);
            out.push((hz, gain * env));
        }
        if resonance {
            out.push((bell.fundamental.hz(), 0.2 * (-t / 0.95).exp()));
        }
    } else {
        for (ratio, start, end) in cue_notes(sound) {
            out.push((ratio.hz(), 0.7 * note_amp(t, *start, *end)));
        }
        if resonance {
            out.push((TONIC_HZ, 0.18 * (-t / 0.9).exp()));
        }
    }
    out
}

fn sample(sound: Sound, t: f64, resonance: bool) -> f64 {
    let mut s = 0.0;
    for (hz, amp) in partials(sound, t, resonance) {
        s += amp * sine(hz, t);
    }
    let tail = (sound.seconds() - t) / 0.012;
    s * tail.clamp(0.0, 1.0)
}

/// PCM for `sound`. Silence is the same length, all zeros.
pub fn pcm(sound: Sound, voice: Voice) -> Vec<i16> {
    let n = (sound.seconds() * f64::from(SAMPLE_RATE)).round() as usize;
    if voice.silence {
        return vec![0; n];
    }
    let mut raw = vec![0.0; n];
    let mut peak = 0.0f64;
    for (i, slot) in raw.iter_mut().enumerate() {
        let t = i as f64 / f64::from(SAMPLE_RATE);
        let s = sample(sound, t, voice.resonance);
        *slot = s;
        peak = peak.max(s.abs());
    }
    let scale = if peak > 0.0 { 0.82 / peak } else { 0.0 };
    raw.into_iter()
        .map(|s| {
            let v = (s * scale * 32_767.0).round();
            v.clamp(-32_767.0, 32_767.0) as i16
        })
        .collect()
}

fn wav_header(nsamples: u32) -> [u8; 44] {
    let data = nsamples.saturating_mul(2);
    let riff = 36u32.saturating_add(data);
    let rate = SAMPLE_RATE;
    let byte_rate = rate.saturating_mul(2);
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&riff.to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

pub fn wav(sound: Sound, voice: Voice) -> Vec<u8> {
    let samples = pcm(sound, voice);
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(&wav_header(samples.len() as u32));
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Resonant, not silent. Built once per process.
pub fn cached_wav(sound: Sound) -> &'static [u8] {
    static SLOTS: [OnceLock<Vec<u8>>; 7] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    SLOTS[sound as usize]
        .get_or_init(|| wav(sound, Voice::default()))
        .as_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn energy(pcm: &[i16], from: f64, to: f64) -> f64 {
        let a = (from * f64::from(SAMPLE_RATE)) as usize;
        let b = ((to * f64::from(SAMPLE_RATE)) as usize).min(pcm.len());
        pcm[a..b]
            .iter()
            .map(|s| {
                let x = f64::from(*s);
                x * x
            })
            .sum()
    }

    fn drive(sound: Sound, t: f64) -> f64 {
        partials(sound, t, false)
            .into_iter()
            .map(|(_, amp)| amp)
            .sum()
    }

    #[test]
    fn pitches_are_432_times_just_ratios() {
        assert_eq!(UNISON.hz(), 432.0);
        assert_eq!(FIFTH.hz(), 648.0);
        assert_eq!(FOURTH.hz(), 576.0);
        assert_eq!(OCTAVE.hz(), 864.0);
        assert_eq!(OCTAVE_BELOW.hz(), 216.0);
        assert_eq!(MAJOR_THIRD.hz(), 540.0);
        assert!((MINOR_SECOND.hz() - 432.0 * 16.0 / 15.0).abs() < 1e-9);
        for sound in Sound::ALL {
            for (ratio, _, _) in cue_notes(sound) {
                assert!(ratio.den > 0);
                assert!(
                    (ratio.hz() - TONIC_HZ * f64::from(ratio.num) / f64::from(ratio.den)).abs()
                        < 1e-9
                );
            }
        }
    }

    #[test]
    fn bells_are_only_the_four_window_events() {
        assert_eq!(bell(Window::Open), Sound::BellOpen);
        assert_eq!(bell(Window::Close), Sound::BellClose);
        assert_eq!(bell(Window::Minimise), Sound::BellMinimise);
        assert_eq!(bell(Window::Settle), Sound::BellSettle);
        for w in [
            Window::Open,
            Window::Close,
            Window::Minimise,
            Window::Settle,
        ] {
            assert!(bell(w).is_bell());
        }
        for kind in [Admin::ApprovalNeeded, Admin::TaskDone, Admin::Error] {
            assert!(!admin(kind).is_bell());
            assert!(!cue_notes(admin(kind)).is_empty());
        }
        assert!(cue_notes(Sound::BellOpen).is_empty());
        assert_eq!(BELL_PARTIALS.len(), 6);
    }

    #[test]
    fn strikes_attack_and_cues_do_not() {
        let voice = Voice::default();
        for sound in [
            Sound::BellOpen,
            Sound::BellClose,
            Sound::BellMinimise,
            Sound::BellSettle,
        ] {
            let hit = drive(sound, 0.004);
            assert!(hit > 1.5, "{sound:?} has not struck at 4 ms ({hit})");
        }
        for sound in [Sound::ApprovalNeeded, Sound::TaskDone, Sound::Error] {
            let hit = drive(sound, 0.004);
            assert!(hit < 0.5, "{sound:?} strikes at 4 ms ({hit})");
            let bell_pcm = pcm(Sound::BellOpen, voice);
            assert_ne!(pcm(sound, voice), bell_pcm);
        }
    }

    #[test]
    fn resonance_is_default_and_silence_is_empty() {
        let voice = Voice::default();
        assert!(voice.resonance);
        assert!(!voice.silence);
        let wet = pcm(Sound::ApprovalNeeded, voice);
        let dry = pcm(
            Sound::ApprovalNeeded,
            Voice {
                resonance: false,
                silence: false,
            },
        );
        assert!(energy(&wet, 0.9, 1.04) > energy(&dry, 0.9, 1.04) * 4.0);
        let quiet = pcm(Sound::TaskDone, Voice::silent());
        assert_eq!(quiet.len(), pcm(Sound::TaskDone, voice).len());
        assert!(quiet.iter().all(|s| *s == 0));
        let bytes = wav(Sound::Error, voice);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert!(bytes.len() > 44);
        assert_eq!(cached_wav(Sound::Error).len(), bytes.len());
    }
}
