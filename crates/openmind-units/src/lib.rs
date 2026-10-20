// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Render staged unit text from an openmind.toml fixture.
//!
//! [`render_fixture`] reads TOML and returns unit file bodies. Each body has
//! `Restart=always` and a Europe/Zagreb timestamp comment (`+01:00` or
//! `+02:00`, never a `Z` suffix). There is no install section.
//!
//! This crate prints text. It does not install a unit, start a process, or
//! open a socket.

#![forbid(unsafe_code)]

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

/// Token that must never appear in rendered unit text.
const FORBIDDEN: &str = "systemctl";

/// One staged unit file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedUnit {
    /// Unit name without the `.service` suffix.
    pub name: String,
    /// File name, `<name>.service`.
    pub file_name: String,
    /// Full unit text, including the Zagreb timestamp comment.
    pub text: String,
}

/// Why a fixture could not be rendered.
#[derive(Debug)]
pub enum RenderError {
    /// TOML did not match the fixture schema.
    Parse(String),
    /// The fixture listed no `[[unit]]` rows.
    Empty,
    /// The timestamp is not Europe/Zagreb RFC 3339 with an explicit offset.
    Stamp(String),
    /// One unit row cannot be rendered.
    Unit { name: String, reason: String },
    /// A rendered file could not be written.
    Write { path: String, message: String },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Parse(message) => write!(f, "bad fixture: {message}"),
            RenderError::Empty => write!(f, "fixture has no [[unit]] rows"),
            RenderError::Stamp(stamp) => write!(
                f,
                "timestamp {stamp} is not a Europe/Zagreb RFC 3339 stamp (+01:00 or +02:00)"
            ),
            RenderError::Unit { name, reason } => write!(f, "unit {name}: {reason}"),
            RenderError::Write { path, message } => write!(f, "cannot write {path}: {message}"),
        }
    }
}

impl std::error::Error for RenderError {}

#[derive(Debug, Deserialize)]
struct Fixture {
    #[serde(default)]
    hosts: BTreeMap<String, String>,
    #[serde(default)]
    ports: BTreeMap<String, u16>,
    #[serde(default)]
    paths: BTreeMap<String, String>,
    #[serde(default)]
    unit: Vec<UnitSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitSpec {
    name: String,
    description: String,
    exec_start: String,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    wants: Option<String>,
    #[serde(default)]
    environment: Vec<String>,
    #[serde(default = "restart_always")]
    restart: String,
    #[serde(default = "restart_sec_default")]
    restart_sec: u32,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

fn restart_always() -> String {
    "always".to_string()
}

fn restart_sec_default() -> u32 {
    5
}

fn unit_err(name: &str, reason: impl Into<String>) -> RenderError {
    RenderError::Unit {
        name: name.to_string(),
        reason: reason.into(),
    }
}

fn label_name(name: &str) -> String {
    let trimmed = name.trim();
    if valid_name(trimmed) {
        trimmed.to_string()
    } else {
        "(invalid-name)".to_string()
    }
}

/// `YYYY-MM-DDTHH:MM:SS` plus a Zagreb offset of `+01:00` or `+02:00`.
fn zagreb_stamp_ok(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    if bytes.len() != 25 || stamp.contains('Z') || stamp.contains('z') {
        return false;
    }
    let seps = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'+'
        && bytes[22] == b':';
    if !seps {
        return false;
    }
    const PARTS: [(usize, usize, u32, u32); 6] = [
        (0, 4, 1970, 9999),
        (5, 7, 1, 12),
        (8, 10, 1, 31),
        (11, 13, 0, 23),
        (14, 16, 0, 59),
        (17, 19, 0, 59),
    ];
    for (start, end, min, max) in PARTS {
        let Ok(value) = stamp[start..end].parse::<u32>() else {
            return false;
        };
        if !(min..=max).contains(&value) {
            return false;
        }
    }
    let offset = &stamp[20..22];
    let minutes = &stamp[23..25];
    (offset == "01" || offset == "02") && minutes == "00"
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'.' || *c == b'_' || *c == b'-')
}

fn graphic_line(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && !value.to_ascii_lowercase().contains(FORBIDDEN)
        && value.bytes().all(|c| {
            (c == b' ' || c.is_ascii_graphic())
                && !matches!(c, b'{' | b'}' | b'"' | b'\'' | b'\\' | b'`' | b'$')
        })
}

fn valid_exec(value: &str) -> bool {
    value.starts_with('/')
        && value.len() > 1
        && value.bytes().all(|c| {
            c.is_ascii_graphic() && !matches!(c, b'{' | b'}' | b'"' | b'\'' | b'\\' | b'`' | b'$')
        })
        && !value.to_ascii_lowercase().contains(FORBIDDEN)
}

fn valid_env(value: &str) -> bool {
    let Some((key, raw)) = value.split_once('=') else {
        return false;
    };
    let mut key_bytes = key.bytes();
    let Some(first) = key_bytes.next() else {
        return false;
    };
    if !(first.is_ascii_uppercase() || first == b'_') {
        return false;
    }
    if !key_bytes.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') {
        return false;
    }
    valid_exec_value(raw)
}

