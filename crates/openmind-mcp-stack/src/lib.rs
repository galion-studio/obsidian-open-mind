// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! OpenMind MCP server (stdio, newline-delimited JSON-RPC 2.0).
//!
//! Replaces the B57 fixture stub (2026-10-29). Two tool groups:
//!
//! - **memory** (backed by the live `openmind-memory` HTTP API on loopback):
//!   `memory_search`, `memory_recall`, `memory_save`, `memory_recent`.
//! - **vault** (read-only Obsidian vault folder, see [`vault`]):
//!   `vault_list`, `vault_read`, `vault_search`.
//!
//! There is no vault write tool. A `tools/call` for any name that is not
//! listed is an error; a write-like name (`vault_write`, `*_delete`, ...) is
//! answered with an explicit "vault is read-only" refusal. No fixture data:
//! every result comes from the memory server or the vault folder.

#![forbid(unsafe_code)]

pub mod memory;
pub mod vault;

use memory::Memory;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Duration;
use vault::{Vault, VaultError};

/// MCP server name advertised in `initialize`.
pub const SERVER_NAME: &str = "openmind-mcp-stack";
/// Server version advertised in `initialize`.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Protocol versions this server echoes.
pub const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];
/// Used when the client omits a version or asks for one we do not speak.
pub const DEFAULT_PROTOCOL: &str = "2025-03-26";
/// Default memory server.
pub const DEFAULT_MEMORY_URL: &str = "http://127.0.0.1:8420";
/// Default memory session key for `memory_save` / `memory_recall`.
pub const DEFAULT_SESSION_KEY: &str = "hermes-mcp";
/// Max characters `memory_save` accepts.
pub const MAX_SAVE_CHARS: usize = 8000;

pub const MEMORY_TOOLS: &[&str] = &[
    "memory_search",
    "memory_recall",
    "memory_save",
    "memory_recent",
];
pub const VAULT_TOOLS: &[&str] = &["vault_list", "vault_read", "vault_search"];

/// Server settings (see `main.rs` for the flags and env names).
#[derive(Debug, Clone)]
pub struct Settings {
    pub memory_url: Option<String>,
    pub memory_key: Option<String>,
    pub vault: Option<PathBuf>,
    pub session_key: String,
    pub timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            memory_url: Some(DEFAULT_MEMORY_URL.into()),
            memory_key: None,
            vault: None,
            session_key: DEFAULT_SESSION_KEY.into(),
            timeout: Duration::from_secs(10),
        }
    }
}

/// The MCP server state.
#[derive(Debug, Clone)]
pub struct Server {
    memory: Option<Memory>,
    vault: Option<Vault>,
    session_key: String,
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn ok_json(v: Value) -> Value {
    let text = serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string());
    json!({"content": [{"type": "text", "text": text}], "structuredContent": v, "isError": false})
}

fn err_text(msg: impl Into<String>) -> Value {
    text_result(msg.into(), true)
}

fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn arg_usize(a: &Value, k: &str, default: usize, min: usize, max: usize) -> usize {
    a.get(k)
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .map(|n| n as usize)
        .unwrap_or(default)
        .clamp(min, max)
}

fn is_write_like(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "write",
        "create",
        "delete",
        "remove",
        "rename",
        "move",
        "edit",
        "append",
        "update",
        "patch",
        "put",
        "mkdir",
        "rm",
        "save_note",
        "upload",
    ]
    .iter()
    .any(|w| n.contains(w))
}

impl Server {
    /// Build from settings. A bad memory URL or a missing vault folder is an
    /// error (fail loudly at start, not at the first call).
    pub fn new(s: &Settings) -> Result<Server, String> {
        let memory = match &s.memory_url {
            Some(u) => Some(Memory::new(u, s.memory_key.clone(), s.timeout)?),
            None => None,
        };
        let vault = match &s.vault {
            Some(p) => Some(Vault::open(p).map_err(|e| e.to_string())?),
            None => None,
        };
        if memory.is_none() && vault.is_none() {
            return Err("nothing to serve: no memory URL and no vault".into());
        }
        let session_key = s.session_key.trim().to_string();
        if session_key.is_empty() || session_key.len() > 128 {
            return Err("session key must be 1..=128 bytes".into());
        }
        Ok(Server {
            memory,
            vault,
            session_key,
        })
    }

