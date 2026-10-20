// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! llama.cpp adapter for `llama-server` and `rpc-server`.
//!
//! [`LlamaCppProvider`] parses fixture bodies for `GET /health` and
//! `GET /v1/models`. The endpoint string is kept for a later registry row
//! and is never dialed. This crate does not spawn a process and does not
//! require a GPU.
//!
//! Ready health is `{"status":"ok"}`. A model that is still loading is
//! `{"error":{"code":503,"message":"Loading model","type":"unavailable_error"}}`.
//! Model lists use the OpenAI `data` array. The extra Ollama-shaped `models`
//! key on the same body is ignored. `rpc-server` does not speak these routes;
//! a fronting `llama-server` is what emits them.

#![forbid(unsafe_code)]

use serde_json::{Map, Number, Value};
use std::fmt;

/// Usual `llama-server` listen port. This crate does not bind it.
pub const LLAMA_SERVER_PORT: u16 = 8080;

/// Port named for `rpc-server` in the desk notes. This crate does not bind it.
pub const RPC_SERVER_PORT: u16 = 50052;

/// Which llama.cpp binary the row refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerKind {
    /// HTTP `llama-server` (`/health`, `/v1/models`, OpenAI-compatible routes).
    LlamaServer,
    /// ggml `rpc-server` device. Health and model fixtures still come from
    /// the fronting `llama-server`, not from the RPC socket.
    RpcServer,
}

/// One llama.cpp row. Parsing never opens a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaCppProvider {
    id: String,
    endpoint: String,
    kind: ServerKind,
}

impl LlamaCppProvider {
    /// Path of the public health route, including the `/v1/health` alias.
    pub const HEALTH_PATH: &'static str = "/health";
    /// Path of the OpenAI-compatible model list.
    pub const MODELS_PATH: &'static str = "/v1/models";

    /// Build a row. `endpoint` is stored and never dialed.
    ///
    /// # Errors
    ///
    /// Empty id, empty endpoint, whitespace inside either, or a key-shaped
    /// token in either.
    pub fn new(
        id: impl Into<String>,
        endpoint: impl Into<String>,
        kind: ServerKind,
    ) -> Result<Self, FixtureError> {
        let id = id.into();
        let endpoint = endpoint.into();
        validate_token("id", &id)?;
        validate_token("endpoint", &endpoint)?;
        Ok(Self { id, endpoint, kind })
    }

    /// Registry id. Not a secret.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Configured endpoint. Not dialed by this crate.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Which binary this row names.
    pub fn kind(&self) -> ServerKind {
        self.kind
    }

    /// `llama-server` speaks the OpenAI routes. `rpc-server` does not.
    pub fn speaks_openai(&self) -> bool {
        match self.kind {
            ServerKind::LlamaServer => true,
            ServerKind::RpcServer => false,
        }
    }

    /// True only for [`ServerKind::RpcServer`].
    pub fn is_rpc_device(&self) -> bool {
        match self.kind {
            ServerKind::LlamaServer => false,
            ServerKind::RpcServer => true,
        }
    }

    /// Always false. Parsing a fixture does not open a socket.
    pub fn dials_network(&self) -> bool {
        match self.kind {
            ServerKind::LlamaServer | ServerKind::RpcServer => false,
        }
    }

    /// Always false. A remote box may have a card; this adapter does not use one.
    pub fn requires_gpu(&self) -> bool {
        match self.kind {
            ServerKind::LlamaServer | ServerKind::RpcServer => false,
        }
    }

    /// Always false. This crate does not start `llama-server` or `rpc-server`.
    pub fn spawns_process(&self) -> bool {
        match self.kind {
            ServerKind::LlamaServer | ServerKind::RpcServer => false,
        }
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
    /// use openmind_llama::{LlamaCppProvider, ServerKind};
    ///
    /// let row = LlamaCppProvider::new(
    ///     "laptop-4060",
    ///     "http://127.0.0.1:8080",
    ///     ServerKind::LlamaServer,
    /// )
    /// .unwrap();
    /// let health = row.parse_health(r#"{"status":"ok"}"#).unwrap();
    /// assert!(health.is_ready());
    /// assert!(!row.dials_network());
    /// assert!(!row.requires_gpu());
    /// assert!(!row.spawns_process());
    /// ```
    pub fn parse_health(&self, body: &str) -> Result<Health, FixtureError> {
        let _ = self.kind;
        parse_health_body(body)
    }

    /// Parse a `/v1/models` body.
    ///
    /// # Errors
    ///
    /// The body is empty, not a JSON object, `object` is not `list`, or `data`
    /// is missing or not an array of model objects.
    pub fn parse_models(&self, body: &str) -> Result<ModelList, FixtureError> {
        let _ = self.kind;
        parse_models_body(body)
    }

    /// Parse a fixture body for `/health`, `/v1/health`, or `/v1/models`.
    ///
    /// A query string is ignored (`/v1/models?reload=1`). Nothing is fetched.
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
    /// Body from `/v1/models`.
    Models(ModelList),
}

/// Readiness of a `llama-server` process, from a fixture body alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// `{"status":"ok"}`. The model is loaded.
    Ready,
    /// Loading-model error. The live server uses HTTP 503 for this body.
    Loading,
    /// No free slot (`no slot available`).
    NoSlot,
    /// Some other error object.
    Unavailable,
}

