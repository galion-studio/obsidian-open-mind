// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! # hermes-bench: the three benchmark tool sets as hermes-rs tools
//!
//! Workstream H of `plans/benchmarks.md`. [`adapters`] exposes the siblings'
//! exact tool surfaces as `hermes_rs::tools::Tool`s (B1 banking: 12, B2 Wall
//! Street: 14 names of which 13 are model-facing, `ws_kill_reset` being an
//! operator-only function; B3 vending: 11). [`runner`] runs a benchmark with
//! a pluggable agent through the real hermes-rs agent loop:
//!
//! * `scripted`: deterministic scripts as the model (in-process provider);
//!   reproduces each sibling's reference result, proving the adapters faithful;
//! * `mock-llm`: the same scripts behind a loopback OpenAI-compatible server,
//!   driven by hermes-rs's real curl provider (the live wire path, zero spend);
//! * `live`: OpenRouter, only with `GALION_BENCH_LIVE_MODEL=1` and the key in
//!   the environment (never read or printed here), spend-capped.
//!
//! [`gateway`] wires the same registries into hermes-gateway behind
//! `HERMES_RS_BENCH=1` (decision H4 option b; gateway `bench` feature).
//!
//! Everything runs against local mocks / paper / simulation. No tool can move
//! money: payments, card changes and disputes are drafts; orders are paper;
//! supplier orders are simulated cents. See `docs/bench/hermes-bench.md`.

pub mod adapters;
pub mod banking;
pub mod e2e;
pub mod gateway;
pub mod live;
pub mod mockllm;
pub mod planner;
pub mod runner;
pub mod session;
pub mod transport;
pub mod vending;
pub mod wallstreet;
