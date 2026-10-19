// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

//! Fixture-backed memory routes the selected Hermes provider calls.
//!
//! v1 is `GET /health`, `POST /capture`, `POST /recall`, `POST /session/end`,
//! `POST /search/memories`, and `POST /search/conversations`. Offload, which
//! uses the same bearer plus `x-tdai-service-id`, is `POST /v2/offload/ingest`,
//! `POST /v2/offload/query-mmd`, and `POST /v2/offload/read-ref`. The spare
//! server also keeps the unselected v2 store and search routes.
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
