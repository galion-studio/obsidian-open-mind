// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Devices for the Galion console `/instances` page.
//!
//! - [`code`]: one-time pairing codes and device tokens (hashed at rest);
//! - [`fleet`]: the device list, joins, reports, rename and remove;
//! - [`metrics`]: CPU, RAM and GPU/VRAM readings for Linux, macOS, Windows;
//! - [`lan`]: finding the console on the local network over UDP broadcast;
//! - [`agent`]: the `galion-join` program a laptop runs.
//!
//! Pure Rust (std + serde_json + galion-common), no unsafe.

#![forbid(unsafe_code)]

pub mod agent;
pub mod code;
pub mod fleet;
pub mod lan;
pub mod metrics;

pub use fleet::{Fleet, Joined, Refused};
pub use metrics::{Gpu, Metrics};

/// Owner-only JSON: the device list.
pub const API_LIST: &str = "/api/v1/fleet";
/// Device: join with a code.
pub const API_JOIN: &str = "/api/v1/fleet/join";
/// Device: report a reading with its token.
pub const API_BEAT: &str = "/api/v1/fleet/beat";
