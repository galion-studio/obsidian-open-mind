// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Deterministic mock backend for tests and loopback demos.
//!
//! After each user turn it "answers" with `reply_frames` frames of a 440 Hz
//! tone, one frame per poll (real-time pacing), preceded by a text event and
//! followed by `TurnDone`. `interrupt()` drops the rest of the reply.

use crate::audio::{synth, Frame, FrameSpec};
use crate::backend::{BackendError, BackendEvent, Capabilities, DuplexMode, VoiceBackend};
use crate::persona::PersonaSource;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MockStats {
    pub frames_in: u64,
    pub turns: u64,
    pub interrupts: u64,
    pub frames_out: u64,
    pub persona_text: String,
    pub persona_voice: String,
}

#[derive(Debug)]
pub struct MockBackend {
    caps: Capabilities,
    reply_frames: usize,
    started: bool,
    closed: bool,
    queue: Vec<BackendEvent>,
    out_seq: u64,
    fail_next: bool,
    stats: MockStats,
}

impl MockBackend {
    pub fn new(spec: FrameSpec, duplex: DuplexMode, reply_frames: usize) -> Self {
        MockBackend {
            caps: Capabilities {
                duplex,
                spec,
                emits_text: true,
            },
            reply_frames,
            started: false,
            closed: false,
            queue: Vec::new(),
            out_seq: 0,
            fail_next: false,
            stats: MockStats::default(),
        }
    }

    pub fn stats(&self) -> &MockStats {
        &self.stats
    }

    /// Make the next poll report the backend as unavailable.
    pub fn fail_next_poll(&mut self) {
        self.fail_next = true;
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
}

impl VoiceBackend for MockBackend {
    fn name(&self) -> &str {
        "mock"
    }

    fn capabilities(&self) -> Capabilities {
        self.caps
    }

    fn start(&mut self, persona: &dyn PersonaSource) -> Result<(), BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        self.stats.persona_text = persona.text_prompt();
        self.stats.persona_voice = persona.voice_prompt();
        self.started = true;
        Ok(())
    }

    fn send_audio(&mut self, _frame: &Frame) -> Result<(), BackendError> {
        self.ready()?;
        self.stats.frames_in += 1;
        Ok(())
    }

    fn end_of_user_turn(&mut self) -> Result<(), BackendError> {
        self.ready()?;
        self.stats.turns += 1;
        self.queue.clear();
        self.queue
            .push(BackendEvent::Text(format!("reply {}", self.stats.turns)));
        let n = self.caps.spec.frame_samples;
        for i in 0..self.reply_frames {
            let samples = synth::tone(self.caps.spec.sample_rate, 440.0, 0.25, n, i * n);
            self.queue.push(BackendEvent::Audio(Frame {
                seq: self.out_seq,
                samples,
            }));
            self.out_seq += 1;
        }
        self.queue.push(BackendEvent::TurnDone);
        self.queue.reverse(); // pop from the end
        Ok(())
    }

    fn interrupt(&mut self) -> Result<(), BackendError> {
        self.ready()?;
        self.stats.interrupts += 1;
        self.queue.clear();
        Ok(())
    }

    fn poll(&mut self) -> Result<Vec<BackendEvent>, BackendError> {
        self.ready()?;
        if self.fail_next {
            self.fail_next = false;
            return Ok(vec![BackendEvent::Unavailable("mock failure".into())]);
        }
        let mut out = Vec::new();
        // Text and control are free; at most one audio frame per poll.
        while let Some(ev) = self.queue.pop() {
            let audio = matches!(ev, BackendEvent::Audio(_));
            if audio {
                self.stats.frames_out += 1;
            }
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
