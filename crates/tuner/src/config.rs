// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Bind address and www root. Defaults match `/opt/galion/tuner` today.

use std::fmt;
use std::fs;
use std::io;

/// Live listen address when `TUNER_ADDR` is unset and no config file sets `bind`.
pub const DEFAULT_ADDR: &str = "127.0.0.1:8650";

/// Live www directory when `TUNER_ROOT` is unset and no config file sets `root`.
pub const DEFAULT_ROOT: &str = "/opt/galion/tuner/www";

/// Resolved listen address and file root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Socket address passed to `TcpListener::bind`.
    pub bind: String,
    /// Directory that holds the whitelisted files.
    pub root: String,
}

/// A bad config path or a config file this crate will not guess about.
#[derive(Debug)]
pub enum ConfigError {
    /// The file named by `TUNER_CONFIG` could not be read.
    Read { path: String, source: io::Error },
    /// The file is not the small `bind` / `root` assignment format.
    Parse {
        path: String,
        line: usize,
        message: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read { path, source } => write!(f, "config {path}: {source}"),
            ConfigError::Parse {
                path,
                line,
                message,
            } => write!(f, "config {path}:{line}: {message}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Read { source, .. } => Some(source),
            ConfigError::Parse { .. } => None,
        }
    }
}

/// Resolve bind and root from an env lookup. The live process uses `std::env::var`.
pub fn resolve(env: &dyn Fn(&str) -> Option<String>) -> Result<Settings, ConfigError> {
    let (file_bind, file_root) = match config_path(env) {
        Some(path) => parse_config(&path)?,
        None => (None, None),
    };
    let bind = match env("TUNER_ADDR") {
        Some(value) => value,
        None => file_bind.unwrap_or_else(|| DEFAULT_ADDR.to_string()),
    };
    let root = match env("TUNER_ROOT") {
        Some(value) => value,
        None => file_root.unwrap_or_else(|| DEFAULT_ROOT.to_string()),
    };
    Ok(Settings { bind, root })
}

fn config_path(env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let raw = env("TUNER_CONFIG")?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_err(path: &str, line: usize, message: impl Into<String>) -> ConfigError {
    ConfigError::Parse {
        path: path.to_string(),
        line,
        message: message.into(),
    }
}

fn parse_config(path: &str) -> Result<(Option<String>, Option<String>), ConfigError> {
    let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_string(),
        source,
    })?;
    let mut bind = None;
    let mut root = None;
    for (idx, line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (key, value) = split_assign(path, line_no, trimmed)?;
        match key {
            "bind" => {
                if bind.is_some() {
                    return Err(parse_err(path, line_no, "duplicate key bind"));
                }
                bind = Some(value);
            }
            "root" => {
                if root.is_some() {
                    return Err(parse_err(path, line_no, "duplicate key root"));
                }
                root = Some(value);
            }
            other => {
                return Err(parse_err(path, line_no, format!("unknown key {other}")));
            }
        }
    }
    Ok((bind, root))
}

fn split_assign<'a>(
    path: &str,
    line: usize,
    trimmed: &'a str,
) -> Result<(&'a str, String), ConfigError> {
    let Some((key_raw, value_raw)) = trimmed.split_once('=') else {
        return Err(parse_err(path, line, "expected key = \"value\""));
    };
    let key = key_raw.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_lowercase()) {
        return Err(parse_err(path, line, "invalid key"));
    }
    let value = parse_quoted(path, line, value_raw.trim())?;
    Ok((key, value))
}

