// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The backend trait: the engine behind a voice session.
//!
//! Implementations: [`crate::mock::MockBackend`] (tests), the PersonaPlex GPU
//! relay (`persona-plex-relay::backend::RelayBackend`), and in M2 an on-device
//! CPU pipeline (STT -> text model -> TTS). The trait is poll-based and
//! synchronous so it works without an async runtime on every target; a
//! network backend does its I/O behind a [`Transport`]-style queue.

use std::fmt;

use crate::audio::{Frame, FrameSpec};
use crate::persona::PersonaSource;

/// How the backend handles turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplexMode {
    /// The model listens while it speaks and decides itself when to yield
    /// (PersonaPlex / moshi). The core still flushes local playout on barge-in.
    Full,
    /// Turn-based engine (STT -> LLM -> TTS): the core detects end of user
    /// speech and tells the backend; barge-in cancels generation.
    HalfWithBargeIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub duplex: DuplexMode,
    /// Audio format the backend wants in and produces out.
    pub spec: FrameSpec,
    /// True if the backend emits text (transcript / inner monologue).
    pub emits_text: bool,
}

/// What a backend hands back on `poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendEvent {
    /// A frame of model speech for playout.
    Audio(Frame),
    /// Model text (token, transcript fragment).
    Text(String),
    /// The model finished its turn (half-duplex engines).
    TurnDone,
    /// Backend lost / unavailable; the session should fall back.
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    NotStarted,
    Closed,
    Transport(String),
    Protocol(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::NotStarted => write!(f, "backend not started"),
            BackendError::Closed => write!(f, "backend closed"),
            BackendError::Transport(e) => write!(f, "transport: {e}"),
            BackendError::Protocol(e) => write!(f, "protocol: {e}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// A speech-to-speech engine.
pub trait VoiceBackend {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    /// Open the conversation with a persona.
    fn start(&mut self, persona: &dyn PersonaSource) -> Result<(), BackendError>;
    /// One microphone frame (full duplex: called every tick, speech or not).
    fn send_audio(&mut self, frame: &Frame) -> Result<(), BackendError>;
    /// End of user utterance (VAD). Full-duplex engines may ignore it.
    fn end_of_user_turn(&mut self) -> Result<(), BackendError>;
    /// Barge-in: stop generating the current reply now.
    fn interrupt(&mut self) -> Result<(), BackendError>;
    /// Drain whatever the backend has produced since the last poll.
    fn poll(&mut self) -> Result<Vec<BackendEvent>, BackendError>;
    fn close(&mut self);
}
