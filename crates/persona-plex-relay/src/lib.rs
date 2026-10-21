// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Persona Plex v2 relay - scaffold.
//!
//! The VPS side of the Galion voice-to-voice channel: client (browser,
//! WebSocket, Opus) -> this relay (auth, session, speaker-verification policy,
//! no inference) -> GPU host running the PersonaPlex moshi server.
//!
//! This scaffold defines the protocol, config and verification logic only. It
//! opens no sockets and runs no model. Design:
//! `docs/persona-plex-v2-design.md`.

pub mod backend;
pub mod config;
#[cfg(feature = "opus")]
pub mod opus_ogg;
pub mod plan;
pub mod protocol;
pub mod speaker;
