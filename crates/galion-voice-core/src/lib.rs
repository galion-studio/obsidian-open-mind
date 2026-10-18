// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Galion voice core (M1).
//!
//! Portable building blocks for real-time speech-to-speech, the same code on a
//! phone, a Pi or the VPS:
//!
//! * [`audio`] - PCM frames and a framer that cuts arbitrary chunks into fixed frames.
//! * [`vad`] - energy-based voice activity detection with an adaptive noise floor.
//! * [`jitter`] - reordering jitter buffer with loss concealment (silence).
//! * [`backend`] - the [`backend::VoiceBackend`] trait every engine implements.
//! * [`persona`] - the small persona interface (text prompt + voice prompt).
//! * [`session`] - the full-duplex session state machine with barge-in.
//! * [`mock`] - a deterministic mock backend for tests.
//!
//! No codec lives here: frames are PCM i16. Opus is a backend/transport
//! concern (see `docs/ws-voice.md`). std only; `#![forbid(unsafe_code)]`.
#![forbid(unsafe_code)]

pub mod audio;
pub mod backend;
pub mod jitter;
pub mod mock;
pub mod persona;
pub mod session;
pub mod vad;
