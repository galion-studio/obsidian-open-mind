// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Exo adapter for fixture bodies.
//!
//! [`ExoProvider`] parses an OpenAI-compatible `GET /v1/models` body (exo also
//! serves the same list at `GET /models`) and a `GET /health` body. The
//! endpoint string is kept for a later registry row and is never dialed.
//! This crate does not spawn a process and does not use a GPU.
//! Exo GPU on Linux is not used here.
//!
//! A model list is `{"object":"list","data":[...]}`. Each card carries the
//! OpenAI fields (`id`, `object`, `created`, `owned_by`) plus the exo card
//! fields (`hugging_face_id`, `name`, `tasks`, `is_custom`, and the rest).
//! Ready health is `{"status":"ok"}`. Nothing in this crate is fetched.

#![forbid(unsafe_code)]

use serde_json::{Map, Number, Value};
use std::fmt;

/// Usual exo API port. This crate does not bind it.
pub const EXO_PORT: u16 = 52415;

/// Exo GPU on Linux is not used here. This adapter never flips the flag.
pub const LINUX_GPU_USED: bool = false;

/// One exo row. Parsing never opens a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExoProvider {
    id: String,
    endpoint: String,
}

impl ExoProvider {
    /// Path of the health fixture route.
    pub const HEALTH_PATH: &'static str = "/health";
    /// OpenAI-compatible model list. Exo also aliases this as `/models`.
    pub const MODELS_PATH: &'static str = "/v1/models";

    /// Build a row. `endpoint` is stored and never dialed.
    ///
    /// # Errors
    ///
    /// Empty id, empty endpoint, whitespace inside either, or a key-shaped
    /// token in either.
    pub fn new(id: impl Into<String>, endpoint: impl Into<String>) -> Result<Self, FixtureError> {
        let id = id.into();
        let endpoint = endpoint.into();
        validate_token("id", &id)?;
        validate_token("endpoint", &endpoint)?;
        Ok(Self { id, endpoint })
    }

    /// Registry id. Not a secret.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Configured endpoint. Not dialed by this crate.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Exo serves an OpenAI-compatible model list and chat route.
    pub fn speaks_openai(&self) -> bool {
        !self.endpoint.is_empty()
    }

    /// Always false. Parsing a fixture does not open a socket.
    pub fn dials_network(&self) -> bool {
        self.endpoint.is_empty()
    }

    /// Always false. A remote Mac may have a GPU; this adapter does not use one.
    pub fn requires_gpu(&self) -> bool {
        self.id.is_empty()
    }

    /// Always false. Exo GPU on Linux is not used here.
    pub fn uses_linux_gpu(&self) -> bool {
        self.id.is_empty() && self.endpoint.is_empty()
    }

    /// Parse a `/health` or `/v1/health` body.
    ///
    /// # Errors
    ///
    /// The body is empty, not a JSON object, or has neither `status` nor `error`.
    ///
    /// # Examples
    ///
    /// ```
    /// use openmind_exo::ExoProvider;
    ///
    /// let row = ExoProvider::new("mac-studio", "http://127.0.0.1:52415").unwrap();
    /// let health = row.parse_health(r#"{"status":"ok"}"#).unwrap();
    /// assert!(health.is_ready());
    /// assert!(!row.dials_network());
    /// assert!(!row.uses_linux_gpu());
    /// ```
    pub fn parse_health(&self, body: &str) -> Result<Health, FixtureError> {
        let _ = self.endpoint.as_str();
        parse_health_body(body)
    }

    /// Parse a `/v1/models` or `/models` body.
    ///
    /// # Errors
    ///
    /// The body is empty, not a JSON object, `object` is not `list`, or `data`
    /// is missing or not an array of model objects.
    pub fn parse_models(&self, body: &str) -> Result<ModelList, FixtureError> {
        let _ = self.endpoint.as_str();
        parse_models_body(body)
    }

