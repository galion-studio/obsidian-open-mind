// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! # openmind-persona (Persona Plex)
//!
//! The persona layer for Galion agents. A persona is a Markdown file such as
//! `personas/persona-plex-v1.md`. This crate is std-only and portable.
//!
//! Stable API (v1, consumed by hermes-rs, Galion One and galion-voice-core):
//!
//! * [`Persona::parse`] / [`Persona::from_file`] - load a persona file.
//! * [`Persona::validate`] - list problems ([`Issue`]); errors mean "do not use".
//! * [`Persona::role_prompt`] - the text role prompt, verbatim.
//! * [`Persona::voice_prompt`] - the audio voice prompt reference (a path on
//!   the GPU host; never bundled).
//! * [`Persona::style_rules`] - per-channel rules for [`Channel::Text`] or [`Channel::Voice`].
//! * [`Persona::prompt`] - SOUL.md + role prompt + channel rules, one string.
//! * [`Persona::version`], [`Persona::changelog`].
//!
//! Anything else is internal and may change.

use std::fmt;
use std::path::Path;

/// The official reply-length rule (decided by Slavik on 2026-10-18).
pub const LENGTH_RULE: &str = "one or two short sentences";
pub const MAX_ROLE_PROMPT_CHARS: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Typed chat (admin line, console).
    Text,
    /// Spoken reply (Galion One mouth, PersonaPlex speech-to-speech).
    Voice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    pub version: String,
    /// As written, e.g. "2026-10-18 19:25 CEST (+02:00)".
    pub when: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePrompt {
    /// Audio reference path (e.g. a .wav), if the persona names one.
    pub reference: Option<String>,
    /// The other notes of the "Voice prompt" section.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    pub name: String,
    version: String,
    pub created: Option<String>,
    pub base: Option<String>,
    pub status: Option<String>,
    pub traits: Vec<String>,
    role_prompt: String,
    voice: VoicePrompt,
    pub rules: Vec<String>,
    pub limits: Vec<String>,
    changelog: Vec<ChangelogEntry>,
    /// The bold value of the `Length` trait, if present.
    pub length_rule: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub level: Level,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "persona: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

fn sections(md: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = vec![(String::new(), Vec::new())];
    let mut in_fence = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence && line.starts_with("## ") {
            out.push((line[3..].trim().to_string(), Vec::new()));
            continue;
        }
        if let Some(last) = out.last_mut() {
            last.1.push(line.to_string());
        }
    }
    out
}

fn bullets(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| l.trim_start().strip_prefix("- "))
        .map(|s| s.trim().to_string())
        .collect()
}

fn first_fence(lines: &[String]) -> Option<String> {
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with("```"))?;
    let len = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with("```"))?;
    Some(lines[start + 1..start + 1 + len].join("\n"))
}

