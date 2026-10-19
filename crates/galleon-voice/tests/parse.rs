// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

use galleon_voice::{
    load_path, load_str, Persona, PersonaFile, ReplyLength, SessionPolicy, VoiceIdentity,
    REPLY_ONE_OR_TWO,
};
use std::path::{Path, PathBuf};

fn repo_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name)
}

fn fenced_role(markdown: &str) -> String {
    let start = markdown.find("## Role prompt").expect("role heading");
    let rest = &markdown[start..];
    let open = rest.find("```").expect("opening fence");
    let after = &rest[open + 3..];
    let body_start = after.find('\n').expect("newline") + 1;
    let body = &after[body_start..];
    let close = body.find("```").expect("closing fence");
    body[..close].trim().to_string()
}

#[test]
fn parses_persona_plex_v1() {
    let toml_path = repo_file("personas/persona-plex-v1.toml");
    let md_path = repo_file("personas/persona-plex-v1.md");
    let raw = std::fs::read_to_string(&toml_path).expect("read toml");
    assert!(
        raw.contains("SPDX-License-Identifier: MIT"),
        "toml header lost"
    );
    assert!(raw.contains("rule = \"one or two short sentences\""));

    let doc = load_path(&toml_path).expect("parse v1");
    let md = std::fs::read_to_string(&md_path).expect("read md");
    assert!(md.contains("one or two short sentences"));

    assert_eq!(doc.version, "v1");
    assert_eq!(doc.base, "Samantha preferences v1");
    assert!(doc.created.contains("+02:00"));
    assert!(doc.created.contains("Europe/Zagreb"));
    assert_eq!(doc.identity.name, "Persona Plex");
    assert!(doc.identity.is_persona);
    assert!(!doc.identity.is_model);
    assert!(!doc.identity.is_admin_desk);
    assert_eq!(doc.sources.len(), 7);
    assert_eq!(doc.changelog.len(), 2);
    assert!(doc.changelog[1].note.contains("one or two short sentences"));

    let persona: &dyn Persona = &doc;
    assert_eq!(persona.name(), "Persona Plex");
    assert_eq!(persona.role_prompt(), fenced_role(&md));
    assert!(persona.role_prompt().contains("one or two short sentences"));
    assert_eq!(persona.reply_rule(), "one or two short sentences");
    assert_eq!(persona.language(), "plain English");
    let length: ReplyLength = persona.reply_length();
    assert_eq!(length, REPLY_ONE_OR_TWO);
    assert_eq!(length.min_sentences, 1);
    assert_eq!(length.max_sentences, 2);
    assert!(length.allows(1));
    assert!(length.allows(2));
    assert!(!length.allows(0));
    assert!(!length.allows(3));
    assert_eq!(doc.reply.decided_by, "Slavik");
    assert!(doc.reply.decided_at.contains("2026-10-18 19:27"));
    assert_eq!(doc.reply.unit, "short sentences");

    let voice: &dyn VoiceIdentity = &doc;
    assert_eq!(voice.voice_name(), "samantha");
    assert_eq!(
        voice.reference_path(),
        "/workspace/galion-src/voice-backend/assets/voices/samantha.wav"
    );
    assert!(!voice.published_with_stack());
    assert!(doc.voice_prompt.inferred_not_verified);

    let policy: &dyn SessionPolicy = &doc;
    assert_eq!(policy.language_limit(), "English only");
    assert_eq!(policy.personas_per_session(), 1);
    assert!(!policy.allows_mid_session_switch());
    assert!(!policy.bundles_weights());
    assert!(!policy.money_approved());
    assert!(!policy.heavy_inference_on_vps());
    assert!(doc.rules.money.contains("never touches money"));
    assert!(doc.context.timestamps.contains("Europe/Zagreb"));
    assert!(!doc.context.voice_model.weights_bundled);
    assert_eq!(doc.context.voice_model.code_license, "MIT");
}

#[test]
fn rejects_bad_reply_length() {
    let path = repo_file("personas/persona-plex-v1.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let bad = text.replace("min_sentences = 1", "min_sentences = 9");
    let err = load_str(&bad, &path).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid persona file"), "{msg}");
    assert!(msg.contains("reply length"), "{msg}");
}

#[test]
fn rejects_unknown_field() {
    let path = repo_file("personas/persona-plex-v1.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let bad = format!("{text}\nnot_a_persona_field = true\n");
    let err = load_str(&bad, Path::new("persona-plex-v1.toml")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("bad persona file"), "{msg}");
    assert!(msg.contains("not_a_persona_field"), "{msg}");
}

#[test]
fn rejects_missing_file() {
    let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/no-such-persona.toml");
    let err = load_path(&missing).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cannot read persona file"), "{msg}");
    assert!(msg.contains("no-such-persona.toml"), "{msg}");
}

#[test]
fn rejects_bad_toml() {
    let err = load_str("version = [\n", Path::new("bad.toml")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("bad persona file"), "{msg}");
    assert!(msg.contains("bad.toml"), "{msg}");
}

#[test]
fn path_and_str_agree() {
    let path = repo_file("personas/persona-plex-v1.toml");
    let from_path: PersonaFile = load_path(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let from_str = load_str(&text, &path).unwrap();
    assert_eq!(from_path, from_str);
}
