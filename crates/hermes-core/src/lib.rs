// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! # hermes-core
//!
//! Hermes is the boss agent of the Galion swarm: "the one agent the operator
//! hears. The others are yours. You send them." (`stack/hermes/SOUL.md`).
//! This crate is the coordinator Hermes runs above the seven swarm sectors
//! (gaia, nerve, cortex, vps-ops, integration-ci, security, docs-whitepaper)
//! plus the bench sector, which coordinates benchmark runs:
//!
//! * [`registry`]: sectors, leads and Gaia mapping from `config/hermes/sectors.toml`;
//! * [`plan`]: the sector plans (`/workspace/swarm/plans/*.md`) as tasks with deps;
//! * [`engine`]: intake, dispatch, retry loop, states rebuilt from the ledger;
//! * [`governor`]: the concurrency cap (default 16; measured limit 18-20);
//! * [`retry`]: request-limit and stall retries with capped backoff;
//! * [`gate`]: HADES approvals (Ed25519, single-use, scoped) for merge, deploy
//!   and destructive work;
//! * [`ledger`]: the hash-chained JSONL ledger and audit log, Zagreb time;
//! * [`provider`]: model calls behind a trait; OpenRouter refuses (402), the
//!   offline mock runs;
//! * [`readiness`]: the gate before merge / connect / replace-infrastructure;
//! * [`bench`]: the bench sector: the built-in benchmark run plan and the
//!   coordinator that runs `hermes-bench` through the governor and ledger;
//! * [`report`]: one spoken sentence plus detail;
//! * [`http`]: the loopback status API.
//!
//! Design: `docs/hermes-core.md`. Nothing here deploys, merges, or calls a
//! paid API.

pub mod action;
pub mod bench;
pub mod engine;
pub mod gate;
pub mod governor;
pub mod http;
pub mod ledger;
pub mod plan;
pub mod provider;
pub mod readiness;
pub mod registry;
pub mod report;
pub mod retry;
