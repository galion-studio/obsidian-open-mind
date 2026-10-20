// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only stdio MCP stub.
//!
//! DESK-ID: openmind.B57
//!
//! [`StackServer`] answers `initialize` and `tools/list` for one tool,
//! `stack_status`. Calling that tool returns fixture JSON: the crate names
//! this server was given, plus one Europe/Zagreb timestamp. The server reads
//! no files, does not open a foreign install tree, does not open a state
//! database, and does not listen on a TCP port.
//!
//! Tests pass stdin bytes to [`StackServer::handle_bytes`] and read the
//! response bytes back. That path does not spawn a process.

#![forbid(unsafe_code)]

use serde_json::{json, Value};
use std::fmt;

/// MCP server name advertised in `initialize`.
pub const SERVER_NAME: &str = "openmind-mcp-stack";

/// MCP server version advertised in `initialize`.
pub const SERVER_VERSION: &str = "0.1.0";

/// The only tool this stub lists or calls.
pub const TOOL_NAME: &str = "stack_status";

/// Protocol versions this stub will echo. Anything else is answered with
/// [`DEFAULT_PROTOCOL`].
pub const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Version used when the client omits one, or asks for a version we do not speak.
pub const DEFAULT_PROTOCOL: &str = "2024-11-05";

const INSTRUCTIONS: &str = concat!(
    "Read-only stub. stack_status returns the crate names this process was given ",
    "and one Europe/Zagreb timestamp. No files and no sockets."
);

const TOOL_DESCRIPTION: &str = concat!(
    "Read-only fixture: crate names this server was given, ",
    "plus one Europe/Zagreb timestamp. Opens no files and no sockets."
);

/// One read-only stub.
///
/// Crate names and the timestamp are fixed at construction so a test can
/// inject a fixture clock. Nothing here is read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackServer {
    crates: Vec<String>,
    observed_at: String,
}

impl StackServer {
    /// Build a server.
    ///
    /// The timestamp is the current Europe/Zagreb instant from `zagreb-time`
    /// (RFC 3339, explicit offset, never `Z`).
    ///
    /// # Errors
    ///
    /// A crate name is empty or is not a plain crate token.
    pub fn new(crates: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self, StackError> {
        Self::with_timestamp(crates, zagreb_time::now_rfc3339())
    }

    /// Build a server with a fixture timestamp.
    ///
    /// `observed_at` must be RFC 3339 seconds with a Zagreb offset (`+01:00`
    /// or `+02:00`). A `Z` suffix is rejected.
    ///
    /// # Errors
    ///
    /// A crate name is not a plain token, or the timestamp is not a Zagreb stamp.
    pub fn with_timestamp(
        crates: impl IntoIterator<Item = impl AsRef<str>>,
        observed_at: impl AsRef<str>,
    ) -> Result<Self, StackError> {
        let mut names = Vec::new();
        for name in crates {
            names.push(check_crate_name(name.as_ref())?);
        }
        let observed_at = check_timestamp(observed_at.as_ref())?;
        Ok(Self {
            crates: names,
            observed_at,
        })
    }

    /// Crate names this server was given, in that order.
    #[must_use]
    pub fn crates(&self) -> &[String] {
        &self.crates
    }

    /// Fixture timestamp, Europe/Zagreb, never `Z`.
    #[must_use]
    pub fn observed_at(&self) -> &str {
        &self.observed_at
    }

    /// Fixture object returned by `stack_status`.
    #[must_use]
    pub fn fixture(&self) -> Value {
        json!({
            "crates": self.crates,
            "observed_at": self.observed_at,
        })
    }

    /// Answer newline-delimited JSON-RPC messages from `stdin` bytes.
    ///
    /// Each response is one JSON object followed by `\n`. Notifications and
    /// blank lines produce no output. A trailing object with no newline is
    /// still handled, so a test can pass one buffer and stop.
    ///
    /// This function does not read process stdin, does not spawn, and does
    /// not block waiting for another process.
    ///
    /// # Examples
    ///
    /// ```
    /// use openmind_mcp_stack::StackServer;
    ///
    /// let server = StackServer::with_timestamp(
    ///     ["openmind-schema"],
    ///     "2026-10-19T16:39:10+02:00",
    /// )
    /// .unwrap();
    /// let raw = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    /// let out = String::from_utf8(server.handle_bytes(raw)).unwrap();
    /// assert!(out.contains("stack_status"));
    /// ```
    #[must_use]
    pub fn handle_bytes(&self, stdin: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let text = match std::str::from_utf8(stdin) {
            Ok(text) => text,
            Err(_) => {
                push_line(
                    &mut out,
                    &error_response(Value::Null, -32700, "parse error"),
                );
                return out;
            }
        };
        for message in split_messages(text) {
            if let Some(response) = self.handle_message(message) {
                push_line(&mut out, &response);
            }
        }
        out
    }

    fn handle_message(&self, raw: &str) -> Option<Value> {
        let value: Value = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(_) => return Some(error_response(Value::Null, -32700, "parse error")),
        };
        let Some(obj) = value.as_object() else {
            return Some(error_response(Value::Null, -32600, "invalid request"));
        };
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return obj
                .get("id")
                .cloned()
                .map(|id| error_response(id, -32600, "invalid request"));
        }
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            return obj
                .get("id")
                .cloned()
                .map(|id| error_response(id, -32600, "invalid request"));
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let id = obj.get("id").cloned()?;
        Some(self.dispatch(id, method, &params))
    }

    fn dispatch(&self, id: Value, method: &str, params: &Value) -> Value {
        match method {
            "initialize" => self.initialize(id, params),
            "tools/list" => self.tools_list(id, params),
            "tools/call" => self.tools_call(id, params),
            "ping" => result_response(id, json!({})),
            _ => error_response(id, -32601, "method not found"),
        }
    }

    fn initialize(&self, id: Value, params: &Value) -> Value {
        if !(params.is_null() || params.is_object()) {
            return error_response(id, -32602, "invalid params");
        }
        let Some(protocol) = protocol_of(params) else {
            return error_response(id, -32602, "invalid params");
        };
        result_response(
            id,
            json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
                "instructions": INSTRUCTIONS,
            }),
        )
    }

    fn tools_list(&self, id: Value, params: &Value) -> Value {
        if !(params.is_null() || params.is_object()) {
            return error_response(id, -32602, "invalid params");
        }
        result_response(
            id,
            json!({
                "tools": [{
                    "name": TOOL_NAME,
                    "description": TOOL_DESCRIPTION,
                    "inputSchema": {
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }
                }]
            }),
        )
    }

    fn tools_call(&self, id: Value, params: &Value) -> Value {
        let Some(obj) = params.as_object() else {
            return error_response(id, -32602, "invalid params");
        };
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            return error_response(id, -32602, "invalid params");
        };
        if let Some(args) = obj.get("arguments") {
            if !(args.is_null() || args.is_object()) {
                return error_response(id, -32602, "invalid params");
            }
        }
        if name != TOOL_NAME {
            return result_response(
                id,
                json!({
                    "content": [{ "type": "text", "text": "unknown tool" }],
                    "isError": true
                }),
            );
        }
        let fixture = self.fixture();
        let text = serde_json::to_string(&fixture).expect("fixture json is finite");
        result_response(
            id,
            json!({
                "content": [{ "type": "text", "text": text }],
                "structuredContent": fixture,
                "isError": false
            }),
        )
    }
}

