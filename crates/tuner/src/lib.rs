// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Static file server for the ukulele tuner page.
//!
//! Imported from `/opt/galion/tuner` (ticket B53). The listen address and the
//! www root are config-driven. With nothing set they stay `127.0.0.1:8650` and
//! `/opt/galion/tuner/www`, the same defaults the live binary uses today.
//!
//! Precedence for each field:
//! 1. `TUNER_ADDR` or `TUNER_ROOT` when that variable is set, including empty
//! 2. `bind` or `root` in the file named by `TUNER_CONFIG`
//! 3. the defaults above
//!
//! `TUNER_CONFIG` is optional. The file format is one assignment per line,
//! `key = "value"`, with `#` comments. Only `bind` and `root` are accepted.
//! This crate does not start or stop the live tuner.

#![forbid(unsafe_code)]

mod config;
mod http;

pub use config::{resolve, ConfigError, Settings, DEFAULT_ADDR, DEFAULT_ROOT};
pub use http::{respond, FILES};
