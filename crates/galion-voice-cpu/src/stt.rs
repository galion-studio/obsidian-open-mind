// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Speech-to-text, text model and text-to-speech interfaces for the CPU pipeline.

/// Utterance-level speech recogniser (VAD hands it one finished utterance).
pub trait SpeechToText {
    /// Sample rate the recogniser wants (Whisper: 16 kHz).
    fn sample_rate(&self) -> u32;
    /// Mono f32 PCM in -1..1 at `sample_rate()`.
    fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String>;
    /// Language hint from the request (`en`, `de`, ...); ignored by
    /// English-only engines.
    fn set_language(&mut self, _language: &str) {}
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
    /// Longest text one request accepts (chars); longer input is cut.
    fn max_chars(&self) -> usize {
        300
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

/// [`sentences`], with sentences longer than `max` bytes cut again after
/// `,` or `:` (pieces of at least 20 bytes), so the first audio of a long
/// reply comes sooner.
pub fn clauses(text: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    for s in sentences(text) {
        if s.len() <= max {
            out.push(s);
            continue;
        }
        let mut start = 0;
        for (i, c) in s.char_indices() {
            let end = i + c.len_utf8();
            if matches!(c, ',' | ':') && end - start >= 20 && s.len() - end >= 20 {
                out.push(s[start..end].trim());
                start = end;
            }
        }
        out.push(s[start..].trim());
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
        let long =
            "Galion heard: and so my fellow Americans, ask not what your country can do for you.";
        assert_eq!(
            super::clauses(long, 60),
            [
                "Galion heard: and so my fellow Americans,",
                "ask not what your country can do for you."
            ]
        );
        assert_eq!(super::clauses("Short, one.", 60), ["Short, one."]);
    }
}
