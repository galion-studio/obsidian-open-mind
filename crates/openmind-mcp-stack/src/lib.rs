// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! OpenMind MCP server (stdio, newline-delimited JSON-RPC 2.0).
//!
//! DESK-ID: openmind.B57 (replaces the fixture stub of the same crate).
//!
//! Tools:
//! - `memory_search`, `memory_conversations`, `memory_recall`, `memory_save`:
//!   the openmind-memory gateway on loopback (`/search/memories`,
//!   `/search/conversations`, `/recall`, `/capture`).
//! - `vault_list`, `vault_read`, `vault_search`: a **read-only** Obsidian
//!   vault (see [`vault`]). No tool writes, renames or deletes a vault file.
//! - `openmind_status`: which parts are configured, memory `/health`, note
//!   count, one Europe/Zagreb timestamp.
//! - Stack tools (read-only, see [`stack`]): `stack_health`, `stack_alerts`,
//!   `cortex_heals`, `spend_status`, `backup_status`, `queue_status`,
//!   `docs_list`, `docs_read`, `docs_search` (Galion One folders excluded),
//!   `hermes_readiness`.
//! - `vault_note_create` (see [`notes`]): the only write tool; **off by
//!   default**, listed only when switched on in the stack config file.
//!
//! Every tool result goes through [`redact::value`] (secret-named fields,
//! token-like strings and non-loopback IPv4 addresses are masked).
//!
//! A part that is not configured (no `--vault`, or `--no-memory`) does not
//! list its tools. Tool errors are returned as `isError: true` results,
//! never as panics. The memory key is never echoed.
//!
//! Tests feed bytes to [`Server::handle_bytes`]; that path spawns nothing.

#![forbid(unsafe_code)]

pub mod memory;
pub mod notes;
pub mod redact;
pub mod stack;
pub mod vault;

use memory::MemoryClient;
use notes::NoteCreator;
use serde_json::{json, Map, Value};
use stack::Stack;
use vault::Vault;

/// MCP server name advertised in `initialize`.
pub const SERVER_NAME: &str = "openmind-mcp-stack";

/// MCP server version advertised in `initialize`.
pub const SERVER_VERSION: &str = "0.3.0";

/// Protocol versions this server echoes. Anything else gets [`DEFAULT_PROTOCOL`].
pub const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

/// Version used when the client omits one or asks for one we do not speak.
pub const DEFAULT_PROTOCOL: &str = "2024-11-05";

/// Longest memory query accepted.
pub const MAX_QUERY_CHARS: usize = 2000;
/// Longest text `memory_save` stores.
pub const MAX_SAVE_CHARS: usize = 8000;
/// Most results asked from a memory search.
pub const MAX_MEMORY_LIMIT: u64 = 20;
/// Longest tool text returned to the model.
pub const MAX_TOOL_TEXT: usize = 256 * 1024;

/// Session key used for `memory_save` (and for `memory_recall` without one).
pub const DEFAULT_SAVE_SESSION: &str = "hermes-mcp-notes";

/// Every tool name this server can list.
pub const TOOL_NAMES: [&str; 8] = [
    "memory_search",
    "memory_conversations",
    "memory_recall",
    "memory_save",
    "vault_list",
    "vault_read",
    "vault_search",
    "openmind_status",
];

/// Read-only stack tools (listed when their part is configured).
pub const STACK_TOOL_NAMES: [&str; 10] = [
    "stack_health",
    "stack_alerts",
    "cortex_heals",
    "spend_status",
    "backup_status",
    "queue_status",
    "docs_list",
    "docs_read",
    "docs_search",
    "hermes_readiness",
];

/// Write tools: each off by default, behind its own switch.
pub const WRITE_TOOL_NAMES: [&str; 1] = ["vault_note_create"];

