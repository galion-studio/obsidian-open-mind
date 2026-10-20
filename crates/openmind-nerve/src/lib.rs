// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

#![forbid(unsafe_code)]

//! openmind-nerve: the Obsidian Open Mind nervous system and cortex.
//!
//! Live as `galion-nerve.service` on 127.0.0.1:8670. One crate, one binary
//! (`openmind-nerve`), std + serde_json + zagreb-time.
//!
//! - [`writer`]: append-only JSONL nerve log (re-exported at the crate root).
//! - [`probe`]: read-only pull probes (systemd, /proc, journald, HTTP,
//!   cloudflared metrics, df, `ss -ltn`).
//! - [`logscan`]: journald text insights (zerolog `ERR`, refused origin
//!   dials, scanner paths).
//! - [`access`]: access-log tailing and the request-rate anomaly (DDoS)
//!   detector.
//! - [`hub`]: POST /signal ingest for services that push (see `nerve-client`).
//! - [`state`]: per-component history and trend verdicts.
//! - [`incident`]: correlation: several failing signals become one incident.
//! - [`cortex`]: vulnerabilities, where the stack is "bleeding", DDoS: the
//!   contract ([`cortex::Input`], [`cortex::Finding`] as B33/B41 events).
//! - [`sensors`]: the cortex detectors, one file per [`sensors::Sensor`].
//! - [`score`]: the explained self-eval score.
//! - [`reflex`]: allowlisted, rate-limited auto-remediation with an audit log.
//! - [`alerts`]: journald + alerts.json.
//! - [`feed`]: the cortex B33 event feed (`GET /api/v1/cortex/events`).
//! - [`engine`]: the loop; [`server`]: the loopback JSON API and dashboard;
//!   [`replay`]: run captured live data through the same pipeline.

pub mod access;
pub mod alerts;
pub mod collect;
pub mod config;
pub mod cortex;
pub mod engine;
pub mod feed;
pub mod hub;
pub mod incident;
pub mod logscan;
pub mod probe;
pub mod reflex;
pub mod replay;
pub mod score;
pub mod sensors;
pub mod server;
pub mod state;
pub mod util;
pub mod writer;

pub use writer::*;
