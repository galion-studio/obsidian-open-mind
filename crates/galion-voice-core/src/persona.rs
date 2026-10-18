// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Persona interface.
//!
//! Integration point: the `openmind-persona` crate (branch `ws/hermes`) will
//! implement [`PersonaSource`]. Until then the core depends on nothing and
//! [`StaticPersona`] carries prompts from config. PersonaPlex takes exactly
//! these two values (`text_prompt`, `voice_prompt` URL query parameters); the
//! CPU pipeline uses `text_prompt` as the LLM system prompt and `voice_prompt`
//! to pick a TTS voice.

/// Where a session gets its persona.
pub trait PersonaSource {
    /// Role / system prompt (text).
    fn text_prompt(&self) -> String;
    /// Voice identifier: a PersonaPlex voice-prompt file name or a TTS voice id.
    fn voice_prompt(&self) -> String;
}

/// A persona fixed at construction time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticPersona {
    pub text: String,
    pub voice: String,
}

impl StaticPersona {
    pub fn new(text: impl Into<String>, voice: impl Into<String>) -> Self {
        StaticPersona {
            text: text.into(),
            voice: voice.into(),
        }
    }
}

impl PersonaSource for StaticPersona {
    fn text_prompt(&self) -> String {
        self.text.clone()
    }
    fn voice_prompt(&self) -> String {
        self.voice.clone()
    }
}
