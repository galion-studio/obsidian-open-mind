// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-nerve: the Galion stack's self-analysis (V1) and nervous system (V2).
//!
//! - [`probe`]: read-only pull probes (systemd, /proc, journald, HTTP,
//!   cloudflared metrics, df), reusable by `galion-health`.
//! - [`hub`]: POST /signal ingest for services that push (see `nerve-client`).
//! - [`state`]: per-component history and the healthy / degrading /
//!   failing-soon / failing verdict from trends.
//! - [`reflex`]: allowlisted, rate-limited auto-remediation with an audit log.
//! - [`alerts`]: journald + alerts.json (+ an existing Telegram bot if configured).
//! - [`engine`]: the loop and the self-eval report; [`server`]: loopback API.
//!
//! In Gaia terms (ws/gaia `gaia-design.md`) the nerve is the AETHER `nerve`
//! system; every component carries its Gaia function in the state.

pub mod alerts;
pub mod collect;
pub mod config;
pub mod engine;
pub mod hub;
pub mod probe;
pub mod reflex;
pub mod server;
pub mod state;
pub mod util;
