// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Energy-based voice activity detection.
//!
//! Frame RMS in dBFS against an adaptive noise floor, with onset (frames above
//! threshold before speech starts) and hangover (frames below before it ends).
//! Pure scalar arithmetic, portable to every target. Not a neural VAD: it will
//! fire on loud non-speech; good enough for barge-in and end-of-utterance on a
//! headset or with browser echo cancellation. A model VAD is an M2 option.

use crate::audio::{dbfs, Frame};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadConfig {
    /// Speech when level exceeds noise floor by this many dB.
    pub margin_db: f32,
    /// Absolute floor: below this level is never speech.
    pub min_speech_dbfs: f32,
    /// Consecutive loud frames needed to declare speech start.
    pub onset_frames: u32,
    /// Consecutive quiet frames needed to declare speech end.
    pub hangover_frames: u32,
    /// Noise floor smoothing (0..1, fraction of new value per quiet frame).
    pub floor_alpha: f32,
    /// Initial noise floor.
    pub initial_floor_dbfs: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        VadConfig {
            margin_db: 12.0,
            min_speech_dbfs: -45.0,
            onset_frames: 2,
            hangover_frames: 6,
            floor_alpha: 0.05,
            initial_floor_dbfs: -60.0,
        }
    }
}

/// Edge events produced by the VAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    SpeechStart,
    SpeechEnd,
}

#[derive(Debug, Clone)]
pub struct EnergyVad {
    cfg: VadConfig,
    floor_db: f32,
    speaking: bool,
    loud_run: u32,
    quiet_run: u32,
}

impl EnergyVad {
    pub fn new(cfg: VadConfig) -> Self {
        EnergyVad {
            cfg,
            floor_db: cfg.initial_floor_dbfs,
            speaking: false,
            loud_run: 0,
            quiet_run: 0,
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    pub fn noise_floor_dbfs(&self) -> f32 {
        self.floor_db
    }

    /// Feed one frame; returns an edge event if the state flipped.
    pub fn process(&mut self, frame: &Frame) -> Option<VadEvent> {
        self.process_level(dbfs(frame.rms()))
    }

    /// Same as [`process`](Self::process) with a precomputed level in dBFS.
    pub fn process_level(&mut self, level_db: f32) -> Option<VadEvent> {
        let threshold = (self.floor_db + self.cfg.margin_db).max(self.cfg.min_speech_dbfs);
        let loud = level_db >= threshold;
        if loud {
            self.loud_run += 1;
            self.quiet_run = 0;
        } else {
            self.quiet_run += 1;
            self.loud_run = 0;
            // Adapt the floor only on non-speech frames, so speech does not
            // drag it up. Fast down, slow up.
            if !self.speaking {
                let a = if level_db < self.floor_db {
                    0.5
                } else {
                    self.cfg.floor_alpha
                };
                self.floor_db += a * (level_db - self.floor_db);
            }
        }
        if !self.speaking && self.loud_run >= self.cfg.onset_frames.max(1) {
            self.speaking = true;
            return Some(VadEvent::SpeechStart);
        }
        if self.speaking && self.quiet_run >= self.cfg.hangover_frames.max(1) {
            self.speaking = false;
            return Some(VadEvent::SpeechEnd);
        }
        None
    }

    pub fn reset(&mut self) {
        *self = EnergyVad::new(self.cfg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::synth;

    fn frame(samples: Vec<i16>) -> Frame {
        Frame { seq: 0, samples }
    }

    #[test]
    fn detects_tone_over_noise_with_onset_and_hangover() {
        let mut v = EnergyVad::new(VadConfig::default());
        let mut events = Vec::new();
        for i in 0..20 {
            events.extend(v.process(&frame(synth::noise(i + 1, 0.001, 480))));
        }
        assert!(events.is_empty(), "noise must not trigger");
        assert!(v.noise_floor_dbfs() < -55.0);
        assert_eq!(
            v.process(&frame(synth::tone(24_000, 300.0, 0.3, 480, 0))),
            None
        );
        assert_eq!(
            v.process(&frame(synth::tone(24_000, 300.0, 0.3, 480, 480))),
            Some(VadEvent::SpeechStart)
        );
        for i in 0..5 {
            assert_eq!(v.process(&frame(synth::noise(i + 50, 0.001, 480))), None);
        }
        assert_eq!(
            v.process(&frame(synth::noise(99, 0.001, 480))),
            Some(VadEvent::SpeechEnd)
        );
    }

    #[test]
    fn single_click_is_ignored_and_floor_tracks_noise() {
        let mut v = EnergyVad::new(VadConfig::default());
        assert_eq!(v.process_level(-10.0), None); // one loud frame < onset
        assert_eq!(v.process_level(-70.0), None);
        assert!(!v.is_speaking());
        // Background rising slowly (fan spinning up) becomes the floor, not speech.
        for _ in 0..200 {
            v.process_level(-50.0);
        }
        for _ in 0..200 {
            v.process_level(-40.0);
        }
        assert!(!v.is_speaking());
        assert!((v.noise_floor_dbfs() + 40.0).abs() < 1.0);
        v.reset();
        assert_eq!(v.noise_floor_dbfs(), -60.0);
    }
}
