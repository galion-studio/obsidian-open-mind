// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! galion-stt: live speech-to-text over a loopback WebSocket for the Hermes
//! research page dictation box.
//!
//! - `GET /health`: JSON status.
//! - `WS /stream`: client sends 16 kHz mono PCM16LE binary frames (20-100 ms)
//!   and a final text `{"type":"stop"}`; server sends `partial` / `final` /
//!   `error` JSON (see [`protocol`]).
//!
//! Engine: Moonshine Tiny Streaming (English, MIT code and weights) through
//! the official libmoonshine C API, opened at run time ([`ffi`]). The heavy
//! transcribe call runs on one dedicated thread per session behind a bounded
//! drop-oldest queue ([`session`]); sessions are capped by a fixed pool of
//! preloaded transcribers ([`engine::EnginePool`]).
//!
//! Privacy: audio and transcripts stay in memory; logs carry counts and
//! timings only; every model and library file is sha256-pinned ([`models`]).

pub mod audio;
pub mod engine;
pub mod ffi;
pub mod models;
pub mod protocol;
pub mod server;
pub mod session;
