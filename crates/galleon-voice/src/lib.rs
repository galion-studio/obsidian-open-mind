// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Galleon Voice scaffold.
//!
//! DESK-ID: openmind.B47
//!
//! Loads `personas/persona-plex-v1.toml` and exposes it through text-only
//! traits. Nothing here captures, encodes, decodes, or plays sound, and
//! nothing opens a socket. A later crate would speak. This one only reads
//! the persona document.

#![forbid(unsafe_code)]

use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Official Persona Plex v1 reply length: one or two short sentences.
pub const REPLY_ONE_OR_TWO: ReplyLength = ReplyLength {
    min_sentences: 1,
    max_sentences: 2,
};

/// How many short sentences a spoken reply may use. A count, not samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyLength {
    pub min_sentences: u8,
    pub max_sentences: u8,
}

impl ReplyLength {
    /// True when `count` sits inside this bound.
    #[must_use]
    pub const fn allows(self, count: u8) -> bool {
        count >= self.min_sentences && count <= self.max_sentences
    }
}

/// Text persona a later voice path would speak as.
///
/// Object-safe. No method accepts or returns sound.
pub trait Persona {
    fn name(&self) -> &str;
    fn role_prompt(&self) -> &str;
    fn reply_length(&self) -> ReplyLength;
    /// Phrase from the document, such as "one or two short sentences".
    fn reply_rule(&self) -> &str;
    /// Spoken wording, such as "plain English".
    fn language(&self) -> &str;
}

/// Named voice reference. The path is stored and never opened.
pub trait VoiceIdentity {
    fn voice_name(&self) -> &str;
    fn reference_path(&self) -> &str;
    fn published_with_stack(&self) -> bool;
}

/// Session rules from the persona document. Still text. No stream.
pub trait SessionPolicy {
    fn language_limit(&self) -> &str;
    fn personas_per_session(&self) -> u8;
    fn allows_mid_session_switch(&self) -> bool;
    fn bundles_weights(&self) -> bool;
    fn money_approved(&self) -> bool;
    fn heavy_inference_on_vps(&self) -> bool;
}

/// Why a persona file could not be loaded.
#[derive(Debug)]
pub enum PersonaError {
    /// The path could not be read.
    Read { file: PathBuf, message: String },
    /// The bytes are not this document's TOML.
    Parse { file: PathBuf, message: String },
    /// The document parsed but failed a structural check.
    Invalid { file: PathBuf, message: String },
}

impl fmt::Display for PersonaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersonaError::Read { file, message } => {
                write!(f, "cannot read persona file {}: {message}", file.display())
            }
            PersonaError::Parse { file, message } => {
                write!(f, "bad persona file {}: {message}", file.display())
            }
            PersonaError::Invalid { file, message } => {
                write!(f, "invalid persona file {}: {message}", file.display())
            }
        }
    }
}

impl std::error::Error for PersonaError {}

