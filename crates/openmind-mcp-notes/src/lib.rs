// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Stdio MCP server for a research vault of markdown notes.
//!
//! DESK-ID: openmind.MCP-NOTES
//!
//! [`NotesServer`] answers `initialize`, `tools/list` and `tools/call`.
//! Search, read, list recent, tag search and backlinks only read the vault.
//! `create_draft_note` and `link_notes` write only under `drafts/`.
//! A published note is never modified. The process opens no socket.
//!
//! The sibling crate `openmind-mcp-stack` stays the fixture stub: its only
//! tool is `stack_status`, and it still opens no files.

#![forbid(unsafe_code)]

mod schema;
mod vault;

use schema::tools_result;
use serde_json::{json, Map, Value};
use std::path::Path;
use vault::{snippet_of, Vault, VaultError};

pub use schema::TOOL_NAMES;
pub use vault::VaultError as NotesError;

/// MCP server name advertised in `initialize`.
pub const SERVER_NAME: &str = "openmind-mcp-notes";

/// MCP server version advertised in `initialize`.
pub const SERVER_VERSION: &str = "0.1.0";

/// How many notes a list returns when `limit` is omitted.
pub const DEFAULT_LIMIT: usize = 20;

/// Largest `limit` a tool accepts.
pub const MAX_LIMIT: usize = 50;

/// Protocol versions this server will echo. Anything else is answered with
/// [`DEFAULT_PROTOCOL`].
pub const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Version used when the client omits one, or asks for a version we do not speak.
pub const DEFAULT_PROTOCOL: &str = "2024-11-05";

/// The four product lines, the same text as `openmind-desktop` `BRAND_LINES`.
pub const BRAND_LINES: [&str; 4] = [
    "G.A.L.I.O.N. \u{2014} General Autonomous Local Intelligence Open Network",
    "Developed and distributed by Galion Labs",
    "Perpetual Open MIT License",
    "Copyright \u{00A9} 2026 Galion Labs. All rights reserved.",
];

/// One vault, one clock.
///
/// The clock is the Europe/Zagreb stamp written into new drafts. Tests pass
/// a fixed stamp. The binary uses the live Zagreb clock.
pub struct NotesServer {
    vault: Vault,
    stamp: Option<String>,
}

impl NotesServer {
    /// Open `root` and stamp new drafts with the live Zagreb clock.
    ///
    /// # Errors
    ///
    /// The path is missing or is not a directory.
    pub fn open(root: &Path) -> Result<Self, NotesError> {
        Ok(Self {
            vault: Vault::open(root)?,
            stamp: None,
        })
    }

    /// Open `root` and stamp new drafts with `stamp`.
    ///
    /// `stamp` must be Europe/Zagreb RFC 3339 (`+01:00` or `+02:00`), never `Z`.
    ///
    /// # Errors
    ///
    /// The path is not a directory, or the stamp is not a Zagreb stamp.
    pub fn open_with_stamp(root: &Path, stamp: &str) -> Result<Self, NotesError> {
        zagreb_time::require_rfc3339(stamp).map_err(|_| VaultError::BadStamp)?;
        Ok(Self {
            vault: Vault::open(root)?,
            stamp: Some(stamp.to_string()),
        })
    }

    fn now(&self) -> String {
        match &self.stamp {
            Some(stamp) => stamp.clone(),
            None => zagreb_time::now_rfc3339(),
        }
    }