    pub fn tool_names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.memory.is_some() {
            v.extend_from_slice(MEMORY_TOOLS);
        }
        if self.vault.is_some() {
            v.extend_from_slice(VAULT_TOOLS);
        }
        v
    }

    fn tool_defs(&self) -> Vec<Value> {
        let mut out = Vec::new();
        if self.memory.is_some() {
            out.push(json!({
                "name": "memory_search",
                "description": "Search OpenMind long-term memory (openmind-memory full-text search). kind: memories (extracted facts), conversations (saved messages) or both.",
                "inputSchema": {"type": "object", "properties": {
                    "query": {"type": "string", "description": "words to search for"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 20, "default": 5},
                    "kind": {"type": "string", "enum": ["memories", "conversations", "both"], "default": "both"}
                }, "required": ["query"]}
            }));
            out.push(json!({
                "name": "memory_recall",
                "description": "Recall OpenMind memory context for a query (openmind-memory /recall). Scoped to one memory session by default; the saved text must contain the query to come back as a direct hit.",
                "inputSchema": {"type": "object", "properties": {
                    "query": {"type": "string"},
                    "session_key": {"type": "string", "description": "memory session (default: this server's session key)"},
                    "scope": {"type": "string", "enum": ["session", "all"], "default": "session"}
                }, "required": ["query"]}
            }));
            out.push(json!({
                "name": "memory_save",
                "description": "Save one note into OpenMind long-term memory (openmind-memory /capture, one message row).",
                "inputSchema": {"type": "object", "properties": {
                    "text": {"type": "string", "maxLength": MAX_SAVE_CHARS},
                    "session_key": {"type": "string", "description": "memory session (default: this server's session key)"}
                }, "required": ["text"]}
            }));
            out.push(json!({
                "name": "memory_recent",
                "description": "List the newest saved memory messages (openmind-memory /recent), newest first, times in Europe/Zagreb.",
                "inputSchema": {"type": "object", "properties": {
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 10},
                    "session_key": {"type": "string", "description": "only this memory session (default: all)"}
                }}
            }));
        }
        if self.vault.is_some() {
            out.push(json!({
                "name": "vault_list",
                "description": "List Markdown notes in the Obsidian vault (read-only). Hidden folders such as .git and .obsidian are never listed.",
                "inputSchema": {"type": "object", "properties": {
                    "folder": {"type": "string", "description": "sub-folder relative to the vault root (default: whole vault)"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 200}
                }}
            }));
            out.push(json!({
                "name": "vault_read",
                "description": "Read one Markdown note from the Obsidian vault (read-only). Path is relative to the vault root.",
                "inputSchema": {"type": "object", "properties": {
                    "path": {"type": "string", "description": "e.g. 00-summary.md"},
                    "max_bytes": {"type": "integer", "minimum": 1, "maximum": vault::MAX_READ_BYTES, "default": vault::DEFAULT_READ_BYTES}
                }, "required": ["path"]}
            }));
            out.push(json!({
                "name": "vault_search",
                "description": "Case-insensitive text search over the Obsidian vault notes (read-only). Returns path, line and snippet.",
                "inputSchema": {"type": "object", "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                }, "required": ["query"]}
            }));
        }
        out
    }

    /// Handle one parsed JSON-RPC message. `None` = no reply (notification).
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str);
        let Some(method) = method else {
            return id.map(|id| rpc_error(id, -32600, "invalid request: no method"));
        };
        let id = id?; // notifications get no reply
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or("");
                let v = if SUPPORTED_PROTOCOLS.contains(&asked) {
                    asked
                } else {
                    DEFAULT_PROTOCOL
                };
                json!({
                    "protocolVersion": v,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
                    "instructions": "OpenMind memory (openmind-memory API) and a read-only Obsidian vault. The vault cannot be written through this server."
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({"tools": self.tool_defs()}),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                self.call(name, &args)
            }
            "resources/list" => json!({"resources": []}),
            "prompts/list" => json!({"prompts": []}),
            _ => {
                return Some(rpc_error(
                    id,
                    -32601,
                    &format!("method not found: {method}"),
                ))
            }
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    /// Handle one raw line (used by `main` and tests).
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => self.handle(&v).map(|r| r.to_string()),
            Err(e) => {
                Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}")).to_string())
            }
        }
    }

    /// Run one tool. Always returns an MCP `CallToolResult`.
    pub fn call(&self, name: &str, args: &Value) -> Value {
        let args = if args.is_object() {
            args.clone()
        } else {
            json!({})
        };
        match name {
            "memory_search" | "memory_recall" | "memory_save" | "memory_recent" => {
                let Some(m) = &self.memory else {
                    return err_text("memory tools are disabled on this server");
                };
                match name {
                    "memory_search" => self.memory_search(m, &args),
                    "memory_recall" => self.memory_recall(m, &args),
                    "memory_save" => self.memory_save(m, &args),
                    _ => self.memory_recent(m, &args),
                }
            }
            "vault_list" | "vault_read" | "vault_search" => {
                let Some(v) = &self.vault else {
                    return err_text(
                        "vault tools are disabled on this server (no vault configured)",
                    );
                };
                let r = match name {
                    "vault_list" => vault_list(v, &args),
                    "vault_read" => vault_read(v, &args),
                    _ => vault_search(v, &args),
                };
                r.unwrap_or_else(|e| err_text(format!("vault: {e}")))
            }
            n if is_write_like(n) => err_text(format!(
                "refused: {n}: the vault is read-only through this server; there are no write tools"
            )),
            n => err_text(format!("unknown tool: {n}")),
        }
    }

    fn session<'a>(&'a self, args: &'a Value) -> &'a str {
        arg_str(args, "session_key").unwrap_or(&self.session_key)
    }

    fn memory_search(&self, m: &Memory, a: &Value) -> Value {
        let Some(q) = arg_str(a, "query") else {
            return err_text("query is required");
        };
        let limit = arg_usize(a, "limit", 5, 1, 20);
        let kind = arg_str(a, "kind").unwrap_or("both");
        let paths: &[&str] = match kind {
            "memories" => &["/search/memories"],
            "conversations" => &["/search/conversations"],
            "both" => &["/search/memories", "/search/conversations"],
            _ => return err_text("kind must be memories, conversations or both"),
        };
        let mut out = Map::new();
        for p in paths {
            match m.post(p, &json!({"query": q, "limit": limit})) {
                Ok((200, v)) => {
                    let key = if p.ends_with("memories") {
                        "memories"
                    } else {
                        "conversations"
                    };
                    out.insert(
                        key.into(),
                        json!({"total": v["total"], "results": v["results"]}),
                    );
                }
                Ok((s, v)) => {
                    return err_text(format!("openmind-memory {p}: HTTP {s}: {}", brief(&v)))
                }
                Err(e) => return err_text(e),
            }
        }
        ok_json(Value::Object(out))
    }

    fn memory_recall(&self, m: &Memory, a: &Value) -> Value {
        let Some(q) = arg_str(a, "query") else {
            return err_text("query is required");
        };
        let scope = arg_str(a, "scope").unwrap_or("session");
        if scope != "session" && scope != "all" {
            return err_text("scope must be session or all");
        }
        let sk = self.session(a);
        match m.post(
            "/recall",
            &json!({"query": q, "session_key": sk, "scope": scope}),
        ) {
            Ok((200, v)) => {
                if scope == "session" && v["scope"].as_str() != Some("session") {
                    // Older server ignores `scope`: its reply may mix sessions.
                    return err_text(
                        "openmind-memory did not confirm session scope (server too old); refusing to return possibly cross-session memory. Upgrade openmind-memory or pass scope=all.",
                    );
                }
                ok_json(json!({
                    "session_key": sk,
                    "scope": scope,
                    "context": v["context"],
                    "memory_count": v["memory_count"],
                }))
            }
            Ok((s, v)) => err_text(format!("openmind-memory /recall: HTTP {s}: {}", brief(&v))),
            Err(e) => err_text(e),
        }
    }

    fn memory_save(&self, m: &Memory, a: &Value) -> Value {
        let Some(text) = arg_str(a, "text") else {
            return err_text("text is required");
        };
        if text.chars().count() > MAX_SAVE_CHARS {
            return err_text(format!("text longer than {MAX_SAVE_CHARS} characters"));
        }
        let sk = self.session(a);
        let body = json!({
            "user_content": text,
            "assistant_content": "(saved via openmind-mcp-stack memory_save)",
            "session_key": sk,
            "messages": [{"role": "user", "content": text}],
        });
        match m.post("/capture", &body) {
            Ok((200, v)) => {
                let n = v["l0_recorded"].as_u64().unwrap_or(0);
                if n == 0 {
                    return err_text(
                        "openmind-memory stored nothing (text filtered as too short/noise, or the server is read-only)",
                    );
                }
                ok_json(
                    json!({"saved": n, "session_key": sk, "saved_at": zagreb_time::now_rfc3339()}),
                )
            }
            Ok((s, v)) => err_text(format!("openmind-memory /capture: HTTP {s}: {}", brief(&v))),
            Err(e) => err_text(e),
        }
    }

    fn memory_recent(&self, m: &Memory, a: &Value) -> Value {
        let limit = arg_usize(a, "limit", 10, 1, 50);
        let mut body = json!({"limit": limit});
        if let Some(sk) = arg_str(a, "session_key") {
            body["session_key"] = json!(sk);
        }
        match m.post("/recent", &body) {
            Ok((200, v)) => ok_json(json!({"total": v["total"], "items": v["items"]})),
            Ok((404, _)) => err_text(
                "openmind-memory has no /recent route (server older than 2026-10-29); upgrade openmind-memory",
            ),
            Ok((s, v)) => err_text(format!("openmind-memory /recent: HTTP {s}: {}", brief(&v))),
            Err(e) => err_text(e),
        }
    }
}

