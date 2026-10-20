// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Galion voice M2: on-device CPU pipeline.
//!
//! * [`stt`] - `SpeechToText`, `TextModel`, `TextToSpeech` traits.
//! * [`pipeline`] - `CpuPipelineBackend`: STT -> text model -> TTS as a
//!   `galion-voice-core` backend (turn-based with barge-in).
//! * [`wav`] - minimal PCM WAV reader, [`resample`] - linear resampler.
//! * `whisper_stt` (feature `whisper`) - Whisper tiny via candle, CPU only.
#![deny(unsafe_code)]

pub mod g2p;
#[cfg(feature = "kitten")]
pub mod kitten;
pub mod pipeline;
pub mod resample;
pub mod stt;
pub mod wav;

#[cfg(feature = "whisper")]
pub mod whisper;
#[cfg(feature = "whisper")]
pub mod whisper_stt;