fn valid_exec_value(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|c| {
            c.is_ascii_graphic() && !matches!(c, b'{' | b'}' | b'"' | b'\'' | b'\\' | b'`' | b'$')
        })
        && !value.to_ascii_lowercase().contains(FORBIDDEN)
}

fn valid_unit_ref(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b'@'))
        && !value.to_ascii_lowercase().contains(FORBIDDEN)
}

fn lookup(key: &str, fixture: &Fixture) -> Result<String, String> {
    let Some((table, name)) = key.split_once('.') else {
        return Err(format!("placeholder {{{key}}} needs table.name"));
    };
    if table.is_empty() || name.is_empty() {
        return Err(format!("placeholder {{{key}}} is empty"));
    }
    let value = match table {
        "hosts" => fixture.hosts.get(name).cloned(),
        "ports" => fixture.ports.get(name).map(u16::to_string),
        "paths" => fixture.paths.get(name).cloned(),
        _ => return Err(format!("placeholder {{{key}}} has an unknown table")),
    };
    let Some(value) = value else {
        return Err(format!("placeholder {{{key}}} is not in the fixture"));
    };
    if value.is_empty()
        || value
            .bytes()
            .any(|c| c.is_ascii_control() || c == b'{' || c == b'}')
    {
        return Err(format!("placeholder {{{key}}} has an unusable value"));
    }
    Ok(value)
}

fn subst(input: &str, fixture: &Fixture) -> Result<String, String> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err("unclosed placeholder".to_string());
        };
        let key = &after[..end];
        if key.is_empty() {
            return Err("empty placeholder".to_string());
        }
        out.push_str(&lookup(key, fixture)?);
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err("stray closing brace".to_string());
    }
    out.push_str(rest);
    Ok(out)
}

fn resolve(name: &str, input: &str, fixture: &Fixture) -> Result<String, RenderError> {
    subst(input, fixture).map_err(|reason| unit_err(name, reason))
}

fn refuse_forbidden(name: &str, text: &str) -> Result<(), RenderError> {
    if text.to_ascii_lowercase().contains(FORBIDDEN) {
        return Err(unit_err(name, "rendered text contains a forbidden token"));
    }
    if text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "[Install]" || trimmed.starts_with("WantedBy=")
    }) {
        return Err(unit_err(
            name,
            "rendered text must not carry an install section",
        ));
    }
    Ok(())
}

