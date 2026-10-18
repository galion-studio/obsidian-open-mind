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
    /// How to cut a reply for streaming: the pipeline synthesises and sends
    /// one chunk at a time (first audio sooner, barge-in checked between
    /// chunks). Default: the whole text in one go.
    fn chunks<'a>(&self, text: &'a str) -> Vec<&'a str> {
        vec![text]
    }
}

/// Split after `. ! ? ;` (and at a space past ~400 bytes); trimmed, no empties.
pub fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        if matches!(c, '.' | '!' | '?' | ';') || (end - start > 400 && c == ' ') {
            let s = text[start..end].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = end;
        }
    }
    let s = text[start..].trim();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sentence_split() {
        assert_eq!(
            super::sentences("Hi. How are you? Fine"),
            ["Hi.", "How are you?", "Fine"]
        );
        assert!(super::sentences("  ").is_empty());
    }
}