fn brief(v: &Value) -> String {
    let s = v
        .get("error")
        .map(|e| e.to_string())
        .unwrap_or_else(|| v.to_string());
    s.chars().take(300).collect()
}

fn vault_list(v: &Vault, a: &Value) -> Result<Value, VaultError> {
    let folder = arg_str(a, "folder").unwrap_or("");
    let limit = arg_usize(a, "limit", 200, 1, 500);
    let (notes, truncated) = v.list(folder, limit)?;
    Ok(ok_json(json!({
        "count": notes.len(),
        "truncated": truncated,
        "notes": notes.iter().map(|n| n.to_json()).collect::<Vec<_>>(),
    })))
}

fn vault_read(v: &Vault, a: &Value) -> Result<Value, VaultError> {
    let Some(path) = arg_str(a, "path") else {
        return Ok(err_text("path is required"));
    };
    let max = arg_usize(
        a,
        "max_bytes",
        vault::DEFAULT_READ_BYTES,
        1,
        vault::MAX_READ_BYTES,
    );
    let (info, text, truncated) = v.read(path, max)?;
    let head = format!(
        "# {} ({} bytes, modified {}{})\n\n",
        info.path,
        info.bytes,
        info.modified,
        if truncated { ", truncated" } else { "" }
    );
    Ok(json!({
        "content": [{"type": "text", "text": format!("{head}{text}")}],
        "structuredContent": {"path": info.path, "bytes": info.bytes, "modified": info.modified, "truncated": truncated},
        "isError": false
    }))
}

