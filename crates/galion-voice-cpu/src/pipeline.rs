// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `CpuPipelineBackend`: STT -> text model -> TTS behind the voice-core
//! `VoiceBackend` trait (turn-based, barge-in cancels the reply).
//!
//! M2 spike: the work runs synchronously inside `end_of_user_turn()`, so a slow
//! STT stalls that one tick. Moving it to a worker thread is M2 follow-up work;
//! the trait already allows it, since `poll()` is how results come back.

use std::collections::VecDeque;

use galion_voice_core::audio::{Frame, FrameSpec};
use galion_voice_core::backend::{
    BackendError, BackendEvent, Capabilities, DuplexMode, VoiceBackend,
};
use galion_voice_core::persona::PersonaSource;

use crate::resample::{f32_to_i16, i16_to_f32, linear};
use crate::stt::{SpeechToText, TextModel, TextToSpeech};

/// Timings of the last turn, in milliseconds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TurnTiming {
    pub utterance_ms: u64,
    pub stt_ms: u64,
    pub model_ms: u64,
    pub tts_ms: u64,
}

pub struct CpuPipelineBackend<S: SpeechToText, M: TextModel, T: TextToSpeech> {
    stt: S,
    model: M,
    tts: T,
    spec: FrameSpec,
    system_prompt: String,
    voice: String,
    started: bool,
    closed: bool,
    utterance: Vec<i16>,
    queue: VecDeque<BackendEvent>,
    out_seq: u64,
    pub last_transcript: String,
    pub last_timing: TurnTiming,
}

impl<S: SpeechToText, M: TextModel, T: TextToSpeech> CpuPipelineBackend<S, M, T> {
    pub fn new(stt: S, model: M, tts: T, spec: FrameSpec) -> Self {
        CpuPipelineBackend {
            stt,
            model,
            tts,
            spec,
            system_prompt: String::new(),
            voice: String::new(),
            started: false,
            closed: false,
            utterance: Vec::new(),
            queue: VecDeque::new(),
            out_seq: 0,
            last_transcript: String::new(),
            last_timing: TurnTiming::default(),
        }
    }

    fn ready(&self) -> Result<(), BackendError> {
        if self.closed {
            Err(BackendError::Closed)
        } else if !self.started {
            Err(BackendError::NotStarted)
        } else {
            Ok(())
        }
    }

    fn run_turn(&mut self) -> Result<(), String> {
        let ms = |t: std::time::Instant| t.elapsed().as_millis() as u64;
        let pcm = std::mem::take(&mut self.utterance);
        let mut timing = TurnTiming {
            utterance_ms: pcm.len() as u64 * 1000 / self.spec.sample_rate.max(1) as u64,
            ..TurnTiming::default()
        };
        let t = std::time::Instant::now();
        let input = linear(
            &i16_to_f32(&pcm),
            self.spec.sample_rate,
            self.stt.sample_rate(),
        );
        let text = self.stt.transcribe(&input)?;
        timing.stt_ms = ms(t);
        self.last_transcript = text.clone();
        if text.trim().is_empty() {
            self.last_timing = timing;
            self.queue.push_back(BackendEvent::TurnDone);
            return Ok(());
        }
        self.queue
            .push_back(BackendEvent::Text(format!("user: {text}")));
        let t = std::time::Instant::now();
        let reply = self.model.reply(&self.system_prompt, &text)?;
        timing.model_ms = ms(t);
        self.queue.push_back(BackendEvent::Text(reply.clone()));
        let t = std::time::Instant::now();
        let speech = self.tts.synthesize(&reply, &self.voice)?;
        let speech = f32_to_i16(&linear(
            &i16_to_f32(&speech),
            self.tts.sample_rate(),
            self.spec.sample_rate,
        ));
        timing.tts_ms = ms(t);
        let n = self.spec.frame_samples.max(1);
        for chunk in speech.chunks(n) {
            let mut samples = chunk.to_vec();
            samples.resize(n, 0);
            self.queue.push_back(BackendEvent::Audio(Frame {
                seq: self.out_seq,
                samples,
            }));
            self.out_seq += 1;
        }
        self.queue.push_back(BackendEvent::TurnDone);
        self.last_timing = timing;
        Ok(())
    }
}

