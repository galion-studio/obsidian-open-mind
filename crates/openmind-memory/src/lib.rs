// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
//! openmind-memory: API-compatible replacement for the endpoints Hermes uses
//! on the TencentDB `tdai` memory gateway. See docs/ws-memory.md.

pub mod api;
pub mod config;
pub mod engine;
pub mod fts;
pub mod hash;
pub mod http;
pub mod log;
pub mod offload;
pub mod store;
pub mod text;
pub mod util;