fn parse_quoted(path: &str, line: usize, raw: &str) -> Result<String, ConfigError> {
    let mut chars = raw.chars();
    if chars.next() != Some('"') {
        return Err(parse_err(
            path,
            line,
            "value must be a double-quoted string",
        ));
    }
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if chars.any(|rest| !rest.is_whitespace()) {
                    return Err(parse_err(path, line, "trailing junk after string"));
                }
                return Ok(out);
            }
            '\\' => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                _ => return Err(parse_err(path, line, "unknown escape in string")),
            },
            other => out.push(other),
        }
    }
    Err(parse_err(path, line, "unterminated string"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_string())
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tuner-b53-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_cfg(name: &str, body: &str) -> PathBuf {
        let path = scratch(name).join("tuner.conf");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn defaults_match_the_live_tuner() {
        assert_eq!(DEFAULT_ADDR, "127.0.0.1:8650");
        assert_eq!(DEFAULT_ROOT, "/opt/galion/tuner/www");
        let settings = resolve(&|_| None).unwrap();
        assert_eq!(settings.bind, DEFAULT_ADDR);
        assert_eq!(settings.root, DEFAULT_ROOT);
    }

    #[test]
    fn env_wins_and_empty_is_not_the_default() {
        let settings = resolve(&lookup(&[
            ("TUNER_ADDR", "127.0.0.1:19000"),
            ("TUNER_ROOT", "/tmp/tuner-www"),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:19000");
        assert_eq!(settings.root, "/tmp/tuner-www");

        let empty = resolve(&lookup(&[("TUNER_ADDR", ""), ("TUNER_ROOT", "")])).unwrap();
        assert_eq!(empty.bind, "");
        assert_eq!(empty.root, "");
    }

    #[test]
    fn blank_config_path_is_unset() {
        let settings = resolve(&lookup(&[("TUNER_CONFIG", "   ")])).unwrap();
        assert_eq!(settings.bind, DEFAULT_ADDR);
        assert_eq!(settings.root, DEFAULT_ROOT);
    }

    #[test]
    fn file_sets_bind_and_root() {
        let path = write_cfg(
            "file",
            "# comment\n\nbind = \"127.0.0.1:19050\"\nroot = \"/tmp/tuner root\"\n",
        );
        let path = path.to_str().unwrap();
        let settings = resolve(&lookup(&[("TUNER_CONFIG", path)])).unwrap();
        assert_eq!(settings.bind, "127.0.0.1:19050");
        assert_eq!(settings.root, "/tmp/tuner root");
    }

    #[test]
    fn env_overrides_file_per_field() {
        let path = write_cfg(
            "override",
            "bind = \"127.0.0.1:1\"\nroot = \"/tmp/from-file\"\n",
        );
        let path = path.to_str().unwrap();
        let settings = resolve(&lookup(&[
            ("TUNER_CONFIG", path),
            ("TUNER_ADDR", "127.0.0.1:19051"),
        ]))
        .unwrap();
        assert_eq!(settings.bind, "127.0.0.1:19051");
        assert_eq!(settings.root, "/tmp/from-file");
    }

    #[test]
    fn escapes_and_rejects_junk() {
        let path = write_cfg("esc", "root = \"a\\\"b\\\\c\"\n");
        let path = path.to_str().unwrap();
        let settings = resolve(&lookup(&[("TUNER_CONFIG", path)])).unwrap();
        assert_eq!(settings.root, "a\"b\\c");
        assert_eq!(settings.bind, DEFAULT_ADDR);

        let bad = write_cfg("junk", "bind = \"127.0.0.1:9\" leftover\n");
        let bad = bad.to_str().unwrap();
        let err = resolve(&lookup(&[("TUNER_CONFIG", bad)])).unwrap_err();
        assert!(err.to_string().contains("trailing junk"));
    }

    #[test]
    fn unknown_key_does_not_echo_the_value() {
        let path = write_cfg("unknown", "extra = \"do-not-echo\"\n");
        let path = path.to_str().unwrap();
        let err = resolve(&lookup(&[("TUNER_CONFIG", path)])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown key extra"), "{msg}");
        assert!(!msg.contains("do-not-echo"), "{msg}");
    }

    #[test]
    fn missing_file_and_duplicate_key() {
        let err = resolve(&lookup(&[("TUNER_CONFIG", "/no/such/tuner-b53.conf")])).unwrap_err();
        assert!(err.to_string().contains("/no/such/tuner-b53.conf"));

        let path = write_cfg("dup", "root = \"/a\"\nroot = \"/b\"\n");
        let path = path.to_str().unwrap();
        let err = resolve(&lookup(&[("TUNER_CONFIG", path)])).unwrap_err();
        assert!(err.to_string().contains("duplicate key root"));
    }
}
