// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The legacy GPU mouth (`tts_server.py`, Chatterbox) as a CPU
//! [`TextToSpeech`] engine on `galion-chatterbox` (pure Rust, candle).
//!
//! Voices follow the legacy table: `samantha` and `jarvis` map to reference
//! clips `<voices>/samantha.wav` and `<voices>/jarvis.wav`. A voice whose clip
//! is missing is generated with Chatterbox's built-in voice (`conds.pt`),
//! exactly as `model.generate(chunk)` did without `audio_prompt_path`.
//! Reference clips are analysed once at load time (the legacy server
//! re-analysed them on every call; the result is the same).

use std::path::{Path, PathBuf};
use std::time::Instant;

use galion_chatterbox::conds::Conditionals;
use galion_chatterbox::{Chatterbox, GenerateOptions};

use crate::stt::TextToSpeech;

/// The legacy voice table, in order; the first is the default.
pub const LEGACY_VOICES: [&str; 2] = ["samantha", "jarvis"];

/// Engine settings beyond the model directory.
#[derive(Clone, Debug)]
pub struct ChatterboxConfig {
    /// Directory holding `samantha.wav` / `jarvis.wav` (optional).
    pub voices_dir: Option<PathBuf>,
    pub options: GenerateOptions,
    /// Cap on the S3Gen reference prompt in seconds (reference: 10).
    pub prompt_seconds: f32,
}

impl Default for ChatterboxConfig {
    fn default() -> Self {
        ChatterboxConfig {
            voices_dir: None,
            options: GenerateOptions::default(),
            prompt_seconds: 10.0,
        }
    }
}

struct Voice {
    name: String,
    /// True when conditioned on its own reference clip.
    from_clip: bool,
    conds: Conditionals,
}

pub struct ChatterboxTts {
    model: Chatterbox,
    voices: Vec<Voice>,
    pub options: GenerateOptions,
    /// Load notes for logs and /health (which clip, timings).
    pub notes: Vec<String>,
}

/// Legacy `chunks()`: whitespace collapsed, at most 1200 chars, one chunk up
/// to 300 chars, else split after `.`/`!`/`?` + whitespace and pack greedily
/// (a new chunk starts when the current one has at least 50 chars and the
/// next sentence would take it past 200). Lengths count chars, as Python
/// does. The server collapses whitespace and applies the 1200 cap
/// ([`TextToSpeech::max_chars`]) before calling this, so the chunks are
/// slices of the input.
pub fn legacy_chunks(text: &str) -> Vec<&str> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let chars = |s: &str| s.chars().count();
    if chars(text) <= 300 {
        return vec![text];
    }
    // Sentence ranges: a boundary is whitespace right after . ! or ?.
    let mut parts: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    let mut prev = '\0';
    let mut in_gap = false;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if !in_gap && matches!(prev, '.' | '!' | '?') {
                parts.push((start, i));
                in_gap = true;
            }
        } else if in_gap {
            start = i;
            in_gap = false;
        }
        prev = c;
    }
    parts.push((start, text.len()));
    let mut out = Vec::new();
    let mut cur: Option<(usize, usize)> = None;
    for (s, e) in parts {
        cur = match cur {
            Some((cs, ce)) => {
                let (lc, lp) = (chars(&text[cs..ce]), chars(&text[s..e]));
                if lc + 1 + lp > 200 && lc >= 50 {
                    out.push(&text[cs..ce]);
                    Some((s, e))
                } else {
                    Some((cs, e))
                }
            }
            None => Some((s, e)),
        };
    }
    if let Some((cs, ce)) = cur {
        out.push(&text[cs..ce]);
    }
    out
}

/// Float audio to 16-bit PCM as the legacy `to_pcm`: scale by 32767, clamp,
/// truncate toward zero (torch `.to(int16)`).
pub fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

