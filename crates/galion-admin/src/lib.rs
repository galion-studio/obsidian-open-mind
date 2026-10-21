// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! galion-admin: Galion OS L1 admin CLI skeleton (device-independent; Linux and Android).
//!
//! Commands: sandbox start/stop/status/probe (through the `galion-box` launcher), tunnel status
//! (loopback reachability only), audit tail/verify (hash-chained local log). No `su`, no root:
//! the binary refuses to run as uid 0.

pub mod audit;
pub mod cli;
pub mod image;
pub mod json;
pub mod sandbox;
pub mod sha256;
pub mod timefmt;
pub mod tunnel;