/// Why a server could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackError {
    /// The name was empty, too long, or not a plain crate token.
    BadCrate,
    /// The stamp was not Europe/Zagreb RFC 3339 with an explicit offset.
    BadTimestamp,
}

impl fmt::Display for StackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadCrate => write!(f, "crate name must be a plain token"),
            Self::BadTimestamp => {
                write!(f, "timestamp must be Europe/Zagreb RFC 3339, never Z")
            }
        }
    }
}

impl std::error::Error for StackError {}

fn check_crate_name(name: &str) -> Result<String, StackError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(StackError::BadCrate);
    };
    if !first.is_ascii_alphabetic() || name.len() > 64 {
        return Err(StackError::BadCrate);
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(StackError::BadCrate);
    }
    Ok(name.to_owned())
}

fn check_timestamp(ts: &str) -> Result<String, StackError> {
    let bytes = ts.as_bytes();
    if bytes.len() != 25 || bytes.contains(&b'Z') {
        return Err(StackError::BadTimestamp);
    }
    let digit_span = |start: usize, end: usize| bytes[start..end].iter().all(u8::is_ascii_digit);
    let zagreb_offset = bytes[19] == b'+'
        && bytes[20] == b'0'
        && (bytes[21] == b'1' || bytes[21] == b'2')
        && bytes[22] == b':'
        && bytes[23] == b'0'
        && bytes[24] == b'0';
    let shape = bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && digit_span(0, 4)
        && digit_span(5, 7)
        && digit_span(8, 10)
        && digit_span(11, 13)
        && digit_span(14, 16)
        && digit_span(17, 19)
        && zagreb_offset;
    if shape {
        Ok(ts.to_owned())
    } else {
        Err(StackError::BadTimestamp)
    }
}

fn protocol_of(params: &Value) -> Option<&str> {
    match params.get("protocolVersion") {
        None => Some(DEFAULT_PROTOCOL),
        Some(Value::String(version)) => {
            if SUPPORTED_PROTOCOLS.contains(&version.as_str()) {
                Some(version.as_str())
            } else {
                Some(DEFAULT_PROTOCOL)
            }
        }
        Some(_) => None,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn result_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn push_line(out: &mut Vec<u8>, value: &Value) {
    let line = serde_json::to_vec(value).expect("response json is finite");
    out.extend_from_slice(&line);
    out.push(b'\n');
}

fn split_messages(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("Content-Length:") || line.starts_with("Content-Type:") {
            continue;
        }
        out.push(line);
    }
    out
}
