// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Full-duplex voice session state machine with barge-in.
//!
//! Driven by one [`Session::tick`] per audio frame (e.g. every 80 ms at the
//! moshi frame size): the microphone frame goes through the VAD and to the
//! backend, the backend is polled, model audio goes into the jitter buffer,
//! and one playout frame comes out. Both directions flow on every tick.
//!
//! Barge-in: if the user starts speaking while the model is speaking (or has
//! speech queued), the jitter buffer is flushed at once and the backend is
//! told to `interrupt()`.

use crate::audio::Frame;
use crate::backend::{BackendError, BackendEvent, DuplexMode, VoiceBackend};
use crate::jitter::{JitterBuffer, JitterStats};
use crate::persona::PersonaSource;
use crate::vad::{EnergyVad, VadConfig, VadEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Created, backend not started.
    Idle,
    /// Nobody speaking.
    Listening,
    /// VAD says the user is speaking.
    UserSpeaking,
    /// User finished; waiting for the first model frame.
    AwaitingReply,
    /// Model audio is playing.
    Speaking,
    /// Backend lost; caller should switch to text mode or another backend.
    Fallback,
    Closed,
}

/// Things a tick reports to the caller (UI, logs, tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    State(State),
    UserSpeechStart,
    UserSpeechEnd,
    BargeIn,
    Text(String),
    BackendUnavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionConfig {
    pub vad: VadConfig,
    /// Playout prebuffer in frames.
    pub jitter_target: usize,
    /// Playout cap in frames.
    pub jitter_max: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            vad: VadConfig::default(),
            jitter_target: 2,
            jitter_max: 25,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SessionStats {
    pub ticks: u64,
    pub frames_up: u64,
    pub frames_down: u64,
    pub barge_ins: u64,
    pub user_turns: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickOutput {
    /// Frame to play now (`None` = play silence / nothing queued).
    pub playout: Option<Frame>,
    pub events: Vec<SessionEvent>,
}

pub struct Session<B: VoiceBackend> {
    backend: B,
    vad: EnergyVad,
    jitter: JitterBuffer,
    state: State,
    turn_done: bool,
    stats: SessionStats,
}

impl<B: VoiceBackend> Session<B> {
    pub fn new(backend: B, cfg: SessionConfig) -> Self {
        let n = backend.capabilities().spec.frame_samples;
        Session {
            backend,
            vad: EnergyVad::new(cfg.vad),
            jitter: JitterBuffer::new(n, cfg.jitter_target, cfg.jitter_max),
            state: State::Idle,
            turn_done: false,
            stats: SessionStats::default(),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }
    pub fn stats(&self) -> SessionStats {
        self.stats
    }
    pub fn jitter_stats(&self) -> JitterStats {
        self.jitter.stats()
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    fn set(&mut self, s: State, out: &mut TickOutput) {
        if self.state != s {
            self.state = s;
            out.events.push(SessionEvent::State(s));
        }
    }

    pub fn start(&mut self, persona: &dyn PersonaSource) -> Result<TickOutput, BackendError> {
        let mut out = TickOutput::default();
        if self.state != State::Idle {
            return Ok(out);
        }
        self.backend.start(persona)?;
        self.set(State::Listening, &mut out);
        Ok(out)
    }

    /// Advance one frame: `mic` in, at most one playout frame out.
    pub fn tick(&mut self, mic: &Frame) -> Result<TickOutput, BackendError> {
        let mut out = TickOutput::default();
        match self.state {
            State::Idle => return Err(BackendError::NotStarted),
            State::Closed => return Err(BackendError::Closed),
            State::Fallback => return Ok(out),
            _ => {}
        }
        self.stats.ticks += 1;
        let duplex = self.backend.capabilities().duplex;

        // 1. Uplink: VAD, barge-in, send.
        let vad_ev = self.vad.process(mic);
        match vad_ev {
            Some(VadEvent::SpeechStart) => {
                out.events.push(SessionEvent::UserSpeechStart);
                if self.state == State::Speaking || self.jitter.depth() > 0 {
                    self.jitter.flush();
                    self.backend.interrupt()?;
                    self.stats.barge_ins += 1;
                    out.events.push(SessionEvent::BargeIn);
                }
                self.turn_done = false;
                self.set(State::UserSpeaking, &mut out);
            }
            Some(VadEvent::SpeechEnd) => {
                out.events.push(SessionEvent::UserSpeechEnd);
                self.stats.user_turns += 1;
                self.backend.end_of_user_turn()?;
                self.set(State::AwaitingReply, &mut out);
            }
            None => {}
        }
        let send = match duplex {
            DuplexMode::Full => true,
            DuplexMode::HalfWithBargeIn => {
                self.vad.is_speaking() || vad_ev == Some(VadEvent::SpeechEnd)
            }
        };
        if send {
            self.backend.send_audio(mic)?;
            self.stats.frames_up += 1;
        }

        // 2. Downlink: poll backend.
        for ev in self.backend.poll()? {
            match ev {
                BackendEvent::Audio(f) => {
                    // While the user holds the floor, model audio is not queued.
                    if self.state != State::UserSpeaking {
                        self.jitter.push(f);
                    }
                }
                BackendEvent::Text(t) => out.events.push(SessionEvent::Text(t)),
                BackendEvent::TurnDone => self.turn_done = true,
                BackendEvent::Unavailable(why) => {
                    self.jitter.flush();
                    out.events.push(SessionEvent::BackendUnavailable(why));
                    self.set(State::Fallback, &mut out);
                    return Ok(out);
                }
            }
        }

        // 3. Playout.
        out.playout = self.jitter.pop();
        if out.playout.is_some() {
            self.stats.frames_down += 1;
            if matches!(self.state, State::Listening | State::AwaitingReply) {
                self.set(State::Speaking, &mut out);
            }
        } else if self.jitter.depth() == 0
            && match self.state {
                // Turn-based engines say when the reply is over.
                State::AwaitingReply => self.turn_done,
                // Full-duplex engines never do: playout running dry ends it.
                State::Speaking => self.turn_done || duplex == DuplexMode::Full,
                _ => false,
            }
        {
            self.turn_done = false;
            self.set(State::Listening, &mut out);
        }
        Ok(out)
    }

    /// Backend came back (or a different one was swapped in): resume.
    pub fn resume(&mut self) -> TickOutput {
        let mut out = TickOutput::default();
        if self.state == State::Fallback {
            self.vad.reset();
            self.set(State::Listening, &mut out);
        }
        out
    }

    pub fn close(&mut self) -> TickOutput {
        let mut out = TickOutput::default();
        self.backend.close();
        self.jitter.flush();
        self.set(State::Closed, &mut out);
        out
    }
}