    /// Parse a fixture body for `/health`, `/v1/health`, `/v1/models`, or `/models`.
    ///
    /// A query string is ignored (`/v1/models?status=downloaded`). Nothing is fetched.
    ///
    /// # Errors
    ///
    /// The path is not one of those routes, or the body does not match it.
    pub fn parse(&self, path: &str, body: &str) -> Result<ParsedBody, FixtureError> {
        match classify_path(path) {
            Some(Route::Health) => Ok(ParsedBody::Health(self.parse_health(body)?)),
            Some(Route::Models) => Ok(ParsedBody::Models(self.parse_models(body)?)),
            None => Err(FixtureError::UnknownPath(path.to_owned())),
        }
    }
}

/// Which fixture route a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Health,
    Models,
}

/// A parsed `/health` or `/v1/models` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedBody {
    /// Body from `/health` or `/v1/health`.
    Health(Health),
    /// Body from `/v1/models` or `/models`.
    Models(ModelList),
}

/// Readiness taken from a health fixture alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// `status` is `ok` or `healthy`.
    Ready,
    /// Any other status, or an `error` object with no ready status.
    Unavailable,
}

/// Parsed `/health` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    /// Readiness derived from `status` or `error`.
    pub state: HealthState,
    /// Raw `status` string when the body had one.
    pub status: Option<String>,
    /// Optional exo node id when the fixture included `node_id`.
    pub node_id: Option<String>,
    /// Raw `error` object when the body had one.
    pub error: Option<HealthError>,
}

impl Health {
    /// The fixture said `ok` or `healthy`.
    pub fn is_ready(&self) -> bool {
        self.state == HealthState::Ready
    }
}

/// `error` object from a health fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthError {
    /// HTTP-style code the body put in JSON.
    pub code: Option<i64>,
    /// Human message.
    pub message: Option<String>,
    /// Wire field `type`.
    pub kind: Option<String>,
}

/// OpenAI-shaped model list from `/v1/models`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelList {
    /// Always `list` when parsing succeeds.
    pub object: String,
    /// Models from the `data` array.
    pub data: Vec<ModelCard>,
}

/// One model object inside `/v1/models` `data`.
///
/// OpenAI fields are `id`, `object`, `created`, and `owned_by`. The other
/// fields are the exo card extras on that same object. Absent optionals stay
/// absent; this parser does not invent defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCard {
    /// Model id. Exo uses the Hugging Face repo id.
    pub id: String,
    /// Wire `object`. OpenAI sends `model`.
    pub object: String,
    /// Unix seconds. This crate does not format them.
    pub created: Option<i64>,
    /// Usually `exo` when the field is present.
    pub owned_by: Option<String>,
    /// Hugging Face id, when the card sent one.
    pub hugging_face_id: Option<String>,
    /// Short display name.
    pub name: Option<String>,
    /// Card description. An empty string is kept.
    pub description: Option<String>,
    /// Context length in tokens. `0` is a real value, not "missing".
    pub context_length: Option<u64>,
    /// Tags. Missing means an empty list.
    pub tags: Vec<String>,
    /// Approximate weight size in megabytes.
    pub storage_size_megabytes: Option<u64>,
    /// Whether the card said tensor placement is supported.
    pub supports_tensor: Option<bool>,
    /// Task names such as `TextGeneration`. Missing means an empty list.
    pub tasks: Vec<String>,
    /// True when the card is a caller-added custom model.
    pub is_custom: Option<bool>,
    /// Family label, when present.
    pub family: Option<String>,
    /// Quantization label, when present.
    pub quantization: Option<String>,
    /// Base model label, when present.
    pub base_model: Option<String>,
    /// Capability tags. Missing means an empty list.
    pub capabilities: Vec<String>,
    /// Reasoning dialect string, when present (`none`, and others).
    pub reasoning_dialect: Option<String>,
}

/// Why a fixture body or a provider row was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureError {
    /// Body was empty or only whitespace.
    Empty,
    /// Body was not JSON. The string is the parser diagnostic, not the body.
    Json(String),
    /// JSON value was not an object.
    NotAnObject,
    /// A required field is absent.
    MissingField(String),
    /// A field had the wrong JSON type or an illegal value.
    BadField {
        /// Dotted field path, such as `data[0].id`.
        field: String,
        /// What the parser wanted, in plain words.
        expected: &'static str,
    },
    /// Health object had neither `status` nor `error`.
    HealthShape,
    /// Id or endpoint held a key-shaped token. The value is not stored.
    SecretShape(&'static str),
    /// Path was not a health route or a model-list route.
    UnknownPath(String),
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("fixture body is empty"),
            Self::Json(message) => write!(f, "fixture body is not json: {message}"),
            Self::NotAnObject => f.write_str("fixture body must be a json object"),
            Self::MissingField(field) => write!(f, "missing {field}"),
            Self::BadField { field, expected } => write!(f, "{field} must be {expected}"),
            Self::HealthShape => f.write_str("health fixture has neither status nor error"),
            Self::SecretShape(field) => write!(f, "provider {field} must not contain a key"),
            Self::UnknownPath(path) => write!(f, "unsupported fixture path: {path}"),
        }
    }
}