    /// Answer newline-delimited JSON-RPC messages.
    ///
    /// Each response is one JSON object followed by `\n`. Notifications and
    /// blank lines produce no output. This function does not spawn and does
    /// not block waiting for another process.
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
                "instructions": instructions(),
            }),
        )
    }

    fn tools_list(&self, id: Value, params: &Value) -> Value {
        if !(params.is_null() || params.is_object()) {
            return error_response(id, -32602, "invalid params");
        }
        result_response(id, tools_result())
    }

    fn tools_call(&self, id: Value, params: &Value) -> Value {
        let Some(obj) = params.as_object() else {
            return error_response(id, -32602, "invalid params");
        };
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            return error_response(id, -32602, "invalid params");
        };
        let args = match obj.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return error_response(id, -32602, "invalid params"),
        };
        if !keys_ok(name, &args) {
            return error_response(id, -32602, "invalid params");
        }
        match name {
            "search_notes" => self.search_notes(id, &args),
            "read_note" => self.read_note(id, &args),
            "list_recent" => self.list_recent(id, &args),
            "search_tags" => self.search_tags(id, &args),
            "backlinks" => self.backlinks(id, &args),
            "create_draft_note" => self.create_draft(id, &args),
            "link_notes" => self.link_notes(id, &args),
            _ => tool_text(id, "unknown tool", true),
        }
    }

    fn search_notes(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Some(query) = required_str(args, "query") else {
            return error_response(id, -32602, "invalid params");
        };
        let Ok(limit) = opt_limit(args) else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.search(query, limit) {
            Ok(out) => {
                let notes: Vec<Value> = out
                    .hits
                    .iter()
                    .map(|hit| {
                        json!({
                            "path": hit.note.path,
                            "title": hit.note.title,
                            "tags": hit.note.tags,
                            "updated": hit.note.updated,
                            "score": hit.score,
                            "snippet": snippet_of(&hit.note, &out.query),
                        })
                    })
                    .collect();
                tool_ok(
                    id,
                    json!({
                        "query": out.query,
                        "matched": out.matched,
                        "skipped": out.skipped,
                        "notes": notes,
                    }),
                )
            }
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }

    fn read_note(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Some(path) = required_str(args, "path") else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.read(path) {
            Ok(note) => tool_ok(id, note_json(&note)),
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }

    fn list_recent(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Ok(limit) = opt_limit(args) else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.recent(limit) {
            Ok(out) => tool_ok(id, list_json(out.matched, out.skipped, &out.notes)),
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }

    fn search_tags(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Some(tag) = required_str(args, "tag") else {
            return error_response(id, -32602, "invalid params");
        };
        let Ok(limit) = opt_limit(args) else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.by_tag(tag, limit) {
            Ok(out) => {
                let mut body = list_json(out.matched, out.skipped, &out.notes);
                if let Some(obj) = body.as_object_mut() {
                    obj.insert("tag".to_string(), json!(tag.trim()));
                }
                tool_ok(id, body)
            }
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }

    fn backlinks(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Some(note) = required_str(args, "note") else {
            return error_response(id, -32602, "invalid params");
        };
        let Ok(limit) = opt_limit(args) else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.backlinks(note, limit) {
            Ok(out) => {
                let notes: Vec<Value> = out
                    .notes
                    .iter()
                    .map(|hit| {
                        json!({
                            "path": hit.path,
                            "title": hit.title,
                            "via": hit.via,
                        })
                    })
                    .collect();
                tool_ok(
                    id,
                    json!({
                        "note": out.query,
                        "resolved": out.resolved,
                        "matched": out.matched,
                        "skipped": out.skipped,
                        "notes": notes,
                    }),
                )
            }
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }

    fn create_draft(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Some(title) = required_str(args, "title") else {
            return error_response(id, -32602, "invalid params");
        };
        let body = match optional_str(args, "body") {
            Ok(body) => body.unwrap_or(""),
            Err(()) => return error_response(id, -32602, "invalid params"),
        };
        let Ok(tags) = optional_tags(args) else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.create_draft(title, body, &tags, &self.now()) {
            Ok(note) => tool_ok(id, note_json(&note)),
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }

    fn link_notes(&self, id: Value, args: &Map<String, Value>) -> Value {
        let Some(from) = required_str(args, "from") else {
            return error_response(id, -32602, "invalid params");
        };
        let Some(to) = required_str(args, "to") else {
            return error_response(id, -32602, "invalid params");
        };
        match self.vault.link(from, to, &self.now()) {
            Ok(out) => tool_ok(
                id,
                json!({
                    "kind": out.kind,
                    "path": out.path,
                    "wrote": out.wrote,
                    "from": out.from,
                    "to": out.to,
                }),
            ),
            Err(err) => tool_text(id, &err.to_string(), true),
        }
    }
}

fn note_json(note: &vault::Note) -> Value {
    json!({
        "path": note.path,
        "title": note.title,
        "tags": note.tags,
        "links": note.links,
        "created": note.created,
        "updated": note.updated,
        "bytes": note.bytes,
        "body": note.body,
    })
}

fn list_json(matched: usize, skipped: u32, notes: &[vault::Note]) -> Value {
    let notes: Vec<Value> = notes
        .iter()
        .map(|note| {
            json!({
                "path": note.path,
                "title": note.title,
                "tags": note.tags,
                "updated": note.updated,
            })
        })
        .collect();
    json!({
        "matched": matched,
        "skipped": skipped,
        "notes": notes,
    })
}

fn instructions() -> String {
    let mut text = String::from(
        "Research vault. Read tools do not write. create_draft_note and link_notes write only under drafts/. Published notes stay untouched. No sockets.\n",
    );
    for line in BRAND_LINES {
        text.push_str(line);
        text.push('\n');
    }
    text
}

fn keys_ok(name: &str, args: &Map<String, Value>) -> bool {
    let allowed: &[&str] = match name {
        "search_notes" => &["query", "limit"],
        "read_note" => &["path"],
        "list_recent" => &["limit"],
        "search_tags" => &["tag", "limit"],
        "backlinks" => &["note", "limit"],
        "create_draft_note" => &["title", "body", "tags"],
        "link_notes" => &["from", "to"],
        _ => return true,
    };
    args.keys().all(|key| allowed.contains(&key.as_str()))
}

fn required_str<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn optional_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ()> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(()),
    }
}

fn optional_tags(args: &Map<String, Value>) -> Result<Vec<String>, ()> {
    match args.get("tags") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                let Some(tag) = item.as_str() else {
                    return Err(());
                };
                out.push(tag.to_string());
            }
            Ok(out)
        }
        Some(_) => Err(()),
    }
}