fn backticked(s: &str) -> Vec<String> {
    s.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn bold(s: &str) -> Option<String> {
    let a = s.find("**")?;
    let b = s[a + 2..].find("**")?;
    Some(s[a + 2..a + 2 + b].trim().trim_end_matches('.').to_string())
}

impl Persona {
    pub fn from_file(p: &Path) -> Result<Persona, ParseError> {
        let text =
            std::fs::read_to_string(p).map_err(|e| ParseError(format!("{}: {e}", p.display())))?;
        Persona::parse(&text)
    }

    pub fn parse(md: &str) -> Result<Persona, ParseError> {
        let secs = sections(md);
        let title = md
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .ok_or_else(|| ParseError("missing `# <Name> <version>` title".into()))?
            .trim()
            .to_string();
        let (mut name, mut version) = match title.rsplit_once(' ') {
            Some((n, v)) if v.starts_with('v') => (n.to_string(), v.to_string()),
            _ => (title.clone(), String::new()),
        };
        let (mut created, mut base, mut status) = (None, None, None);
        for l in &secs[0].1 {
            let cells: Vec<&str> = l
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            if cells.len() < 2 {
                continue;
            }
            let val = cells[1].to_string();
            match cells[0] {
                "Version" => version = val,
                "Created" => created = Some(val),
                "Base" => base = Some(val),
                "Status" => status = Some(val),
                "Name" => name = val,
                _ => {}
            }
        }
        let get = |h: &str| {
            secs.iter()
                .find(|(t, _)| t.to_lowercase().starts_with(&h.to_lowercase()))
                .map(|(_, l)| l.as_slice())
        };
        let traits = get("Persona traits").map(bullets).unwrap_or_default();
        let length_rule = traits
            .iter()
            .find(|t| t.starts_with("Length"))
            .and_then(|t| bold(t));
        let role_prompt = get("Role prompt")
            .and_then(first_fence)
            .ok_or_else(|| ParseError("missing `## Role prompt` with a fenced block".into()))?
            .trim()
            .to_string();
        let vnotes = get("Voice prompt").map(bullets).unwrap_or_default();
        let reference = vnotes.iter().flat_map(|n| backticked(n)).find(|p| {
            let p = p.to_lowercase();
            p.ends_with(".wav")
                || p.ends_with(".mp3")
                || p.ends_with(".flac")
                || p.ends_with(".ogg")
        });
        let changelog = get("Changelog")
            .map(bullets)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|b| {
                let (head, text) = b
                    .split_once("): ")
                    .map(|(h, t)| (format!("{h})"), t))
                    .or_else(|| b.split_once(": ").map(|(h, t)| (h.to_string(), t)))?;
                let (v, when) = head.split_once(", ")?;
                Some(ChangelogEntry {
                    version: v.trim().into(),
                    when: when.trim().into(),
                    text: text.trim().into(),
                })
            })
            .collect();
        Ok(Persona {
            name,
            version,
            created,
            base,
            status,
            traits,
            role_prompt,
            voice: VoicePrompt {
                reference,
                notes: vnotes,
            },
            rules: get("Rules").map(bullets).unwrap_or_default(),
            limits: get("Limits").map(bullets).unwrap_or_default(),
            changelog,
            length_rule,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn role_prompt(&self) -> &str {
        &self.role_prompt
    }

    pub fn voice_prompt(&self) -> &VoicePrompt {
        &self.voice
    }

    pub fn changelog(&self) -> &[ChangelogEntry] {
        &self.changelog
    }

    pub fn validate(&self) -> Vec<Issue> {
        let mut v = Vec::new();
        let mut err = |m: String| {
            v.push(Issue {
                level: Level::Error,
                message: m,
            })
        };
        if self.name.trim().is_empty() {
            err("persona has no name".into());
        }
        if !self.version.starts_with('v') || self.version.len() < 2 {
            err(format!(
                "version `{}` is not of the form v<N>",
                self.version
            ));
        }
        if self.role_prompt.chars().count() > MAX_ROLE_PROMPT_CHARS {
            err(format!(
                "role prompt is longer than {MAX_ROLE_PROMPT_CHARS} chars"
            ));
        }
        let rp = self.role_prompt.to_lowercase();
        if !rp.contains(LENGTH_RULE) {
            err(format!(
                "role prompt does not carry the official length rule: \"{LENGTH_RULE}\""
            ));
        }
        if let Some(l) = &self.length_rule {
            if !l.to_lowercase().contains(LENGTH_RULE) {
                err(format!(
                    "Length trait `{l}` disagrees with the official rule"
                ));
            }
        }
        if !rp.contains("move money") {
            err("role prompt must refuse money: Revolut / real money is not approved".into());
        }
        if !rp.contains("not a copy of another model") && !rp.contains("do not say you are a model")
        {
            err("role prompt must keep the identity rule (not a model, not a copy)".into());
        }
        if let Some(r) = &self.voice.reference {
            if r.starts_with("http://") || r.starts_with("https://") {
                err("voice prompt reference must be a local path, never a published URL".into());
            }
        }
        let mut warn = |m: String| {
            v.push(Issue {
                level: Level::Warning,
                message: m,
            })
        };
        if self.voice.reference.is_none() {
            warn("no voice prompt reference (.wav) found".into());
        }
        if self.changelog.is_empty() {
            warn("no changelog entries".into());
        }
        for c in &self.changelog {
            if !c.when.contains("+0") && !c.when.contains("-0") && !c.when.contains("+1") {
                warn(format!("changelog `{}` has no explicit UTC offset", c.when));
            }
            if version_num(&c.version) > version_num(&self.version) {
                warn(format!(
                    "changelog mentions {} newer than the file version {}",
                    c.version, self.version
                ));
            }
        }
        v
    }

    pub fn is_valid(&self) -> bool {
        self.validate().iter().all(|i| i.level != Level::Error)
    }

    /// Per-channel style rules. Voice is spoken form only; text keeps the
    /// same length rule but allows plain written prose.
    pub fn style_rules(&self, channel: Channel) -> Vec<String> {
        let len = format!("Keep it to {LENGTH_RULE}.");
        match channel {
            Channel::Voice => vec![
                len,
                "Spoken form only: no lists, no headings, no markdown, no paths, no code, no emoji.".into(),
                "Say numbers as words a person would say aloud.".into(),
                "No greeting unless they greeted you.".into(),
                "English only.".into(),
            ],
            Channel::Text => vec![
                format!("{len} Longer only when they ask for detail."),
                "Plain English prose: no headings, no tables. A short list only if they ask for one.".into(),
                "No greeting unless they greeted you.".into(),
            ],
        }
    }

    /// The full system prompt: SOUL.md (if any), then the persona role
    /// prompt, then the channel rules. Later text wins on conflict, so the
    /// persona overrides SOUL for this channel.
    pub fn prompt(&self, soul: Option<&str>, channel: Channel) -> String {
        let mut out = String::new();
        if let Some(s) = soul.map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str(s);
            out.push_str("\n\n");
            out.push_str(&format!(
                "## Persona: {} {} (overrides the above on this channel)\n\n",
                self.name, self.version
            ));
        }
        out.push_str(&self.role_prompt);
        out.push_str("\n\n");
        out.push_str(match channel {
            Channel::Voice => "Voice channel rules:\n",
            Channel::Text => "Text channel rules:\n",
        });
        for r in self.style_rules(channel) {
            out.push_str("- ");
            out.push_str(&r);
            out.push('\n');
        }
        out.trim_end().to_string()
    }
}

fn version_num(v: &str) -> u32 {
    v.trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