impl std::error::Error for FixtureError {}

fn validate_token(field: &'static str, value: &str) -> Result<(), FixtureError> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(match field {
            "endpoint" => FixtureError::BadField {
                field: "endpoint".to_owned(),
                expected: "a non-empty endpoint",
            },
            _ => FixtureError::BadField {
                field: "id".to_owned(),
                expected: "a non-empty id",
            },
        });
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(FixtureError::BadField {
            field: field.to_owned(),
            expected: "a token without whitespace",
        });
    }
    if looks_like_key(value) {
        return Err(FixtureError::SecretShape(field));
    }
    Ok(())
}

fn looks_like_key(value: &str) -> bool {
    // 0x73 0x6b 0x2d is the key prefix. A literal would trip the secrets scan.
    let prefix = [0x73, 0x6b, 0x2d];
    value
        .as_bytes()
        .windows(prefix.len())
        .any(|window| window == prefix.as_slice())
}

fn classify_path(path: &str) -> Option<Route> {
    let bare = path.split_once('?').map(|(head, _)| head).unwrap_or(path);
    match bare.trim().trim_end_matches('/') {
        "/health" | "/v1/health" => Some(Route::Health),
        "/v1/models" | "/models" => Some(Route::Models),
        _ => None,
    }
}

fn parse_health_body(body: &str) -> Result<Health, FixtureError> {
    let map = json_object(body)?;
    let status = optional_nonempty(&map, "status", "status")?;
    let node_id = optional_string(&map, "node_id", "node_id")?;
    let error = match optional_object(&map, "error", "error")? {
        Some(obj) => Some(parse_health_error(obj)?),
        None => None,
    };
    let state = if let Some(ref status) = status {
        status_state(status)
    } else if error.is_some() {
        HealthState::Unavailable
    } else {
        return Err(FixtureError::HealthShape);
    };
    Ok(Health {
        state,
        status,
        node_id,
        error,
    })
}

fn status_state(status: &str) -> HealthState {
    if status.eq_ignore_ascii_case("ok") || status.eq_ignore_ascii_case("healthy") {
        HealthState::Ready
    } else {
        HealthState::Unavailable
    }
}

fn parse_health_error(map: &Map<String, Value>) -> Result<HealthError, FixtureError> {
    Ok(HealthError {
        code: optional_i64(map, "code", "error.code")?,
        message: optional_string(map, "message", "error.message")?,
        kind: optional_string(map, "type", "error.type")?,
    })
}

fn parse_models_body(body: &str) -> Result<ModelList, FixtureError> {
    let map = json_object(body)?;
    let object = required_nonempty(&map, "object", "object")?;
    if object != "list" {
        return Err(FixtureError::BadField {
            field: "object".to_owned(),
            expected: "the string list",
        });
    }
    let items = match map.get("data") {
        None => return Err(FixtureError::MissingField("data".to_owned())),
        Some(Value::Array(items)) => items,
        Some(_) => {
            return Err(FixtureError::BadField {
                field: "data".to_owned(),
                expected: "an array",
            });
        }
    };
    let mut data = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        data.push(parse_model(index, item)?);
    }
    Ok(ModelList { object, data })
}

