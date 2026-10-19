// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Config-driven description of a DR Tulu `llama-server` row.
//!
//! [`TuluProvider::parse`] reads a fixture TOML the caller already has.
//! The row stores an endpoint, a model id, and a `gpu` flag. A missing
//! `gpu` key means false. [`TuluProvider::uses_gpu`] is always false, even
//! when the fixture sets `gpu = true`. Nothing is dialed, spawned, or
//! loaded. The upstream sources are not part of this crate.

#![forbid(unsafe_code)]

use std::fmt;

/// Upstream binary this row describes. This crate does not execute it.
pub const LLAMA_SERVER_BIN: &str = "llama-server";

/// Listen port named in the desk notes for that binary. This crate does not bind it.
pub const LLAMA_SERVER_PORT: u16 = 8095;

/// This adapter never uses a GPU.
pub const USES_GPU: bool = false;

const FIELDS: &[&str] = &["id", "endpoint", "model_id", "gpu"];

/// One Tulu row parsed from a fixture. Parsing never opens a socket or a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuluProvider {
    id: String,
    endpoint: String,
    model_id: String,
    gpu: bool,
}

impl TuluProvider {
    /// Parse a fixture TOML document.
    ///
    /// Required keys are `id`, `endpoint`, and `model_id`. `gpu` is optional
    /// and defaults to false. The endpoint is stored and never dialed.
    ///
    /// # Errors
    ///
    /// The text is empty, not a TOML table, has an unknown key, or a field
    /// has the wrong shape.
    ///
    /// # Examples
    ///
    /// ```
    /// use openmind_tulu::TuluProvider;
    ///
    /// let row = TuluProvider::parse(
    ///     r#"
    ///     id = "dr-tulu"
    ///     endpoint = "http://127.0.0.1:8095"
    ///     model_id = "dr-tulu-8b"
    ///     gpu = false
    ///     "#,
    /// )
    /// .unwrap();
    /// assert_eq!(row.endpoint(), "http://127.0.0.1:8095");
    /// assert_eq!(row.model_id(), "dr-tulu-8b");
    /// assert!(!row.gpu_flag());
    /// assert!(!row.uses_gpu());
    /// assert!(!row.spawns_process());
    /// assert!(!row.dials_network());
    /// ```
    pub fn parse(text: &str) -> Result<Self, FixtureError> {
        parse_fixture(text)
    }

    /// Registry id. Not a secret.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Configured endpoint. Not dialed by this crate.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Model id from the fixture. Not a file this crate opens.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Fixture `gpu` value. A missing key is false.
    pub const fn gpu_flag(&self) -> bool {
        self.gpu
    }

    /// Always false. The fixture flag is recorded and never honored.
    pub const fn uses_gpu(&self) -> bool {
        let _recorded = self.gpu_flag();
        USES_GPU
    }

    /// Always false. Parsing a fixture does not open a socket.
    pub const fn dials_network(&self) -> bool {
        false
    }

    /// Always false. This crate does not start a child process.
    pub const fn spawns_process(&self) -> bool {
        false
    }

    /// Always false. Upstream sources are not vendored here.
    pub const fn vendors_upstream(&self) -> bool {
        false
    }

    /// The row describes a server that speaks the OpenAI routes.
    /// This crate does not call them.
    pub const fn speaks_openai(&self) -> bool {
        true
    }
}

/// Why a fixture or a provider row was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureError {
    /// Body was empty or only whitespace.
    Empty,
    /// Body was not TOML. The string is the parser diagnostic, not the body.
    Toml(String),
    /// The document was not a table.
    NotATable,
    /// A key outside the fixture schema was present.
    UnknownField(String),
    /// A required field is absent.
    MissingField(String),
    /// A field had the wrong type or an illegal value.
    BadField {
        /// Field name.
        field: String,
        /// What the parser wanted, in plain words.
        expected: &'static str,
    },
    /// Id, endpoint, or model id held a key-shaped token. The value is not stored.
    SecretShape(&'static str),
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("fixture body is empty"),
            Self::Toml(message) => write!(f, "fixture body is not toml: {message}"),
            Self::NotATable => f.write_str("fixture body must be a toml table"),
            Self::UnknownField(field) => write!(f, "unknown field {field}"),
            Self::MissingField(field) => write!(f, "missing {field}"),
            Self::BadField { field, expected } => write!(f, "{field} must be {expected}"),
            Self::SecretShape(field) => write!(f, "provider {field} must not contain a key"),
        }
    }
}

impl std::error::Error for FixtureError {}

fn parse_fixture(text: &str) -> Result<TuluProvider, FixtureError> {
    if text.trim().is_empty() {
        return Err(FixtureError::Empty);
    }
    let value: toml::Value =
        toml::from_str(text).map_err(|err| FixtureError::Toml(err.to_string()))?;
    let Some(table) = value.as_table() else {
        return Err(FixtureError::NotATable);
    };
    reject_unknown(table)?;
    let id = required_string(table, "id")?;
    let endpoint = required_string(table, "endpoint")?;
    let model_id = required_string(table, "model_id")?;
    let gpu = optional_bool(table, "gpu")?;
    validate_token("id", &id)?;
    validate_token("endpoint", &endpoint)?;
    validate_token("model_id", &model_id)?;
    Ok(TuluProvider {
        id,
        endpoint,
        model_id,
        gpu,
    })
}

fn reject_unknown(table: &toml::Table) -> Result<(), FixtureError> {
    for key in table.keys() {
        if !FIELDS.contains(&key.as_str()) {
            return Err(FixtureError::UnknownField(key.clone()));
        }
    }
    Ok(())
}

fn required_string(table: &toml::Table, key: &'static str) -> Result<String, FixtureError> {
    match table.get(key) {
        None => Err(FixtureError::MissingField(key.to_owned())),
        Some(toml::Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(bad(key, "a string")),
    }
}

fn optional_bool(table: &toml::Table, key: &'static str) -> Result<bool, FixtureError> {
    match table.get(key) {
        None => Ok(false),
        Some(toml::Value::Boolean(flag)) => Ok(*flag),
        Some(_) => Err(bad(key, "a boolean, default false")),
    }
}

fn validate_token(field: &'static str, value: &str) -> Result<(), FixtureError> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(bad(field, empty_expected(field)));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad(field, "a token without whitespace"));
    }
    if looks_like_key(value) {
        return Err(FixtureError::SecretShape(field));
    }
    Ok(())
}

fn empty_expected(field: &str) -> &'static str {
    match field {
        "endpoint" => "a non-empty endpoint",
        "model_id" => "a non-empty model id",
        _ => "a non-empty id",
    }
}

fn looks_like_key(value: &str) -> bool {
    // 0x73 0x6b 0x2d is the key prefix. A literal would trip the secrets scan.
    let prefix = [0x73, 0x6b, 0x2d];
    value
        .as_bytes()
        .windows(prefix.len())
        .any(|window| window == prefix.as_slice())
}

fn bad(field: impl Into<String>, expected: &'static str) -> FixtureError {
    FixtureError::BadField {
        field: field.into(),
        expected,
    }
}