fn render_one(
    spec: &UnitSpec,
    fixture: &Fixture,
    stamp: &str,
) -> Result<RenderedUnit, RenderError> {
    let name = label_name(&spec.name);
    if !valid_name(spec.name.trim()) {
        return Err(unit_err(
            &name,
            "name must be a short ascii token (letters, digits, '.', '_', '-')",
        ));
    }
    let name = spec.name.trim();
    if spec.restart != "always" {
        return Err(unit_err(name, "restart must be always"));
    }
    if !(1..=300).contains(&spec.restart_sec) {
        return Err(unit_err(name, "restart_sec must be 1..=300"));
    }
    let kind = spec.kind.as_deref().unwrap_or("simple");
    if kind != "simple" {
        return Err(unit_err(name, "type must be simple"));
    }

    let description = resolve(name, &spec.description, fixture)?;
    let exec = resolve(name, &spec.exec_start, fixture)?;
    let after = match &spec.after {
        Some(value) => resolve(name, value, fixture)?,
        None => "network-online.target".to_string(),
    };
    let wants = match &spec.wants {
        Some(value) => Some(resolve(name, value, fixture)?),
        None => None,
    };
    let mut environment = Vec::with_capacity(spec.environment.len());
    for entry in &spec.environment {
        environment.push(resolve(name, entry, fixture)?);
    }

    if !graphic_line(&description) {
        return Err(unit_err(name, "description must be one plain line"));
    }
    if !valid_exec(&exec) {
        return Err(unit_err(name, "exec_start must be an absolute path"));
    }
    if !valid_unit_ref(&after) {
        return Err(unit_err(name, "after must be one unit name"));
    }
    if let Some(value) = &wants {
        if !valid_unit_ref(value) {
            return Err(unit_err(name, "wants must be one unit name"));
        }
    }
    for entry in &environment {
        if !valid_env(entry) {
            return Err(unit_err(
                name,
                "environment entries must be KEY=VALUE with no spaces",
            ));
        }
    }

    let mut text = String::new();
    text.push_str("# SPDX-License-Identifier: MIT\n");
    text.push_str("# SPDX-FileCopyrightText: 3000 Galion / galion-studio\n");
    text.push_str("# Rendered: ");
    text.push_str(stamp);
    text.push_str(" Europe/Zagreb\n");
    text.push_str("# Staged and not installed.\n");
    text.push('\n');
    text.push_str("[Unit]\n");
    text.push_str("Description=");
    text.push_str(&description);
    text.push('\n');
    text.push_str("After=");
    text.push_str(&after);
    text.push('\n');
    if let Some(value) = &wants {
        text.push_str("Wants=");
        text.push_str(value);
        text.push('\n');
    }
    text.push('\n');
    text.push_str("[Service]\n");
    text.push_str("Type=simple\n");
    text.push_str("ExecStart=");
    text.push_str(&exec);
    text.push('\n');
    for entry in &environment {
        text.push_str("Environment=");
        text.push_str(entry);
        text.push('\n');
    }
    text.push_str("Restart=always\n");
    text.push_str("RestartSec=");
    text.push_str(&spec.restart_sec.to_string());
    text.push('\n');
    text.push_str("StartLimitIntervalSec=60\n");
    text.push_str("StartLimitBurst=5\n");
    text.push('\n');
    text.push_str("# Staged text only. Not installed. No install section.\n");

    refuse_forbidden(name, &text)?;
    Ok(RenderedUnit {
        name: name.to_string(),
        file_name: format!("{name}.service"),
        text,
    })
}

/// Render every `[[unit]]` row in `toml_text`.
///
/// `rendered_at` is the Europe/Zagreb timestamp written into the comment.
/// It must be RFC 3339 with seconds and an explicit `+01:00` or `+02:00`.
pub fn render_fixture(
    toml_text: &str,
    rendered_at: &str,
) -> Result<Vec<RenderedUnit>, RenderError> {
    if !zagreb_stamp_ok(rendered_at) {
        return Err(RenderError::Stamp(rendered_at.to_string()));
    }
    let fixture: Fixture =
        toml::from_str(toml_text).map_err(|err| RenderError::Parse(err.to_string()))?;
    if fixture.unit.is_empty() {
        return Err(RenderError::Empty);
    }
    let mut out = Vec::with_capacity(fixture.unit.len());
    for spec in &fixture.unit {
        if out
            .iter()
            .any(|unit: &RenderedUnit| unit.name == spec.name.trim())
        {
            return Err(unit_err(spec.name.trim(), "duplicate name"));
        }
        out.push(render_one(spec, &fixture, rendered_at)?);
    }
    Ok(out)
}

/// Same as [`render_fixture`], using [`zagreb_time::now_rfc3339`] as the stamp.
pub fn render_fixture_now(toml_text: &str) -> Result<Vec<RenderedUnit>, RenderError> {
    render_fixture(toml_text, &zagreb_time::now_rfc3339())
}

fn safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.ends_with(".service")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Write each unit as `dir/<file_name>`. Creates `dir` if needed.
///
/// This only writes text files. It does not install them.
pub fn write_rendered(dir: &Path, units: &[RenderedUnit]) -> Result<(), RenderError> {
    for unit in units {
        if !safe_file_name(&unit.file_name) {
            return Err(RenderError::Write {
                path: dir.display().to_string(),
                message: {
                    let file_name = &unit.file_name;
                    format!("refused file name {file_name}")
                },
            });
        }
    }
    fs::create_dir_all(dir).map_err(|err| RenderError::Write {
        path: dir.display().to_string(),
        message: err.to_string(),
    })?;
    for unit in units {
        let path = dir.join(&unit.file_name);
        fs::write(&path, unit.text.as_bytes()).map_err(|err| RenderError::Write {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
    }
    Ok(())
}
