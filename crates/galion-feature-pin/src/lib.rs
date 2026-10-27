// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! galion-feature-pin: no code, only `[dependencies]` with pinned features.
//!
//! Cargo unifies dependency features over the packages selected for one
//! build. A release binary built alone (`cargo build -p hermes-gateway`) and
//! the same binary built inside the 12-package release set therefore saw
//! different feature sets for indexmap and serde_core and came out with
//! different bytes (D-312-4, slot 312). Release binaries whose graph reaches
//! serde_json depend on this crate, which turns on the union features, so
//! both builds compile the exact same units.
//!
//! The guard is `tests/release_feature_unify.rs`: for x86_64 and aarch64
//! musl it compares `cargo tree` of every release binary alone with the full
//! release set and fails on any package whose features differ.

#![forbid(unsafe_code)]
#![no_std]