fn vault_search(v: &Vault, a: &Value) -> Result<Value, VaultError> {
    let Some(q) = arg_str(a, "query") else {
        return Ok(err_text("query is required"));
    };
    let limit = arg_usize(a, "limit", 20, 1, 100);
    let (hits, truncated) = v.search(q, limit)?;
    Ok(ok_json(json!({
        "count": hits.len(),
        "truncated": truncated,
        "hits": hits.iter().map(|h| json!({"path": h.path, "line": h.line, "snippet": h.snippet})).collect::<Vec<_>>(),
    })))
}

fn rpc_error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_like_names() {
        for n in [
            "vault_write",
            "vault_delete",
            "note_create",
            "rename_note",
            "rm",
        ] {
            assert!(is_write_like(n), "{n}");
        }
        for n in ["vault_read", "vault_list", "memory_search"] {
            assert!(!is_write_like(n), "{n}");
        }
    }

    #[test]
    fn arg_clamps() {
        let a = json!({"limit": 900, "x": "7"});
        assert_eq!(arg_usize(&a, "limit", 5, 1, 20), 20);
        assert_eq!(arg_usize(&a, "x", 5, 1, 20), 7);
        assert_eq!(arg_usize(&a, "missing", 5, 1, 20), 5);
    }

    #[test]
    fn nothing_to_serve_is_an_error() {
        let s = Settings {
            memory_url: None,
            ..Settings::default()
        };
        assert!(Server::new(&s).is_err());
    }

    #[test]
    fn non_loopback_memory_url_is_refused() {
        for u in [
            "http://10.0.0.1:8420",
            "https://127.0.0.1:8420",
            "http://127.0.0.1",
            "http://example.com:80",
        ] {
            let s = Settings {
                memory_url: Some(u.into()),
                ..Settings::default()
            };
            assert!(Server::new(&s).is_err(), "{u}");
        }
    }
}