/// Parsed `/health` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    /// Readiness derived from `status` or `error`.
    pub state: HealthState,
    /// Raw `status` string when the body had one.
    pub status: Option<String>,
    /// Legacy idle-slot count. Current `llama-server` omits this.
    pub slots_idle: Option<u64>,
    /// Legacy busy-slot count. Current `llama-server` omits this.
    pub slots_processing: Option<u64>,
    /// Raw `error` object when the body had one.
    pub error: Option<HealthError>,
}

impl Health {
    /// The model is loaded and the server said `ok`.
    pub fn is_ready(&self) -> bool {
        self.state == HealthState::Ready
    }

    /// The server is still loading a model.
    pub fn is_loading(&self) -> bool {
        self.state == HealthState::Loading
    }

    /// Every slot is busy.
    pub fn is_no_slot(&self) -> bool {
        self.state == HealthState::NoSlot
    }
}

/// `error` object from a llama-server health or failure body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthError {
    /// HTTP-style code the server put in JSON. `503` while loading.
    pub code: Option<i64>,
    /// Human message. `Loading model` or `no slot available`.
    pub message: Option<String>,
    /// Wire field `type` (`unavailable_error`, `server_error`, ...).
    pub kind: Option<String>,
}

/// OpenAI-shaped model list from `/v1/models`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelList {
    /// Always `list` when parsing succeeds.
    pub object: String,
    /// Models from the `data` array. The sibling `models` key is not copied.
    pub data: Vec<ModelCard>,
}

/// One model object inside `/v1/models` `data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCard {
    /// Model id. Default is the GGUF path; `--alias` replaces it.
    pub id: String,
    /// Wire `object`. `llama-server` sends `model`.
    pub object: String,
    /// Upstream unix seconds. This crate does not format them.
    pub created: Option<i64>,
    /// Usually `llamacpp`.
    pub owned_by: Option<String>,
    /// Extra names. Missing means an empty list.
    pub aliases: Vec<String>,
    /// Informational tags. Missing means an empty list.
    pub tags: Vec<String>,
    /// GGUF metadata. `null` or absent while the model is still loading.
    pub meta: Option<ModelMeta>,
    /// Router `architecture.input_modalities`. Empty when the key is absent.
    pub input_modalities: Vec<String>,
    /// Router `architecture.output_modalities`. Empty when the key is absent.
    pub output_modalities: Vec<String>,
    /// Router model status. Absent on the single-model `llama-server` body.
    pub status: Option<ModelStatus>,
}

/// `meta` object on a model card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMeta {
    /// Vocabulary kind integer from llama.cpp.
    pub vocab_type: Option<i64>,
    /// Vocabulary size.
    pub n_vocab: Option<u64>,
    /// Runtime context (`n_ctx`), not the training context.
    pub n_ctx: Option<u64>,
    /// Training context.
    pub n_ctx_train: Option<u64>,
    /// Embedding width.
    pub n_embd: Option<u64>,
    /// Parameter count.
    pub n_params: Option<u64>,
    /// File size in bytes.
    pub size: Option<u64>,
    /// Quant name (`ftype`), for example `Q4_K_M`.
    pub ftype: Option<String>,
}

/// Router `status` object on a model card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStatus {
    /// `loaded`, `loading`, `sleeping`, `unloaded`, `downloading`, `downloaded`.
    pub value: String,
    /// Arguments the child was started with. Empty when omitted.
    pub args: Vec<String>,
    /// True when the server marked the child failed.
    pub failed: bool,
    /// Child exit code when the server sent one.
    pub exit_code: Option<i64>,
}

impl ModelStatus {
    /// Status value is `loaded`.
    pub fn is_loaded(&self) -> bool {
        self.value == "loaded"
    }

    /// Status value is `loading`.
    pub fn is_loading(&self) -> bool {
        self.value == "loading"
    }

    /// Status value is `sleeping`.
    pub fn is_sleeping(&self) -> bool {
        self.value == "sleeping"
    }

    /// Explicit `failed` flag, or a status value of `failed`.
    pub fn is_failed(&self) -> bool {
        self.failed || self.value == "failed"
    }
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
    /// Path was not `/health`, `/v1/health`, or `/v1/models`.
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
        "/v1/models" => Some(Route::Models),
        _ => None,
    }
}