/// Server configuration.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Memory gateway client (None = memory tools off).
    pub memory: Option<MemoryClient>,
    /// Read-only vault (None = vault tools off).
    pub vault: Option<Vault>,
    /// Session key for saved notes.
    pub save_session: String,
    /// Rest of the stack (None = stack tools off).
    pub stack: Option<Stack>,
    /// `vault_note_create` (None = off, the default).
    pub notes: Option<NoteCreator>,
    /// Fixed timestamp for tests; None = now (Europe/Zagreb).
    pub fixed_time: Option<String>,
}

/// The MCP server.
#[derive(Debug, Clone)]
pub struct Server {
    cfg: Config,
}

impl Server {
    /// Build a server.
    #[must_use]
    pub fn new(mut cfg: Config) -> Self {
        if cfg.save_session.trim().is_empty() {
            cfg.save_session = DEFAULT_SAVE_SESSION.into();
        }
        Server { cfg }
    }

    /// Names of the tools this configuration lists.
    #[must_use]
    pub fn tool_names(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = TOOL_NAMES
            .iter()
            .copied()
            .filter(|n| {
                if n.starts_with("memory_") {
                    self.cfg.memory.is_some()
                } else if n.starts_with("vault_") {
                    self.cfg.vault.is_some()
                } else {
                    true
                }
            })
            .collect();
        if let Some(s) = &self.cfg.stack {
            out.extend(s.tool_names());
        }
        if self.cfg.notes.is_some() {
            out.extend(WRITE_TOOL_NAMES);
        }
        out
    }

    /// Answer newline-delimited JSON-RPC messages. One response line per
    /// request; notifications and blank lines produce nothing.
    #[must_use]
    pub fn handle_bytes(&self, stdin: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let Ok(text) = std::str::from_utf8(stdin) else {
            push_line(
                &mut out,
                &error_response(Value::Null, -32700, "parse error"),
            );
            return out;
        };
        for message in split_messages(text) {
            if let Some(response) = self.handle_message(message) {
                push_line(&mut out, &response);
            }
        }
        out
    }

