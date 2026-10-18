// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Config loading. A deliberately small TOML subset parser (tables, `key =
//! value` with strings, integers, floats, booleans, `#` comments) so the crate
//! needs no third-party dependency and does not grow the workspace Cargo.lock.
//! Arrays, inline tables and multi-line strings are rejected with a line number.
//!
//! No host, port or path is hard-coded in the binary: every one comes from the
//! config file. Secrets never appear in the file, only env var NAMES.

use std::collections::BTreeMap;
use std::fmt;

/// A parsed scalar value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Error with an optional 1-based line number.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub msg: String,
}

impl ConfigError {
    fn at(line: usize, msg: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            msg: msg.into(),
        }
    }
    fn new(msg: impl Into<String>) -> Self {
        Self {
            line: None,
            msg: msg.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "config line {l}: {}", self.msg),
            None => write!(f, "config: {}", self.msg),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parsed document: table name -> key -> value. Keys before any table header
/// land in the "" table.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Document {
    tables: BTreeMap<String, BTreeMap<String, Value>>,
}

fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Parse a basic double-quoted string starting at `s[0] == '"'`. Returns the
/// decoded string and the rest of the input after the closing quote.
fn parse_quoted(s: &str, line: usize) -> Result<(String, &str), ConfigError> {
    let mut out = String::new();
    let mut chars = s.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &s[i + 1..])),
            '\\' => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                _ => return Err(ConfigError::at(line, "unsupported escape in string")),
            },
            _ => out.push(c),
        }
    }
    Err(ConfigError::at(line, "unterminated string"))
}

fn parse_value(raw: &str, line: usize) -> Result<Value, ConfigError> {
    let raw = raw.trim();
    if raw.starts_with('"') {
        let (s, rest) = parse_quoted(raw, line)?;
        let rest = rest.trim();
        if !rest.is_empty() && !rest.starts_with('#') {
            return Err(ConfigError::at(line, "unexpected text after string"));
        }
        return Ok(Value::Str(s));
    }
    let v = raw.split('#').next().unwrap_or("").trim();
    if v.is_empty() {
        return Err(ConfigError::at(line, "missing value"));
    }
    if v.starts_with('[') || v.starts_with('{') || v.starts_with('\'') {
        return Err(ConfigError::at(
            line,
            "arrays, inline tables and literal strings are not supported",
        ));
    }
    match v {
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        _ => {}
    }
    let num = v.replace('_', "");
    if let Ok(i) = num.parse::<i64>() {
        return Ok(Value::Int(i));
    }
    if let Ok(f) = num.parse::<f64>() {
        if f.is_finite() {
            return Ok(Value::Float(f));
        }
    }
    Err(ConfigError::at(line, format!("cannot parse value `{v}`")))
}

