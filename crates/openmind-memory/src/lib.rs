// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

//! Fixture-backed v0 of the memory routes Hermes actually calls.
//!
//! Read from the memory-tencentdb Hermes plugin (not from the Hermes home):
//! v1 store is `POST /capture`, v1 search is `POST /search/memories` and
//! `POST /search/conversations`, and v1 probes `GET /health`. v2 store is
//! `POST /v2/conversation/add`. v2 search is `POST /v2/atomic/search` and
//! `POST /v2/conversation/search`.
//!
//! There is no embedding model and no extractor. A stored turn is indexed
//! with FTS5 and searched by ASCII tokens. `embeddingService` stays false.
//! Timestamps come from `zagreb-time` (numeric offset, never `Z`).
//!
//! The live gateway on `127.0.0.1:8420` is never dialed and never bound.
//! A socket, when opened, is `127.0.0.1` and port 0 unless the caller passed
//! another loopback port that is not 8420.

pub mod http;
pub mod index;

pub use http::{parse_bind, Server};
pub use index::{CaptureOut, Hit, Index, Kind, PutRequest, SearchRequest, Stored};
