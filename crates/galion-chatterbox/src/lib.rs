// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Chatterbox text-to-speech in pure Rust (candle, CPU).
//!
//! A re-implementation of the inference path of Resemble AI's Chatterbox
//! (<https://github.com/resemble-ai/chatterbox>, MIT; weights
//! <https://huggingface.co/ResembleAI/chatterbox>, MIT), which itself builds on
//! CosyVoice (Apache-2.0) and S3Tokenizer (Apache-2.0). Written from the
//! published architecture; no Python and no code copied.
//!
//! Pipeline: text -> [`text::EnTokenizer`] -> [`t3::T3`] (0.5 B Llama, CFG,
//! speech tokens at 25 Hz) -> [`flow::Flow`] (conformer encoder + conditional
//! flow matching, 80-bin mel at 50 Hz) -> [`hift::HiFT`] (NSF + iSTFT vocoder)
//! -> 24 kHz mono audio. Voice conditioning comes either from the built-in
//! `conds.pt` or from a reference clip ([`conds::Conditionals::from_wav`]:
//! [`ve::VoiceEncoder`], [`s3tok::S3Tokenizer`], [`campplus::CamPlus`]).
//!
//! Not implemented: Resemble's Perth neural watermark (applied after
//! generation in the Python package). See docs/voice-rust.md.

pub mod campplus;
pub mod conds;
pub mod dsp;
pub mod flow;
pub mod hift;
pub mod model;
pub mod nnx;
pub mod rng;
pub mod s3tok;
pub mod t3;
pub mod text;
pub mod ve;
pub mod wav;

pub use model::{Chatterbox, GenerateOptions};

/// Output sample rate of S3Gen / HiFT.
pub const S3GEN_SR: u32 = 24_000;
/// Input rate of the S3 tokenizer, voice encoder and CAMPPlus.
pub const S3_SR: u32 = 16_000;
/// Number of real speech tokens (3^8); ids at or above are special.
pub const SPEECH_VOCAB: u32 = 6561;

pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}