/// Parsed `persona-plex-v1.toml`. Fields mirror the markdown. No samples.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonaFile {
    pub version: String,
    pub created: String,
    pub base: String,
    pub status: String,
    pub version_note: String,
    pub identity: Identity,
    pub context: Context,
    pub reply: Reply,
    pub traits: Traits,
    pub role_prompt: String,
    pub voice_prompt: VoicePrompt,
    pub sources: Vec<Source>,
    pub rules: Rules,
    pub limits: Limits,
    pub changelog: Vec<Change>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub name: String,
    pub summary: String,
    pub is_persona: bool,
    pub is_model: bool,
    pub is_admin_desk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub job: String,
    pub stack: String,
    pub front_end: String,
    pub boss: String,
    pub boss_note: String,
    pub foundation: String,
    pub timestamps: String,
    pub today: Today,
    pub voice_model: VoiceModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Today {
    pub ear: String,
    pub mind: String,
    pub mouth: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceModel {
    pub id: String,
    pub released: String,
    pub summary: String,
    pub persona_control: String,
    pub code_license: String,
    pub weights_license: String,
    pub weights_bundled: bool,
    pub gpu_class: String,
    pub placement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub min_sentences: u8,
    pub max_sentences: u8,
    pub unit: String,
    pub rule: String,
    pub decided_at: String,
    pub decided_by: String,
    pub replaces: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traits {
    pub opening: String,
    pub tone: String,
    pub tone_note: String,
    pub language: String,
    pub greeting: String,
    pub identity_speech: String,
    pub spoken_form: String,
    pub honesty: String,
    pub file_access: String,
    pub values: String,
    pub example: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoicePrompt {
    pub name: String,
    pub reference: String,
    pub note: String,
    pub inferred_not_verified: bool,
    pub fallback: String,
    pub publish_with_stack: bool,
    pub stays_on: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub language_hygiene: String,
    pub language_hygiene_source: String,
    pub admin: String,
    pub admin_source: String,
    pub money: String,
    pub money_approved: bool,
    pub heavy_inference_on_vps: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub language: String,
    pub personas_per_session: u8,
    pub mid_session_switch: bool,
    pub weights_bundled: bool,
    pub weights_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub version: String,
    pub at: String,
    pub note: String,
}

impl Persona for PersonaFile {
    fn name(&self) -> &str {
        &self.identity.name
    }

    fn role_prompt(&self) -> &str {
        &self.role_prompt
    }

    fn reply_length(&self) -> ReplyLength {
        ReplyLength {
            min_sentences: self.reply.min_sentences,
            max_sentences: self.reply.max_sentences,
        }
    }

    fn reply_rule(&self) -> &str {
        &self.reply.rule
    }

    fn language(&self) -> &str {
        &self.traits.language
    }
}

impl VoiceIdentity for PersonaFile {
    fn voice_name(&self) -> &str {
        &self.voice_prompt.name
    }

    fn reference_path(&self) -> &str {
        &self.voice_prompt.reference
    }

    fn published_with_stack(&self) -> bool {
        self.voice_prompt.publish_with_stack
    }
}

impl SessionPolicy for PersonaFile {
    fn language_limit(&self) -> &str {
        &self.limits.language
    }

    fn personas_per_session(&self) -> u8 {
        self.limits.personas_per_session
    }

    fn allows_mid_session_switch(&self) -> bool {
        self.limits.mid_session_switch
    }

    fn bundles_weights(&self) -> bool {
        self.limits.weights_bundled || self.context.voice_model.weights_bundled
    }

    fn money_approved(&self) -> bool {
        self.rules.money_approved
    }

    fn heavy_inference_on_vps(&self) -> bool {
        self.rules.heavy_inference_on_vps
    }
}

impl PersonaFile {
    fn check(&self, file: &Path) -> Result<(), PersonaError> {
        let fields = [
            ("version", self.version.as_str()),
            ("created", self.created.as_str()),
            ("base", self.base.as_str()),
            ("status", self.status.as_str()),
            ("identity.name", self.identity.name.as_str()),
            ("identity.summary", self.identity.summary.as_str()),
            ("reply.rule", self.reply.rule.as_str()),
            ("reply.unit", self.reply.unit.as_str()),
            ("role_prompt", self.role_prompt.as_str()),
            ("voice_prompt.name", self.voice_prompt.name.as_str()),
            (
                "voice_prompt.reference",
                self.voice_prompt.reference.as_str(),
            ),
            ("traits.language", self.traits.language.as_str()),
            ("limits.language", self.limits.language.as_str()),
        ];
        for (name, value) in fields {
            if value.trim().is_empty() {
                return Err(invalid(file, format!("{name} is empty")));
            }
        }
        if self.reply.min_sentences < 1 || self.reply.max_sentences < self.reply.min_sentences {
            return Err(invalid(
                file,
                "reply length must contain at least one sentence, and max must be at least min",
            ));
        }
        if self.limits.personas_per_session < 1 {
            return Err(invalid(file, "personas_per_session must be at least 1"));
        }
        if self.sources.is_empty() {
            return Err(invalid(file, "sources is empty"));
        }
        for source in &self.sources {
            if source.path.trim().is_empty() {
                return Err(invalid(file, "a source path is empty"));
            }
        }
        if self.changelog.is_empty() {
            return Err(invalid(file, "changelog is empty"));
        }
        Ok(())
    }
}

fn invalid(file: &Path, message: impl Into<String>) -> PersonaError {
    PersonaError::Invalid {
        file: file.to_path_buf(),
        message: message.into(),
    }
}

/// Read a persona document from `path`. Does not open the voice reference.
pub fn load_path(path: &Path) -> Result<PersonaFile, PersonaError> {
    let text = fs::read_to_string(path).map_err(|err| PersonaError::Read {
        file: path.to_path_buf(),
        message: err.to_string(),
    })?;
    load_str(&text, path)
}

/// Parse a persona document. `file` is only used in error text.
///
/// The role prompt is trimmed so a multiline TOML string matches the fenced
/// block in the markdown.
pub fn load_str(text: &str, file: &Path) -> Result<PersonaFile, PersonaError> {
    let mut doc: PersonaFile = toml::from_str(text).map_err(|err| PersonaError::Parse {
        file: file.to_path_buf(),
        message: err.to_string(),
    })?;
    doc.role_prompt = doc.role_prompt.trim().to_string();
    doc.check(file)?;
    Ok(doc)
}
