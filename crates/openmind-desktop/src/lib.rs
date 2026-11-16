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
//! The seven tones are sines tuned from 432 Hz. The microphone opens only
//! after an unchecked box is ticked, and only on loopback or a local file.
//! Microphone samples stay in one reused buffer. They are not saved or sent.
//! Files the operator picks are checked against a type allow-list and a size
//! cap, then uploaded on loopback. The page does not call a model.
//!
//! Printing the page opens no socket. [`listen`] binds a loopback address
//! only. Live checks of nerve, cortex, console, Hermes, voice, desk, memory and Galion One are
//! [`probe`]: read-only GETs, and only when `--probe` or `GET /probes` asks.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cli;
pub mod error;
pub mod files;
pub mod http;
pub mod page;
pub mod probe;
pub mod sound;
pub mod stack;

pub use cli::{parse, resolve_listen_addr, Mode, USAGE};
pub use error::DesktopError;
pub use files::Files;
pub use http::{listen, listen_banner, listen_with, page_hint};
pub use page::document;
pub use probe::{probe_all, probe_one, ProbeConfig, ProbeSet, Service, State, SERVICES};
pub use sound::{wav, TONES};
pub use stack::LAYERS;

/// Listen port when [`Mode::Listen`] has no address, `--port` is absent, and
/// [`PORT_ENV`] is unset.
///
/// 18081 is the staging page beside the Rust console on 18080, so this
/// desktop does not bind it.
pub const DEFAULT_PORT: u16 = 18181;

/// Default loopback address: `127.0.0.1` and [`DEFAULT_PORT`].
pub const DEFAULT_BIND: &str = "127.0.0.1:18181";

/// Environment variable that sets the listen port when `--listen` has no
/// address and `--port` is absent. Read by the binary, not by [`parse`].
pub const PORT_ENV: &str = "OPENMIND_DESKTOP_PORT";
