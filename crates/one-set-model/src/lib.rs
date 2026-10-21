// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Validate a lane and model the way `set-model.py` does, then return the
//! JSON object that helper would write. This crate never writes a file.
//!
//! Lane must be `plan` or `action`. The model must contain `/`.
//! Other top-level keys are kept. `modes` is reduced to `plan` and `action`.

use serde_json::{Map, Value};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// Why a lane update was refused, or why the modes document could not be read.
#[derive(Debug)]
pub enum SetModelError {
    /// `lane` is not exactly `plan` or `action`.
    Lane,
    /// `model` does not contain `/`.
    Model,
    /// The document root is not a JSON object.
    NotObject,
    /// `modes` is present and is not a JSON object.
    ModesNotObject,
    /// Reading the modes file failed.
    Io(io::Error),
    /// The modes file is not JSON.
    Json(serde_json::Error),
}

impl fmt::Display for SetModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lane => write!(f, "lane must be plan or action"),
            Self::Model => write!(f, "model must contain a slash"),
            Self::NotObject => write!(f, "modes document must be a JSON object"),
            Self::ModesNotObject => write!(f, "modes must be a JSON object"),
            Self::Io(err) => write!(f, "read modes file: {err}"),
            Self::Json(err) => write!(f, "parse modes file: {err}"),
        }
    }
}

impl std::error::Error for SetModelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Lane | Self::Model | Self::NotObject | Self::ModesNotObject => None,
        }
    }
}

impl From<io::Error> for SetModelError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for SetModelError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

/// Check lane and model. Does not read or write a file.
///
/// `lane` must be `plan` or `action`. `model` must contain `/`.
/// Lane is checked first.
pub fn validate(lane: &str, model: &str) -> Result<(), SetModelError> {
    if !matches!(lane, "plan" | "action") {
        return Err(SetModelError::Lane);
    }
    if !model.contains('/') {
        return Err(SetModelError::Model);
    }
    Ok(())
}

/// JSON object `set-model.py` would write for `document`.
///
/// Does not write. `document` must be an object. A missing `modes` object is
/// treated as empty. The lane that is not being set keeps its previous value,
/// or `""` when that key is missing. Keys inside `modes` other than `plan`
/// and `action` are dropped. Every other top-level key is copied unchanged.
pub fn apply(document: &Value, lane: &str, model: &str) -> Result<Value, SetModelError> {
    validate(lane, model)?;
    let Some(root) = document.as_object() else {
        return Err(SetModelError::NotObject);
    };
    let previous = match root.get("modes") {
        None => None,
        Some(Value::Object(map)) => Some(map),
        Some(_) => return Err(SetModelError::ModesNotObject),
    };
    let plan = lane_value(previous, "plan", lane, model);
    let action = lane_value(previous, "action", lane, model);
    let mut modes = Map::new();
    modes.insert("plan".to_string(), plan);
    modes.insert("action".to_string(), action);
    let mut out = root.clone();
    out.insert("modes".to_string(), Value::Object(modes));
    Ok(Value::Object(out))
}

fn lane_value(previous: Option<&Map<String, Value>>, key: &str, lane: &str, model: &str) -> Value {
    if lane == key {
        Value::String(model.to_string())
    } else {
        match previous.and_then(|map| map.get(key)) {
            Some(value) => value.clone(),
            None => Value::String(String::new()),
        }
    }
}

/// Read `path` and return the JSON object that would be written.
///
/// The file is only read. This function does not create, truncate, or write
/// any path, including a live modes file.
pub fn apply_file(path: &Path, lane: &str, model: &str) -> Result<Value, SetModelError> {
    validate(lane, model)?;
    let text = fs::read_to_string(path)?;
    let document: Value = serde_json::from_str(&text)?;
    apply(&document, lane, model)
}

/// Pretty JSON plus a trailing newline.
///
/// `set-model.py` uses `json.dumps(..., indent=2)` plus a newline. Key order
/// is the document order, like Python: serde_json `preserve_order` is pinned
/// for the whole workspace (root Cargo.toml, swarm/serde-order-pin), so a
/// `-p one-set-model` build no longer sorts the keys.
pub fn render(document: &Value) -> Result<String, serde_json::Error> {
    let mut text = serde_json::to_string_pretty(document)?;
    text.push('\n');
    Ok(text)
}