impl<S: SpeechToText, M: TextModel, T: TextToSpeech> VoiceBackend for CpuPipelineBackend<S, M, T> {
    fn name(&self) -> &str {
        "cpu-pipeline"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            duplex: DuplexMode::HalfWithBargeIn,
            spec: self.spec,
            emits_text: true,
        }
    }

    fn start(&mut self, persona: &dyn PersonaSource) -> Result<(), BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        self.system_prompt = persona.text_prompt();
        self.voice = persona.voice_prompt();
        self.started = true;
        Ok(())
    }

    fn send_audio(&mut self, frame: &Frame) -> Result<(), BackendError> {
        self.ready()?;
        // Cap one utterance at 30 s (the Whisper window).
        if self.utterance.len() < self.spec.sample_rate as usize * 30 {
            self.utterance.extend_from_slice(&frame.samples);
        }
        Ok(())
    }

    fn end_of_user_turn(&mut self) -> Result<(), BackendError> {
        self.ready()?;
        self.queue.clear();
        self.run_turn().map_err(BackendError::Protocol)
    }

    fn interrupt(&mut self) -> Result<(), BackendError> {
        self.ready()?;
        self.queue.clear();
        Ok(())
    }

    fn poll(&mut self) -> Result<Vec<BackendEvent>, BackendError> {
        self.ready()?;
        let mut out = Vec::new();
        while let Some(ev) = self.queue.pop_front() {
            let audio = matches!(ev, BackendEvent::Audio(_));
            out.push(ev);
            if audio {
                break;
            }
        }
        Ok(out)
    }

    fn close(&mut self) {
        self.closed = true;
        self.queue.clear();
    }
}

/// Test / offline stand-ins. No network, no paid API.
pub mod mocks {
    use super::*;
    use galion_voice_core::audio::synth;

    /// Reports how much audio it heard (proves the audio reached it).
    pub struct MockStt;
    impl SpeechToText for MockStt {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
            Ok(format!("heard {} ms", pcm.len() / 16))
        }
    }

    /// Echo "brain": replies with the persona's first word and the user text.
    pub struct EchoModel;
    impl TextModel for EchoModel {
        fn reply(&mut self, system: &str, user: &str) -> Result<String, String> {
            let who = system.split_whitespace().next().unwrap_or("assistant");
            Ok(format!("{who} heard: {user}"))
        }
    }

    /// One 120 ms beep per word at 16 kHz. Licence-clean placeholder TTS.
    pub struct BeepTts;
    impl TextToSpeech for BeepTts {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn synthesize(&mut self, text: &str, _voice: &str) -> Result<Vec<i16>, String> {
            let mut out = Vec::new();
            for (i, _) in text.split_whitespace().enumerate() {
                out.extend(synth::tone(16_000, 520.0, 0.25, 1920, i * 1920));
                out.extend(std::iter::repeat_n(0i16, 640));
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mocks::*;
    use super::*;
    use galion_voice_core::audio::synth;
    use galion_voice_core::persona::StaticPersona;
    use galion_voice_core::session::{Session, SessionConfig, SessionEvent, State};

    #[test]
    fn pipeline_turn_then_barge_in_through_a_session() {
        let spec = FrameSpec::MOSHI;
        let be = CpuPipelineBackend::new(MockStt, EchoModel, BeepTts, spec);
        let mut s = Session::new(be, SessionConfig::default());
        s.start(&StaticPersona::new("Galion is a calm guide.", "v1"))
            .unwrap();
        let talk = |t: usize| Frame {
            seq: t as u64,
            samples: synth::tone(24_000, 180.0, 0.3, 1920, t * 1920),
        };
        let quiet = |t: usize| Frame {
            seq: t as u64,
            samples: synth::noise(t as u32 + 1, 0.002, 1920),
        };
        let mut texts = Vec::new();
        let mut played = 0;
        let mut barge = false;
        for t in 0..80 {
            // 1 s of speech, then silence; talk again after 3 reply frames.
            let f = if t < 12 || (played >= 3 && !barge && t < 60) {
                talk(t)
            } else {
                quiet(t)
            };
            let out = s.tick(&f).unwrap();
            for e in out.events {
                match e {
                    SessionEvent::Text(x) => texts.push(x),
                    SessionEvent::BargeIn => barge = true,
                    _ => {}
                }
            }
            if out.playout.is_some() {
                played += 1;
            }
        }
        assert!(barge, "barge-in happened");
        assert!(texts[0].starts_with("user: heard "), "{texts:?}");
        assert!(texts[1].starts_with("Galion heard: heard "), "{texts:?}");
        // Speech 12 frames + VAD hangover 6 = 18 frames * 80 ms reached STT.
        assert_eq!(texts[0], "user: heard 1440 ms");
        // Last timing is the barge-in turn (shorter utterance).
        let tm = s.backend().last_timing;
        assert!(tm.utterance_ms > 0 && tm.utterance_ms < 1440, "{tm:?}");
        assert_eq!(s.stats().barge_ins, 1);
        assert!(s.stats().user_turns >= 2);
        assert_ne!(s.state(), State::Closed);
    }
}