impl ChatterboxTts {
    pub fn load(dir: &Path, cfg: &ChatterboxConfig) -> Result<Self, String> {
        let t = Instant::now();
        let model = Chatterbox::load(dir)?;
        let mut notes = vec![format!("model loaded in {} ms", t.elapsed().as_millis())];
        let mut voices = Vec::new();
        for name in LEGACY_VOICES {
            let clip = cfg
                .voices_dir
                .as_ref()
                .map(|d| d.join(format!("{name}.wav")));
            let (conds, from_clip) = match clip.filter(|p| p.is_file()) {
                Some(p) => {
                    let t = Instant::now();
                    let c = model.conds_from_wav_file(&p, cfg.options.exaggeration)?;
                    notes.push(format!(
                        "{name}: reference {} ({} ms)",
                        p.display(),
                        t.elapsed().as_millis()
                    ));
                    (c, true)
                }
                None => {
                    notes.push(format!("{name}: no reference clip, built-in voice"));
                    (model.builtin.clone(), false)
                }
            };
            voices.push(Voice {
                name: name.to_string(),
                from_clip,
                conds: conds.with_gen_prompt_limit(cfg.prompt_seconds),
            });
        }
        Ok(ChatterboxTts {
            model,
            voices,
            options: cfg.options.clone(),
            notes,
        })
    }

    /// Voices with their own reference clip (the legacy `/health` list).
    pub fn ready_voices(&self) -> Vec<String> {
        self.voices
            .iter()
            .filter(|v| v.from_clip)
            .map(|v| v.name.clone())
            .collect()
    }

    /// Generate float audio for one chunk with explicit options.
    pub fn generate(
        &self,
        text: &str,
        voice: &str,
        o: &GenerateOptions,
    ) -> Result<Vec<f32>, String> {
        let v = self
            .voices
            .iter()
            .find(|v| v.name == voice)
            .unwrap_or(&self.voices[0]);
        let (wav, tm) = self.model.generate(text, &v.conds, o)?;
        eprintln!(
            "chatterbox: {} chars -> {} tokens, {:.2} s audio (t3 {} ms, flow {} ms, hift {} ms)",
            text.chars().count(),
            tm.speech_tokens,
            wav.len() as f32 / 24_000.0,
            tm.t3_ms,
            tm.flow_ms,
            tm.hift_ms
        );
        Ok(wav)
    }
}

impl TextToSpeech for ChatterboxTts {
    fn sample_rate(&self) -> u32 {
        24_000
    }
    fn synthesize(&mut self, text: &str, voice: &str) -> Result<Vec<i16>, String> {
        let o = self.options.clone();
        Ok(to_pcm16(&self.generate(text, voice, &o)?))
    }
    fn chunks<'a>(&self, text: &'a str) -> Vec<&'a str> {
        legacy_chunks(text)
    }
    fn max_chars(&self) -> usize {
        1200
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_match_legacy_rules() {
        assert!(legacy_chunks("   ").is_empty());
        assert_eq!(legacy_chunks("  hi there "), ["hi there"]);
        let s = "This sentence is exactly long enough to matter here. ".repeat(8);
        let c = legacy_chunks(&s);
        assert!(c.len() > 1);
        for x in &c {
            assert!(x.chars().count() <= 200 || !x.contains(". "), "{x}");
            assert!(x.chars().count() >= 50);
        }
        assert_eq!(c.join(" "), s.trim());
        // 1200-char cap.
        // No sentence ends: one chunk (the 1200 cap is applied by the server).
        let long = "a".repeat(1200);
        assert_eq!(legacy_chunks(&long), [long.as_str()]);
        // Multibyte text counts chars, not bytes.
        let e = "é".repeat(300);
        assert_eq!(legacy_chunks(&e).len(), 1);
    }

    #[test]
    fn pcm_truncates_like_torch() {
        assert_eq!(
            to_pcm16(&[0.0, 1.0, -1.0, 2.0, 0.5, -0.5]),
            [0, 32767, -32767, 32767, 16383, -16383]
        );
    }
}