fn opt_limit(args: &Map<String, Value>) -> Result<usize, ()> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(DEFAULT_LIMIT),
        Some(Value::Number(number)) => {
            let value = number.as_u64().ok_or(())?;
            let value = usize::try_from(value).map_err(|_| ())?;
            if (1..=MAX_LIMIT).contains(&value) {
                Ok(value)
            } else {
                Err(())
            }
        }
        Some(_) => Err(()),
    }
}

fn tool_ok(id: Value, value: Value) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    result_response(
        id,
        json!({
            "content": [{ "type": "text", "text": text }],
            "structuredContent": value,
            "isError": false
        }),
    )
}

fn tool_text(id: Value, text: &str, is_error: bool) -> Value {
    result_response(
        id,
        json!({
            "content": [{ "type": "text", "text": text }],
            "isError": is_error
        }),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_lines_match_the_desktop_source() {
        let src = include_str!("../../openmind-desktop/src/page.rs");
        let needles = [
            "G.A.L.I.O.N. \\u{2014} General Autonomous Local Intelligence Open Network",
            "Developed and distributed by Galion Labs",
            "Perpetual Open MIT License",
            "Copyright \\u{00A9} 2026 Galion Labs. All rights reserved.",
        ];
        for needle in needles {
            assert!(src.contains(needle), "{needle}");
        }
        assert_eq!(BRAND_LINES[1], "Developed and distributed by Galion Labs");
        assert_eq!(BRAND_LINES[2], "Perpetual Open MIT License");
        assert!(BRAND_LINES[0].contains("G.A.L.I.O.N."));
        assert!(BRAND_LINES[0].contains('\u{2014}'));
        assert!(BRAND_LINES[3].contains('\u{00A9}'));
        assert!(BRAND_LINES[3].contains("2026 Galion Labs"));
    }
}
