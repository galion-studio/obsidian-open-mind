// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! System prompt assembly: SOUL.md, the channel prompt for the caller
//! (channels/<name>.md, e.g. galion-one), config `agent.system_prompt`,
//! then the client's own system messages (ephemeral, per request).

use std::path::Path;

pub const MAX_FILE_CHARS: usize = 20_000;

fn read_capped(p: &Path) -> Option<String> {
    let t = std::fs::read_to_string(p).ok()?;
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.chars().take(MAX_FILE_CHARS).collect())
}

/// Channel name for a declared session key. `galion-one`, `one-warm` and
/// `galion-one:*` map to `galion-one`; otherwise the sanitised key itself if
/// a matching channels/<key>.md exists.
pub fn channel_for_key(key: Option<&str>) -> Option<String> {
    let k = key?.trim();
    if k.is_empty() {
        return None;
    }
    if k == "galion-one" || k == "one-warm" || k.starts_with("galion-one:") {
        return Some("galion-one".into());
    }
    let s: String = k
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    (!s.is_empty() && s.len() <= 64).then_some(s)
}

#[derive(Debug, Clone, Default)]
pub struct PromptParts {
    pub soul: Option<String>,
    pub channel: Option<String>,
    pub config: Option<String>,
    pub client: Vec<String>,
    /// Persona Plex prompt (role + channel rules). Goes last so it wins.
    pub persona: Option<String>,
}

impl PromptParts {
    pub fn load(home: &Path, channel: Option<&str>, config_prompt: Option<&str>) -> Self {
        PromptParts {
            soul: read_capped(&home.join("SOUL.md")),
            channel: channel
                .and_then(|c| read_capped(&home.join("channels").join(format!("{c}.md")))),
            config: config_prompt.map(str::to_string),
            client: Vec::new(),
            persona: None,
        }
    }

    pub fn build(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(s) = &self.soul {
            parts.push(s);
        }
        if let Some(s) = &self.channel {
            parts.push(s);
        }
        if let Some(s) = &self.config {
            parts.push(s);
        }
        for c in &self.client {
            parts.push(c);
        }
        if let Some(p) = &self.persona {
            parts.push(p);
        }
        parts.join("\n\n")
    }
}

/// Persona Plex for a channel, when a persona file is configured and valid.
/// Galion One's admin line (`one-admin`) is the text channel; every other
/// galion-one session is voice. Returns the role prompt + channel rules
/// (SOUL.md stays in its own slot).
pub fn persona_prompt(
    file: Option<&Path>,
    channel: Option<&str>,
    session_id: &str,
) -> Option<String> {
    let file = file?;
    if channel != Some("galion-one") {
        return None;
    }
    let p = openmind_persona::Persona::from_file(file).ok()?;
    if !p.is_valid() {
        return None;
    }
    let ch = if session_id == "one-admin" {
        openmind_persona::Channel::Text
    } else {
        openmind_persona::Channel::Voice
    };
    Some(p.prompt(None, ch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_mapping() {
        assert_eq!(
            channel_for_key(Some("galion-one")).as_deref(),
            Some("galion-one")
        );
        assert_eq!(
            channel_for_key(Some("one-warm")).as_deref(),
            Some("galion-one")
        );
        assert_eq!(
            channel_for_key(Some("console:user:1:project:x")).as_deref(),
            Some("console-user-1-project-x")
        );
        assert_eq!(channel_for_key(None), None);
    }

    #[test]
    fn builds_in_order() {
        let dir = std::env::temp_dir().join(crate::new_id("hermes-rs-persona-"));
        std::fs::create_dir_all(dir.join("channels")).unwrap();
        std::fs::write(dir.join("SOUL.md"), "SOUL").unwrap();
        std::fs::write(dir.join("channels/galion-one.md"), "# Galion One\nwarm").unwrap();
        let mut p = PromptParts::load(&dir, Some("galion-one"), Some("CFG"));
        p.client.push("CLIENT".into());
        assert_eq!(p.build(), "SOUL\n\n# Galion One\nwarm\n\nCFG\n\nCLIENT");
        let _ = std::fs::remove_dir_all(dir);
    }
}
