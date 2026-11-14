// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Local control console for the Obsidian Open Mind desktop.
//!
//! The page is the stack's command surface: a stack list, Open Mind as the
//! presence, and one prompt. The plus on the left opens the operating
//! system's file picker. The voice button is on the right. Nothing on the
//! page is a home screen built around a circle.
//!
//! The seven tones are sines tuned from 432 Hz. The page keeps commands and
//! file names in the browser. It does not read file bytes or call a model.
//! The microphone opens only after an unchecked box is ticked, and only on
//! loopback or a local file. Samples stay in one reused buffer. They are not
//! saved, uploaded, or committed.
//!
//! Printing the page opens no socket. [`listen`] binds a loopback address
//! only. Live checks of nerve, cortex, console, Hermes and voice are
//! [`probe`]: read-only GETs, and only when `--probe` or `GET /probes` asks.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod error;
pub mod http;
pub mod page;
pub mod probe;
pub mod sound;
pub mod stack;

pub use cli::{parse, Mode, USAGE};
pub use error::DesktopError;
pub use http::listen;
pub use page::document;
pub use probe::{probe_all, probe_one, ProbeConfig, ProbeSet, Service, State, SERVICES};
pub use sound::{wav, TONES};
pub use stack::LAYERS;

/// Default loopback address for [`Mode::Listen`] with no address argument.
/// Chosen to match the desktop port named in the Galion OS notes (18081).
pub const DEFAULT_BIND: &str = "127.0.0.1:18081";
