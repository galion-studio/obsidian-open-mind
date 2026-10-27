// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Clean-room MIT implementation of the safetensors container format.
//!
//! Format (public spec): 8-byte little-endian `u64` header length `N`, then `N` bytes of UTF-8 JSON
//! (`{"__metadata__": {str: str}?, "<name>": {"dtype", "shape", "data_offsets": [begin, end]}, ...}`,
//! may be right-padded with spaces), then the raw byte buffer. Offsets are relative to the buffer.
//!
//! The public API mirrors the subset of `safetensors` 0.7 that candle-core / candle-nn 0.9.2 call, so the
//! crate can replace it through `[patch.crates-io]`. Written from the format description only; no
//! upstream source was copied.
#![forbid(unsafe_code)]

pub mod slice;
pub mod tensor;

pub use tensor::serialize_to_file;
pub use tensor::{serialize, Dtype, SafeTensorError, SafeTensors, View};