    fn handle_message(&self, raw: &str) -> Option<Value> {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return Some(error_response(Value::Null, -32700, "parse error"));
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
        Some(match method {
            "initialize" => self.initialize(id, &params),
            "tools/list" => self.tools_list(id),
            "tools/call" => self.tools_call(id, &params),
            "ping" => result_response(id, json!({})),
            _ => error_response(id, -32601, "method not found"),
        })
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
                "instructions": "OpenMind long-term memory (search, recall, save), a read-only Obsidian vault (list, read, search) and read-only views of the Galion stack: health, alerts, cortex heals, OpenRouter spend, backups, work queue, the docs tree (Galion One excluded) and hermes-core readiness. Nothing here restarts, deploys or edits existing notes.",
            }),
        )
    }

    fn tools_list(&self, id: Value) -> Value {
        let tools: Vec<Value> = self.tool_names().into_iter().map(tool_spec).collect();
        result_response(id, json!({ "tools": tools }))
    }

    fn tools_call(&self, id: Value, params: &Value) -> Value {
        let Some(obj) = params.as_object() else {
            return error_response(id, -32602, "invalid params");
        };
        let Some(name) = obj.get("name").and_then(Value::as_str) else {
            return error_response(id, -32602, "invalid params");
        };
        let empty = Map::new();
        let args = match obj.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(m)) => m,
            Some(_) => return error_response(id, -32602, "invalid params"),
        };
        if !self.tool_names().contains(&name) {
            return result_response(id, tool_error("unknown tool"));
        }
        let out = match name {
            "memory_search" => self.memory_search(args),
            "memory_conversations" => self.memory_conversations(args),
            "memory_recall" => self.memory_recall(args),
            "memory_save" => self.memory_save(args),
            "vault_list" => self.vault_list(args),
            "vault_read" => self.vault_read(args),
            "vault_search" => self.vault_search(args),
            "openmind_status" => Ok(self.status()),
            "vault_note_create" => self.note_create(args),
            other => self.stack_call(other, args),
        };
        result_response(
            id,
            match out {
                Ok(v) => tool_ok(v),
                Err(e) => tool_error(&e),
            },
        )
    }

    fn mem(&self) -> Result<&MemoryClient, String> {
        self.cfg
            .memory
            .as_ref()
            .ok_or_else(|| "memory is not configured".into())
    }

    fn vault(&self) -> Result<&Vault, String> {
        self.cfg
            .vault
            .as_ref()
            .ok_or_else(|| "vault is not configured".into())
    }

    fn memory_search(&self, a: &Map<String, Value>) -> Result<Value, String> {
        let query = query_arg(a, "query")?;
        let mut body = json!({ "query": query, "limit": limit_arg(a, 5, MAX_MEMORY_LIMIT) });
        for k in ["type", "scene"] {
            if let Some(s) = a.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                body[k] = json!(cut(s, 100));
            }
        }
        let mem = self.mem()?;
        let mut out = mem.post("/search/memories", &body)?;
        // Structured (L1) memories need the extraction pipeline; until it has
        // run, the store only holds conversations (L0). Fall back so a fresh
        // save is still findable by one tool call.
        if out.get("total").and_then(Value::as_u64) == Some(0) {
            let conv = json!({ "query": body["query"], "limit": body["limit"] });
            if let Ok(c) = mem.post("/search/conversations", &conv) {
                out["conversations"] = c;
                out["note"] =
                    json!("no structured memories matched; conversation matches attached");
            }
        }
        Ok(out)
    }

    fn memory_conversations(&self, a: &Map<String, Value>) -> Result<Value, String> {
        let query = query_arg(a, "query")?;
        let mut body = json!({ "query": query, "limit": limit_arg(a, 5, MAX_MEMORY_LIMIT) });
        if let Some(s) = a
            .get("session_key")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            body["session_key"] = json!(cut(s, 200));
        }
        self.mem()?.post("/search/conversations", &body)
    }

    fn memory_recall(&self, a: &Map<String, Value>) -> Result<Value, String> {
        let query = query_arg(a, "query")?;
        let (session, scope) = match a.get("session_key").and_then(Value::as_str) {
            Some(s) if !s.trim().is_empty() => (cut(s.trim(), 200), "session"),
            _ => (self.cfg.save_session.clone(), "all"),
        };
        self.mem()?.post(
            "/recall",
            &json!({ "query": query, "session_key": session, "scope": scope }),
        )
    }

    fn memory_save(&self, a: &Map<String, Value>) -> Result<Value, String> {
        let text = a
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("text is required")?;
        if text.chars().count() > MAX_SAVE_CHARS {
            return Err(format!("text is longer than {MAX_SAVE_CHARS} characters"));
        }
        let r = self.mem()?.post(
            "/capture",
            &json!({
                "user_content": text,
                "assistant_content": "Saved to long-term memory by the memory_save tool.",
                "session_key": self.cfg.save_session,
                "session_id": self.cfg.save_session,
            }),
        )?;
        Ok(json!({ "saved": true, "session_key": self.cfg.save_session, "server": r }))
    }

    fn vault_list(&self, a: &Map<String, Value>) -> Result<Value, String> {
        list_notes(self.vault()?, a)
    }

    fn vault_read(&self, a: &Map<String, Value>) -> Result<Value, String> {
        read_note(self.vault()?, a)
    }

    fn vault_search(&self, a: &Map<String, Value>) -> Result<Value, String> {
        search_notes(self.vault()?, a)
    }

    fn stack(&self) -> Result<&Stack, String> {
        self.cfg
            .stack
            .as_ref()
            .ok_or_else(|| "stack tools are not configured".into())
    }

    fn stack_call(&self, name: &str, a: &Map<String, Value>) -> Result<Value, String> {
        let s = self.stack()?;
        let lim = |d: u64, m: u64| usize::try_from(limit_arg(a, d, m)).unwrap_or(10);
        match name {
            "stack_health" => s.health(a.get("component").and_then(Value::as_str)),
            "stack_alerts" => s.alerts(lim(10, 50)),
            "cortex_heals" => s.heals(lim(10, 50)),
            "spend_status" => s.spend(),
            "backup_status" => s.backup(),
            "queue_status" => s.queue(),
            "docs_list" | "docs_read" | "docs_search" => {
                let docs = s.docs.as_ref().ok_or("docs tree is not configured")?;
                match name {
                    "docs_list" => list_notes(docs, a),
                    "docs_read" => read_note(docs, a),
                    _ => search_notes(docs, a),
                }
            }
            "hermes_readiness" => s.readiness(),
            _ => Err("unknown tool".into()),
        }
    }

    fn note_create(&self, a: &Map<String, Value>) -> Result<Value, String> {
        let n = self
            .cfg
            .notes
            .as_ref()
            .ok_or("vault_note_create is switched off")?;
        let title = a.get("title").and_then(Value::as_str).unwrap_or("");
        let text = a.get("text").and_then(Value::as_str).unwrap_or("");
        let stamp = self
            .cfg
            .fixed_time
            .clone()
            .unwrap_or_else(zagreb_time::now_rfc3339);
        let path = n.create(title, text, &stamp)?;
        Ok(
            json!({ "created": true, "path": path, "note": "new file only; existing notes are never changed" }),
        )
    }

    fn status(&self) -> Value {
        let memory = match &self.cfg.memory {
            None => json!({ "configured": false }),
            Some(m) => match m.health() {
                Ok(h) => json!({ "configured": true, "key": m.has_key(), "health": h }),
                Err(e) => json!({ "configured": true, "key": m.has_key(), "error": e }),
            },
        };
        let vault = match &self.cfg.vault {
            None => json!({ "configured": false }),
            Some(v) => json!({ "configured": true, "read_only": true, "notes": v.note_count() }),
        };
        let stack = match &self.cfg.stack {
            None => json!({ "configured": false }),
            Some(s) => json!({
                "configured": true,
                "tools": s.tool_names(),
                "docs_notes": s.docs.as_ref().map(Vault::note_count),
            }),
        };
        let writes: Vec<Value> = WRITE_TOOL_NAMES
            .iter()
            .map(|n| json!({ "tool": n, "on": self.tool_names().contains(n) }))
            .collect();
        json!({
            "server": SERVER_NAME,
            "version": SERVER_VERSION,
            "memory": memory,
            "vault": vault,
            "stack": stack,
            "write_tools": writes,
            "observed_at": self
                .cfg
                .fixed_time
                .clone()
                .unwrap_or_else(zagreb_time::now_rfc3339),
        })
    }
}

