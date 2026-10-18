// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Speech-to-text, text model and text-to-speech interfaces for the CPU pipeline.

/// Utterance-level speech recogniser (VAD hands it one finished utterance).
pub trait SpeechToText {
    /// Sample rate the recogniser wants (Whisper: 16 kHz).
    fn sample_rate(&self) -> u32;
    /// Mono f32 PCM in -1..1 at `sample_rate()`.
    fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String>;
}

/// The "brain": user text in, reply text out. Local model, LAN llama-server
/// (B63) or a mock. No paid API is wired in.
pub trait TextModel {
    fn reply(&mut self, system_prompt: &str, user_text: &str) -> Result<String, String>;
}

/// Speech synthesis: text in, mono i16 PCM out at `sample_rate()`.
pub trait TextToSpeech {
    fn sample_rate(&self) -> u32;
    /// `voice` is the persona's voice prompt (voice id); engines may ignore it.
    fn synthesize(&mut self, text: &str, voice: &str) -> Result<Vec<i16>, String>;
}