impl Document {
    /// Parse the TOML subset.
    pub fn parse(src: &str) -> Result<Self, ConfigError> {
        let mut doc = Document::default();
        let mut current = String::new();
        doc.tables.entry(current.clone()).or_default();
        for (idx, raw_line) in src.lines().enumerate() {
            let line_no = idx + 1;
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                let body = rest.split('#').next().unwrap_or("").trim();
                let name = body
                    .strip_suffix(']')
                    .ok_or_else(|| ConfigError::at(line_no, "table header missing `]`"))?
                    .trim();
                if name.starts_with('[') || !valid_ident(name) {
                    return Err(ConfigError::at(line_no, "invalid table header"));
                }
                if doc.tables.contains_key(name) && !doc.tables[name].is_empty() {
                    return Err(ConfigError::at(
                        line_no,
                        format!("duplicate table [{name}]"),
                    ));
                }
                current = name.to_string();
                doc.tables.entry(current.clone()).or_default();
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| ConfigError::at(line_no, "expected `key = value`"))?;
            let key = key.trim();
            if !valid_ident(key) {
                return Err(ConfigError::at(line_no, format!("invalid key `{key}`")));
            }
            let value = parse_value(value, line_no)?;
            let table = doc.tables.entry(current.clone()).or_default();
            if table.insert(key.to_string(), value).is_some() {
                return Err(ConfigError::at(line_no, format!("duplicate key `{key}`")));
            }
        }
        Ok(doc)
    }

    fn get(&self, table: &str, key: &str) -> Result<&Value, ConfigError> {
        self.tables
            .get(table)
            .and_then(|t| t.get(key))
            .ok_or_else(|| ConfigError::new(format!("missing [{table}] {key}")))
    }

    pub fn str(&self, table: &str, key: &str) -> Result<String, ConfigError> {
        match self.get(table, key)? {
            Value::Str(s) => Ok(s.clone()),
            _ => Err(ConfigError::new(format!(
                "[{table}] {key} must be a string"
            ))),
        }
    }

    pub fn bool(&self, table: &str, key: &str) -> Result<bool, ConfigError> {
        match self.get(table, key)? {
            Value::Bool(b) => Ok(*b),
            _ => Err(ConfigError::new(format!(
                "[{table}] {key} must be a boolean"
            ))),
        }
    }

    pub fn u32(&self, table: &str, key: &str) -> Result<u32, ConfigError> {
        match self.get(table, key)? {
            Value::Int(i) => u32::try_from(*i).map_err(|_| {
                ConfigError::new(format!(
                    "[{table}] {key} must be a non-negative 32-bit integer"
                ))
            }),
            _ => Err(ConfigError::new(format!(
                "[{table}] {key} must be an integer"
            ))),
        }
    }

    pub fn f32(&self, table: &str, key: &str) -> Result<f32, ConfigError> {
        match self.get(table, key)? {
            Value::Float(f) => Ok(*f as f32),
            Value::Int(i) => Ok(*i as f32),
            _ => Err(ConfigError::new(format!(
                "[{table}] {key} must be a number"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelaySection {
    pub bind: String,
    pub max_sessions: u32,
    pub idle_timeout_s: u32,
    pub auth_token_env: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpstreamSection {
    pub enabled: bool,
    pub url: String,
    pub voice_prompt: String,
    pub role_prompt_file: String,
    pub connect_timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSection {
    pub sample_rate_hz: u32,
    pub frame_ms: u32,
    pub codec: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerSection {
    pub enabled: bool,
    pub model_path: String,
    pub embedding_dim: u32,
    pub accept_threshold: f32,
    pub reject_threshold: f32,
    pub enrollment_min_phrases: u32,
    pub enrollment_min_consistency: f32,
    pub window_ms: u32,
    pub min_speech_ms: u32,
    pub max_consecutive_rejects: u32,
    pub require_liveness: bool,
    pub store_dir: String,
    pub store_key_env: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FallbackSection {
    pub mode: String,
    pub galion_one_url: String,
}

/// The whole relay config.
#[derive(Debug, Clone, PartialEq)]
pub struct RelayConfig {
    pub relay: RelaySection,
    pub upstream: UpstreamSection,
    pub audio: AudioSection,
    pub speaker: SpeakerSection,
    pub fallback: FallbackSection,
}

/// Mimi works on 24 kHz audio at 12.5 Hz frames (80 ms).
pub const MIMI_SAMPLE_RATE_HZ: u32 = 24_000;
pub const MIMI_FRAME_MS: u32 = 80;

fn is_loopback_url(url: &str) -> bool {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = after_scheme
        .split(['/', '?'])
        .next()
        .unwrap_or("")
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(after_scheme);
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

impl RelayConfig {
    pub fn from_toml(src: &str) -> Result<Self, ConfigError> {
        let d = Document::parse(src)?;
        let cfg = RelayConfig {
            relay: RelaySection {
                bind: d.str("relay", "bind")?,
                max_sessions: d.u32("relay", "max_sessions")?,
                idle_timeout_s: d.u32("relay", "idle_timeout_s")?,
                auth_token_env: d.str("relay", "auth_token_env")?,
            },
            upstream: UpstreamSection {
                enabled: d.bool("upstream", "enabled")?,
                url: d.str("upstream", "url")?,
                voice_prompt: d.str("upstream", "voice_prompt")?,
                role_prompt_file: d.str("upstream", "role_prompt_file")?,
                connect_timeout_ms: d.u32("upstream", "connect_timeout_ms")?,
            },
            audio: AudioSection {
                sample_rate_hz: d.u32("audio", "sample_rate_hz")?,
                frame_ms: d.u32("audio", "frame_ms")?,
                codec: d.str("audio", "codec")?,
            },
            speaker: SpeakerSection {
                enabled: d.bool("speaker", "enabled")?,
                model_path: d.str("speaker", "model_path")?,
                embedding_dim: d.u32("speaker", "embedding_dim")?,
                accept_threshold: d.f32("speaker", "accept_threshold")?,
                reject_threshold: d.f32("speaker", "reject_threshold")?,
                enrollment_min_phrases: d.u32("speaker", "enrollment_min_phrases")?,
                enrollment_min_consistency: d.f32("speaker", "enrollment_min_consistency")?,
                window_ms: d.u32("speaker", "window_ms")?,
                min_speech_ms: d.u32("speaker", "min_speech_ms")?,
                max_consecutive_rejects: d.u32("speaker", "max_consecutive_rejects")?,
                require_liveness: d.bool("speaker", "require_liveness")?,
                store_dir: d.str("speaker", "store_dir")?,
                store_key_env: d.str("speaker", "store_key_env")?,
            },
            fallback: FallbackSection {
                mode: d.str("fallback", "mode")?,
                galion_one_url: d.str("fallback", "galion_one_url")?,
            },
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// Structural checks. Returns the first problem found.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let e = |m: &str| Err(ConfigError::new(m.to_string()));
        if self.relay.bind.trim().is_empty() {
            return e("[relay] bind must not be empty");
        }
        if self.relay.max_sessions == 0 {
            return e("[relay] max_sessions must be >= 1");
        }
        if !valid_env_name(&self.relay.auth_token_env) {
            return e("[relay] auth_token_env must be an env var NAME (A-Z, 0-9, _), not a secret");
        }
        let url = &self.upstream.url;
        if !(url.starts_with("wss://") || url.starts_with("ws://")) {
            return e("[upstream] url must be ws:// or wss://");
        }
        if url.starts_with("ws://") && !is_loopback_url(url) {
            return e("[upstream] plain ws:// is only allowed to loopback (e.g. an SSH tunnel); use wss://");
        }
        if self.audio.sample_rate_hz != MIMI_SAMPLE_RATE_HZ {
            return e("[audio] sample_rate_hz must be 24000 (Mimi)");
        }
        if self.audio.frame_ms == 0 || !self.audio.frame_ms.is_multiple_of(MIMI_FRAME_MS) {
            return e("[audio] frame_ms must be a multiple of 80 (Mimi 12.5 Hz frames)");
        }
        if self.audio.codec != "opus" {
            return e("[audio] codec must be \"opus\" (moshi server protocol)");
        }
        let s = &self.speaker;
        let in_range = |x: f32| (-1.0..=1.0).contains(&x);
        if !in_range(s.accept_threshold) || !in_range(s.reject_threshold) {
            return e("[speaker] thresholds must be cosine values in [-1, 1]");
        }
        if s.reject_threshold > s.accept_threshold {
            return e("[speaker] reject_threshold must be <= accept_threshold");
        }
        if !in_range(s.enrollment_min_consistency) {
            return e("[speaker] enrollment_min_consistency must be in [-1, 1]");
        }
        if s.enrollment_min_phrases < 3 {
            return e("[speaker] enrollment_min_phrases must be >= 3");
        }
        if s.embedding_dim == 0 {
            return e("[speaker] embedding_dim must be >= 1");
        }
        if s.min_speech_ms > s.window_ms {
            return e("[speaker] min_speech_ms must be <= window_ms");
        }
        if s.max_consecutive_rejects == 0 {
            return e("[speaker] max_consecutive_rejects must be >= 1");
        }
        if !valid_env_name(&s.store_key_env) {
            return e("[speaker] store_key_env must be an env var NAME, not a secret");
        }
        if self.fallback.mode != "galion-one-text" && self.fallback.mode != "none" {
            return e("[fallback] mode must be \"galion-one-text\" or \"none\"");
        }
        Ok(())
    }
}

/// An env var name: upper-case letters, digits, underscore; not starting with a digit.
pub fn valid_env_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../persona-plex-relay.example.toml");

    #[test]
    fn example_config_parses_and_validates() {
        let cfg = RelayConfig::from_toml(EXAMPLE).expect("example config is valid");
        assert_eq!(cfg.relay.max_sessions, 1);
        assert!(!cfg.upstream.enabled, "GPU upstream must ship disabled");
        assert_eq!(cfg.audio.sample_rate_hz, 24_000);
        assert!(cfg.speaker.reject_threshold <= cfg.speaker.accept_threshold);
    }

    #[test]
    fn parses_scalars_and_comments() {
        let d = Document::parse(
            "top = 1\n[a]\ns = \"x # not a comment\" # comment\nb = true\ni = 1_000\nf = 0.5\n",
        )
        .unwrap();
        assert_eq!(d.str("a", "s").unwrap(), "x # not a comment");
        assert!(d.bool("a", "b").unwrap());
        assert_eq!(d.u32("a", "i").unwrap(), 1000);
        assert!((d.f32("a", "f").unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(d.u32("", "top").unwrap(), 1);
    }

    #[test]
    fn rejects_unsupported_and_duplicates() {
        assert_eq!(
            Document::parse("[a]\nx = [1, 2]\n").unwrap_err().line,
            Some(2)
        );
        assert!(Document::parse("[a]\nx = 1\nx = 2\n").is_err());
        assert!(Document::parse("[a]\nx = \"open\n").is_err());
        assert!(Document::parse("[[arr]]\n").is_err());
    }

    #[test]
    fn refuses_plain_ws_to_remote_host() {
        let bad = EXAMPLE.replace(
            "wss://GPU_HOST_NOT_PROVISIONED:8998/api/chat",
            "ws://203.0.113.7:8998/api/chat",
        );
        assert!(RelayConfig::from_toml(&bad).is_err());
        let tunnel = EXAMPLE.replace(
            "wss://GPU_HOST_NOT_PROVISIONED:8998/api/chat",
            "ws://127.0.0.1:18998/api/chat",
        );
        assert!(RelayConfig::from_toml(&tunnel).is_ok());
    }

    #[test]
    fn refuses_inverted_thresholds() {
        let bad = EXAMPLE.replace("reject_threshold = 0.50", "reject_threshold = 0.90");
        assert!(RelayConfig::from_toml(&bad).is_err());
    }

    #[test]
    fn env_names_not_secrets() {
        assert!(valid_env_name("PERSONA_PLEX_RELAY_TOKEN"));
        assert!(!valid_env_name("sk-live-abc123"));
        assert!(!valid_env_name(""));
        assert!(!valid_env_name("1ABC"));
    }
}