fn list_notes(v: &Vault, a: &Map<String, Value>) -> Result<Value, String> {
    let dir = a.get("folder").and_then(Value::as_str).unwrap_or("");
    let limit = usize::try_from(limit_arg(a, 200, vault::LIST_MAX as u64)).unwrap_or(200);
    let (notes, truncated) = v.list(dir, limit)?;
    let notes: Vec<Value> = notes
        .into_iter()
        .map(|(p, size, mtime)| json!({ "path": p, "bytes": size, "modified": zagreb(mtime) }))
        .collect();
    Ok(json!({ "count": notes.len(), "truncated": truncated, "notes": notes }))
}

fn read_note(v: &Vault, a: &Map<String, Value>) -> Result<Value, String> {
    let path = a
        .get("path")
        .and_then(Value::as_str)
        .ok_or("path is required")?;
    let max = a
        .get("max_bytes")
        .and_then(Value::as_u64)
        .map_or(vault::READ_DEFAULT_BYTES, |n| {
            usize::try_from(n).unwrap_or(vault::READ_MAX_BYTES)
        });
    let (text, size, cut) = v.read(path, max)?;
    Ok(json!({ "path": path.trim(), "bytes": size, "truncated": cut, "text": text }))
}

fn search_notes(v: &Vault, a: &Map<String, Value>) -> Result<Value, String> {
    let q = a
        .get("query")
        .and_then(Value::as_str)
        .ok_or("query is required")?;
    let limit = usize::try_from(limit_arg(a, 20, vault::SEARCH_MAX_HITS as u64)).unwrap_or(20);
    let r = v.search(q, limit)?;
    let hits: Vec<Value> = r
        .hits
        .into_iter()
        .map(|h| json!({ "path": h.path, "line": h.line, "text": h.text }))
        .collect();
    Ok(json!({
        "count": hits.len(),
        "files_scanned": r.files_scanned,
        "truncated": r.truncated,
        "hits": hits,
    }))
}