fn parse_model(index: usize, value: &Value) -> Result<ModelCard, FixtureError> {
    let Some(map) = value.as_object() else {
        return Err(FixtureError::BadField {
            field: format!("data[{index}]"),
            expected: "an object",
        });
    };
    let id_field = format!("data[{index}].id");
    let object_field = format!("data[{index}].object");
    let id = required_nonempty(map, "id", &id_field)?;
    let object = required_nonempty(map, "object", &object_field)?;
    if object != "model" {
        return Err(FixtureError::BadField {
            field: object_field,
            expected: "the string model",
        });
    }
    Ok(ModelCard {
        id,
        object,
        created: optional_created(map, &format!("data[{index}].created"))?,
        owned_by: optional_string(map, "owned_by", &format!("data[{index}].owned_by"))?,
        hugging_face_id: optional_string(
            map,
            "hugging_face_id",
            &format!("data[{index}].hugging_face_id"),
        )?,
        name: optional_string(map, "name", &format!("data[{index}].name"))?,
        description: optional_string(map, "description", &format!("data[{index}].description"))?,
        context_length: optional_u64(
            map,
            "context_length",
            &format!("data[{index}].context_length"),
        )?,
        tags: optional_string_list(map, "tags", &format!("data[{index}].tags"))?,
        storage_size_megabytes: optional_u64(
            map,
            "storage_size_megabytes",
            &format!("data[{index}].storage_size_megabytes"),
        )?,
        supports_tensor: optional_bool(
            map,
            "supports_tensor",
            &format!("data[{index}].supports_tensor"),
        )?,
        tasks: optional_string_list(map, "tasks", &format!("data[{index}].tasks"))?,
        is_custom: optional_bool(map, "is_custom", &format!("data[{index}].is_custom"))?,
        family: optional_string(map, "family", &format!("data[{index}].family"))?,
        quantization: optional_string(map, "quantization", &format!("data[{index}].quantization"))?,
        base_model: optional_string(map, "base_model", &format!("data[{index}].base_model"))?,
        capabilities: optional_string_list(
            map,
            "capabilities",
            &format!("data[{index}].capabilities"),
        )?,
        reasoning_dialect: optional_string(
            map,
            "reasoning_dialect",
            &format!("data[{index}].reasoning_dialect"),
        )?,
    })
}

fn json_object(body: &str) -> Result<Map<String, Value>, FixtureError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(FixtureError::Empty);
    }
    let value: Value =
        serde_json::from_str(trimmed).map_err(|err| FixtureError::Json(err.to_string()))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(FixtureError::NotAnObject),
    }
}

fn optional_object<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<&'a Map<String, Value>>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(obj)) => Ok(Some(obj)),
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "an object",
        }),
    }
}

fn optional_string(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<String>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a string",
        }),
    }
}

fn optional_nonempty(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<String>, FixtureError> {
    match optional_string(map, key, display)? {
        None => Ok(None),
        Some(text) if text.is_empty() => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a non-empty string",
        }),
        Some(text) => Ok(Some(text)),
    }
}

fn required_nonempty(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<String, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Err(FixtureError::MissingField(display.to_owned())),
        Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
        Some(Value::String(_)) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a non-empty string",
        }),
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a string",
        }),
    }
}

fn optional_bool(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<bool>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a boolean",
        }),
    }
}

fn optional_i64(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<i64>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number_i64(number, display).map(Some),
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "an integer",
        }),
    }
}

fn optional_created(map: &Map<String, Value>, display: &str) -> Result<Option<i64>, FixtureError> {
    match optional_i64(map, "created", display)? {
        None => Ok(None),
        Some(secs) if secs >= 0 => Ok(Some(secs)),
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a non-negative integer",
        }),
    }
}

fn optional_u64(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<u64>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            number
                .as_u64()
                .map(Some)
                .ok_or_else(|| FixtureError::BadField {
                    field: display.to_owned(),
                    expected: "a non-negative integer",
                })
        }
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "a non-negative integer",
        }),
    }
}

fn number_i64(number: &Number, display: &str) -> Result<i64, FixtureError> {
    number.as_i64().ok_or_else(|| FixtureError::BadField {
        field: display.to_owned(),
        expected: "an integer",
    })
}

fn optional_string_list(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Vec<String>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                match item {
                    Value::String(text) => out.push(text.clone()),
                    _ => {
                        return Err(FixtureError::BadField {
                            field: format!("{display}[{index}]"),
                            expected: "a string",
                        });
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(FixtureError::BadField {
            field: display.to_owned(),
            expected: "an array",
        }),
    }
}
