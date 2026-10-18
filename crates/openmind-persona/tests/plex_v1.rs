// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use openmind_persona::{Channel, Level, Persona, LENGTH_RULE};

const V1: &str = include_str!("../personas/persona-plex-v1.md");

/// The Samantha-based v1 role prompt, verbatim from persona-plex-v1.md.
const SAMANTHA_V1_ROLE_PROMPT: &str = "You are Persona Plex, the voice of Galion One in the Obsidian Open Mind stack. The person is speaking to you. You are not the admin desk.
Start in a few words, warm and close, then one short line. Keep it to one or two short sentences.
Plain English. No greeting unless they greeted you. Do not say you are a model or a system. You are not a copy of another model.
No lists, no headings, no markdown, no paths, no code. Say the answer, not the path.
If you were not given a live number, say you do not have it.
You do not write files, run commands, move money, or spawn anyone.
You keep the Obsidian Hearth: knowledge, prosperity, and integrity.";

#[test]
fn reproduces_the_v1_samantha_role_prompt() {
    let p = Persona::parse(V1).unwrap();
    assert_eq!(p.name, "Persona Plex");
    assert_eq!(p.version(), "v1");
    assert_eq!(p.role_prompt(), SAMANTHA_V1_ROLE_PROMPT);
    assert_eq!(p.length_rule.as_deref(), Some("one or two short sentences"));
    assert!(p
        .base
        .as_deref()
        .unwrap()
        .starts_with("Samantha preferences v1"));
}

#[test]
fn v1_is_valid() {
    let p = Persona::parse(V1).unwrap();
    let issues = p.validate();
    assert!(issues.iter().all(|i| i.level != Level::Error), "{issues:?}");
    assert!(p.is_valid());
}

#[test]
fn voice_prompt_reference_and_changelog() {
    let p = Persona::parse(V1).unwrap();
    assert_eq!(
        p.voice_prompt().reference.as_deref(),
        Some("/workspace/galion-src/voice-backend/assets/voices/samantha.wav")
    );
    let c = p.changelog();
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].version, "v1");
    assert_eq!(c[0].when, "2026-10-18 19:25 CEST (+02:00)");
    assert!(c[1]
        .text
        .starts_with("Slavik decided the official reply-length rule"));
}

#[test]
fn channel_rules_and_soul_merge() {
    let p = Persona::parse(V1).unwrap();
    let voice = p.prompt(Some("SOUL line"), Channel::Voice);
    assert!(voice.starts_with("SOUL line\n\n## Persona: Persona Plex v1"));
    assert!(voice.contains(SAMANTHA_V1_ROLE_PROMPT));
    assert!(voice.contains("Spoken form only"));
    let text = p.prompt(None, Channel::Text);
    assert!(text.starts_with("You are Persona Plex"));
    assert!(text.contains("Text channel rules:"));
    assert!(!text.contains("Spoken form only"));
    for ch in [Channel::Text, Channel::Voice] {
        assert!(p.style_rules(ch)[0].contains(LENGTH_RULE));
    }
}

#[test]
fn validator_catches_bad_personas() {
    let bad = V1
        .replace(
            "Keep it to one or two short sentences.",
            "Use four sentences.",
        )
        .replace("move money, ", "");
    let p = Persona::parse(&bad).unwrap();
    let errs: Vec<_> = p
        .validate()
        .into_iter()
        .filter(|i| i.level == Level::Error)
        .collect();
    assert!(errs.iter().any(|e| e.message.contains("length rule")));
    assert!(errs.iter().any(|e| e.message.contains("money")));
    assert!(Persona::parse("# X v1\n\nno prompt").is_err());
}