fn zagreb(unix: u64) -> String {
    zagreb_time::from_unix(i64::try_from(unix).unwrap_or(0))
}

fn query_arg(a: &Map<String, Value>, k: &str) -> Result<String, String> {
    let q = a
        .get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{k} is required"))?;
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(format!("{k} is longer than {MAX_QUERY_CHARS} characters"));
    }
    Ok(q.to_string())
}

fn limit_arg(a: &Map<String, Value>, default: u64, max: u64) -> u64 {
    a.get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(1, max)
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn tool_ok(v: Value) -> Value {
    let mut text = serde_json::to_string(&redact::value(v)).unwrap_or_else(|_| "{}".into());
    if text.len() > MAX_TOOL_TEXT {
        let mut end = MAX_TOOL_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(" …[cut]");
    }
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}

fn tool_error(msg: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": redact::text(msg) }], "isError": true })
}

fn obj_schema(props: &Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
}

/// MCP tool descriptor for one tool name.
#[must_use]
pub fn tool_spec(name: &str) -> Value {
    let limit = json!({ "type": "integer", "minimum": 1, "maximum": 50 });
    let (desc, schema) = match name {
        "memory_search" => (
            "Search OpenMind long-term memory (facts and notes saved from earlier chats). When no structured memory matches, matching conversation turns are attached.",
            obj_schema(
                &json!({ "query": { "type": "string" }, "limit": limit,
                         "type": { "type": "string" }, "scene": { "type": "string" } }),
                &["query"],
            ),
        ),
        "memory_conversations" => (
            "Search earlier conversation turns stored in OpenMind memory.",
            obj_schema(
                &json!({ "query": { "type": "string" }, "limit": limit,
                         "session_key": { "type": "string" } }),
                &["query"],
            ),
        ),
        "memory_recall" => (
            "Recall the memory context for a query (all sessions, or one session when session_key is given).",
            obj_schema(
                &json!({ "query": { "type": "string" }, "session_key": { "type": "string" } }),
                &["query"],
            ),
        ),
        "memory_save" => (
            "Save one short note to OpenMind long-term memory so it can be recalled later.",
            obj_schema(&json!({ "text": { "type": "string", "maxLength": MAX_SAVE_CHARS } }), &["text"]),
        ),
        "vault_list" => (
            "List notes in the read-only Obsidian vault (paths relative to the vault root).",
            obj_schema(&json!({ "folder": { "type": "string" }, "limit": limit }), &[]),
        ),
        "vault_read" => (
            "Read one note from the read-only Obsidian vault by its relative path.",
            obj_schema(
                &json!({ "path": { "type": "string" },
                         "max_bytes": { "type": "integer", "minimum": 1, "maximum": vault::READ_MAX_BYTES } }),
                &["path"],
            ),
        ),
        "vault_search" => (
            "Case-insensitive text search across notes in the read-only Obsidian vault.",
            obj_schema(&json!({ "query": { "type": "string" }, "limit": limit }), &["query"]),
        ),
        "stack_health" => (
            "Live health of the whole Galion/OpenMind stack from the nerve: system status, score and verdict, cortex state, and one line per service (unit state, HTTP, restarts, reasons). Pass component=<name> for one service's full record. Read-only.",
            obj_schema(&json!({ "component": { "type": "string" } }), &[]),
        ),
        "stack_alerts" => (
            "Newest stack alerts and open incidents (nerve) plus cortex alerts, newest first. Read-only.",
            obj_schema(&json!({ "limit": limit }), &[]),
        ),
        "cortex_heals" => (
            "Gaia cortex self-healing history: heal-verify stats (verified, failed, pending, success rate), recent heals and open escalations. Read-only.",
            obj_schema(&json!({ "limit": limit }), &[]),
        ),
        "spend_status" => (
            "Current OpenRouter spend from the Hermes ledger against the fresh $5 budget and the $4.50 stop line, today's spend and calls, top sessions. Check before any paid model call. Read-only.",
            obj_schema(&json!({}), &[]),
        ),
        "backup_status" => (
            "galion-backup status: newest archives (name, size, time), age of the newest, offsite encrypted copies, and any problem. Metadata only. Read-only.",
            obj_schema(&json!({}), &[]),
        ),
        "queue_status" => (
            "openmind-queue (POSEIDON work queue) health, depth (ready, leased, delayed, dead), load and per-agent budgets. Read-only.",
            obj_schema(&json!({}), &[]),
        ),
        "docs_list" => (
            "List notes in the whole Galion docs tree (the Obsidian vault folder build-session plus the sibling docs: one, platform, grok-computer, ...). Read-only.",
            obj_schema(&json!({ "folder": { "type": "string" }, "limit": limit }), &[]),
        ),
        "docs_read" => (
            "Read one note from the whole Galion docs tree by its path relative to the docs root (e.g. build-session/00-summary.md). Read-only.",
            obj_schema(
                &json!({ "path": { "type": "string" },
                         "max_bytes": { "type": "integer", "minimum": 1, "maximum": vault::READ_MAX_BYTES } }),
                &["path"],
            ),
        ),
        "docs_search" => (
            "Case-insensitive text search across the whole Galion docs tree (all notes, including the vault). Read-only.",
            obj_schema(&json!({ "query": { "type": "string" }, "limit": limit }), &["query"]),
        ),
        "hermes_readiness" => (
            "hermes-core readiness checks G1..G15 (score, pass/fail and why) from the last recorded run. A snapshot, not a live probe. Read-only.",
            obj_schema(&json!({}), &[]),
        ),
        "vault_note_create" => (
            "Create one NEW note in the vault inbox folder (never edits, renames or deletes an existing note). Only available when the owner switched it on.",
            obj_schema(
                &json!({ "title": { "type": "string", "maxLength": 120 },
                         "text": { "type": "string", "maxLength": notes::MAX_NOTE_CHARS } }),
                &["title", "text"],
            ),
        ),
        _ => (
            "Which OpenMind parts are configured: memory health, vault and docs note counts, which stack tools are on, and whether the (off-by-default) write tool is switched on.",
            obj_schema(&json!({}), &[]),
        ),
    };
    json!({ "name": name, "description": desc, "inputSchema": schema })
}

fn protocol_of(params: &Value) -> Option<&str> {
    match params.get("protocolVersion") {
        None => Some(DEFAULT_PROTOCOL),
        Some(Value::String(v)) => Some(if SUPPORTED_PROTOCOLS.contains(&v.as_str()) {
            v.as_str()
        } else {
            DEFAULT_PROTOCOL
        }),
        Some(_) => None,
    }
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn push_line(out: &mut Vec<u8>, value: &Value) {
    let line = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    out.extend_from_slice(&line);
    out.push(b'\n');
}

fn split_messages(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with("Content-Length:") && !l.starts_with("Content-Type:")
        })
        .collect()
}
