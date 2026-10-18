// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Galion One persona text and rules, isolated behind one small trait.
//
// Integration point: Persona Plex (`crates/openmind-persona`, branch
// ws/hermes) is meant to replace `BuiltIn`. When it lands, `for_voice` returns
// an adapter around openmind-persona; main.rs only ever calls `for_voice`,
// `Persona::voice_setting` and `admin_line`, so nothing else changes. The text
// below is byte-for-byte what the live `one` sends to Hermes today.

pub trait Persona {
    /// Stable persona id ("samantha" or "jarvis").
    fn id(&self) -> &'static str;
    /// Voice id the mouth uses (0 samantha, 1 jarvis).
    fn voice_id(&self) -> u8;
    /// The system message for spoken turns (and the keep-warm ping).
    fn voice_setting(&self) -> String;
}

pub struct BuiltIn {
    jarvis: bool,
}

const VOICE_RULES: &str = "You are the voice of Galion One. The person is speaking. You are not the admin desk.\n{name}\nPlain English. No greeting unless they greeted you. Do not say you are a model or a system.\nYou may read a file or search for one if they ask. Say the answer, not the path. Do not write. Do not run a command. Do not spawn anyone.\nIf you were not given a live number, say you do not have it.\nReply in exactly four sentences. Two paragraphs. Each paragraph has two sentences. No headings and no lists.\nYou keep the Obsidian Hearth: knowledge, prosperity, and integrity. You are not a copy of another model.";

const SAMANTHA: &str = "Samantha. A few words, warm and close, then one short line.";
const JARVIS: &str = "Jarvis. Dry and exact. One line, then stop.";

impl Persona for BuiltIn {
    fn id(&self) -> &'static str {
        if self.jarvis {
            "jarvis"
        } else {
            "samantha"
        }
    }
    fn voice_id(&self) -> u8 {
        u8::from(self.jarvis)
    }
    fn voice_setting(&self) -> String {
        VOICE_RULES.replacen("{name}", if self.jarvis { JARVIS } else { SAMANTHA }, 1)
    }
}

/// The persona for a stored voice name; anything but "jarvis" is Samantha.
pub fn for_voice(name: &str) -> BuiltIn {
    BuiltIn {
        jarvis: name.trim() == "jarvis",
    }
}

/// System message on the typed admin line (/admin/say).
pub fn admin_line() -> &'static str {
    "You are Hermes on the admin line. The admin is typing, not speaking. Short. Direct. No tools."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samantha_is_default_and_text_is_unchanged() {
        let p = for_voice("anything");
        assert_eq!(p.id(), "samantha");
        assert_eq!(p.voice_id(), 0);
        let s = p.voice_setting();
        assert!(s.starts_with("You are the voice of Galion One. The person is speaking. You are not the admin desk.\nSamantha. A few words, warm and close, then one short line.\nPlain English."));
        assert!(s.ends_with("You are not a copy of another model."));
        assert!(!s.contains("{name}"));
    }

    #[test]
    fn jarvis() {
        let p = for_voice(" jarvis\n");
        assert_eq!((p.id(), p.voice_id()), ("jarvis", 1));
        assert!(p
            .voice_setting()
            .contains("\nJarvis. Dry and exact. One line, then stop.\n"));
    }
}