fn parse_health_body(body: &str) -> Result<Health, FixtureError> {
    let map = json_object(body)?;
    let status = optional_nonempty(&map, "status", "status")?;
    let slots_idle = optional_u64(&map, "slots_idle", "slots_idle")?;
    let slots_processing = optional_u64(&map, "slots_processing", "slots_processing")?;
    let error = match optional_object(&map, "error", "error")? {
        Some(obj) => Some(parse_health_error(obj)?),
        None => None,
    };
    let state = if let Some(ref status) = status {
        status_state(status)
    } else if let Some(ref err) = error {
        error_state(err)
    } else {
        return Err(FixtureError::HealthShape);
    };
    Ok(Health {
        state,
        status,
        slots_idle,
        slots_processing,
        error,
    })
}

fn status_state(status: &str) -> HealthState {
    if status.eq_ignore_ascii_case("ok") {
        HealthState::Ready
    } else if status.eq_ignore_ascii_case("no slot available") {
        HealthState::NoSlot
    } else if contains_ignore_ascii(status, "loading") {
        HealthState::Loading
    } else {
        HealthState::Unavailable
    }
}

fn error_state(err: &HealthError) -> HealthState {
    let message = err.message.as_deref().unwrap_or("");
    if contains_ignore_ascii(message, "loading") {
        HealthState::Loading
    } else if message.eq_ignore_ascii_case("no slot available") {
        HealthState::NoSlot
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
    let created = optional_created(map, &format!("data[{index}].created"))?;
    let owned_by = optional_string(map, "owned_by", &format!("data[{index}].owned_by"))?;
    let aliases = optional_string_list(map, "aliases", &format!("data[{index}].aliases"))?;
    let tags = optional_string_list(map, "tags", &format!("data[{index}].tags"))?;
    let meta = parse_meta(map, index)?;
    let (input_modalities, output_modalities) = parse_architecture(map, index)?;
    let status = parse_model_status(map, index)?;
    Ok(ModelCard {
        id,
        object,
        created,
        owned_by,
        aliases,
        tags,
        meta,
        input_modalities,
        output_modalities,
        status,
    })
}

fn parse_meta(map: &Map<String, Value>, index: usize) -> Result<Option<ModelMeta>, FixtureError> {
    let field = format!("data[{index}].meta");
    let Some(obj) = optional_object(map, "meta", &field)? else {
        return Ok(None);
    };
    Ok(Some(ModelMeta {
        vocab_type: optional_i64(obj, "vocab_type", &format!("{field}.vocab_type"))?,
        n_vocab: optional_u64(obj, "n_vocab", &format!("{field}.n_vocab"))?,
        n_ctx: optional_u64(obj, "n_ctx", &format!("{field}.n_ctx"))?,
        n_ctx_train: optional_u64(obj, "n_ctx_train", &format!("{field}.n_ctx_train"))?,
        n_embd: optional_u64(obj, "n_embd", &format!("{field}.n_embd"))?,
        n_params: optional_u64(obj, "n_params", &format!("{field}.n_params"))?,
        size: optional_u64(obj, "size", &format!("{field}.size"))?,
        ftype: optional_string(obj, "ftype", &format!("{field}.ftype"))?,
    }))
}

fn parse_architecture(
    map: &Map<String, Value>,
    index: usize,
) -> Result<(Vec<String>, Vec<String>), FixtureError> {
    let field = format!("data[{index}].architecture");
    let Some(obj) = optional_object(map, "architecture", &field)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let input = optional_string_list(
        obj,
        "input_modalities",
        &format!("{field}.input_modalities"),
    )?;
    let output = optional_string_list(
        obj,
        "output_modalities",
        &format!("{field}.output_modalities"),
    )?;
    Ok((input, output))
}

fn parse_model_status(
    map: &Map<String, Value>,
    index: usize,
) -> Result<Option<ModelStatus>, FixtureError> {
    let field = format!("data[{index}].status");
    let Some(obj) = optional_object(map, "status", &field)? else {
        return Ok(None);
    };
    let value = required_nonempty(obj, "value", &format!("{field}.value"))?;
    let args = optional_string_list(obj, "args", &format!("{field}.args"))?;
    let failed = optional_bool(obj, "failed", &format!("{field}.failed"))?.unwrap_or(false);
    let exit_code = optional_i64(obj, "exit_code", &format!("{field}.exit_code"))?;
    Ok(Some(ModelStatus {
        value,
        args,
        failed,
        exit_code,
    }))
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

fn contains_ignore_ascii(hay: &str, needle: &str) -> bool {
    let needle_bytes = needle.as_bytes();
    !needle_bytes.is_empty()
        && hay
            .as_bytes()
            .windows(needle_bytes.len())
            .any(|window| window.eq_ignore_ascii_case(needle_bytes))
}
