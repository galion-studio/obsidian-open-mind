// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Minimal MCP client for stdio servers (newline-delimited JSON-RPC 2.0):
//! `initialize` -> `notifications/initialized` -> `tools/list` (paged) ->
//! `tools/call`. Tools register as `mcp__<server>__<tool>` exactly like
//! upstream `mcp_prefixed_tool_name` (sanitised, clamped to 64 chars with a
//! sha256 suffix). Done: discovery at startup bounded by `connect_timeout`,
//! per-call `timeout`, one reconnect on a dead pipe. Stubbed: HTTP/SSE
//! transports, resources/prompts utility tools, sampling, hot reload.

use crate::config::McpServerCfg;
use crate::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: &str = "2025-03-26";
pub const PREFIX: &str = "mcp__";

pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// Upstream `mcp_prefixed_tool_name`.
pub fn prefixed_name(server: &str, tool: &str) -> String {
    let server = sanitize(server);
    let tool = sanitize(tool);
    let mut full = String::with_capacity(PREFIX.len() + server.len() + 2 + tool.len());
    full.push_str(PREFIX);
    full.push_str(&server);
    full.push_str("__");
    full.push_str(&tool);
    if full.len() <= 64 {
        return full;
    }
    let digest = sha256(full.as_bytes());
    let hex = galion_common::hex::encode(&digest[..4]);
    let mut out = String::with_capacity(64);
    out.push_str(&full[..55]);
    out.push('_');
    out.push_str(&hex);
    out
}

struct Conn {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
}

impl Drop for Conn {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct McpClient {
    pub cfg: McpServerCfg,
    conn: Mutex<Option<Conn>>,
}

impl McpClient {
    pub fn new(cfg: McpServerCfg) -> Self {
        McpClient {
            cfg,
            conn: Mutex::new(None),
        }
    }

    fn spawn(&self) -> Result<Conn, String> {
        let mut cmd = Command::new(&self.cfg.command);
        cmd.args(&self.cfg.args).env_clear();
        for k in ["PATH", "HOME", "LANG", "TZ", "USER", "HERMES_HOME"] {
            if let Ok(v) = std::env::var(k) {
                cmd.env(k, v);
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("mcp {}: spawn {}: {e}", self.cfg.name, self.cfg.command))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(v) = parse_line(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        let mut c = Conn {
            child,
            stdin,
            rx,
            next_id: 1,
        };
        let init = json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {},
                          "clientInfo": {"name": "hermes-rs", "version": crate::VERSION}});
        Self::rpc(
            &mut c,
            "initialize",
            init,
            Duration::from_secs(self.cfg.connect_timeout_s.max(1)),
        )?;
        Self::notify(&mut c, "notifications/initialized")?;
        Ok(c)
    }

    fn notify(c: &mut Conn, method: &str) -> Result<(), String> {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        writeln!(c.stdin, "{msg}")
            .and_then(|_| c.stdin.flush())
            .map_err(|e| e.to_string())
    }

    fn rpc(c: &mut Conn, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = c.next_id;
        c.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(c.stdin, "{msg}")
            .and_then(|_| c.stdin.flush())
            .map_err(|e| format!("write: {e}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match c.rx.recv_timeout(left) {
                Ok(v) if v["id"].as_u64() == Some(id) => {
                    if let Some(e) = v.get("error") {
                        return Err(format!(
                            "{method}: {}",
                            e["message"].as_str().unwrap_or("error")
                        ));
                    }
                    return Ok(v["result"].clone());
                }
                Ok(_) => continue, // notifications / server requests: ignored
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("{method}: timed out after {}s", timeout.as_secs()))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("{method}: server exited"))
                }
            }
        }
    }

    /// One request, (re)connecting as needed; retries once after a dead pipe.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let mut g = self.conn.lock().map_err(|_| "lock")?;
        for attempt in 0..2 {
            if g.is_none() {
                *g = Some(self.spawn()?);
            }
            let c = g.as_mut().ok_or("no conn")?;
            match Self::rpc(c, method, params.clone(), timeout) {
                Err(e)
                    if attempt == 0 && (e.contains("server exited") || e.starts_with("write")) =>
                {
                    *g = None
                }
                r => return r,
            }
        }
        Err(format!("{method}: unreachable"))
    }

    pub fn list_tools(&self) -> Result<Vec<Value>, String> {
        let t = Duration::from_secs(self.cfg.connect_timeout_s.max(1));
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..50 {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let r = self.request("tools/list", params, t)?;
            out.extend(r["tools"].as_array().cloned().unwrap_or_default());
            cursor = r["nextCursor"].as_str().map(String::from);
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }
}

pub struct McpTool {
    client: Arc<McpClient>,
    tool: String,
    name: String,
    description: String,
    schema: Value,
}

impl Tool for McpTool {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn description(&self) -> String {
        self.description.clone()
    }
    fn parameters(&self) -> Value {
        self.schema.clone()
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let args = if args.is_object() {
            args.clone()
        } else {
            json!({})
        };
        let t = Duration::from_secs(self.client.cfg.timeout_s.max(1));
        match self.client.request(
            "tools/call",
            json!({"name": self.tool, "arguments": args}),
            t,
        ) {
            Ok(r) => {
                let s = crate::redact(&tool_result_text(&r));
                if r["isError"] == true {
                    ToolOutput::err(s)
                } else {
                    ToolOutput::ok(s)
                }
            }
            Err(e) => ToolOutput::err(format!("MCP {}: {e}", self.client.cfg.name)),
        }
    }
}

/// Connect each server and turn its tools into registry tools. Failures are
/// returned per server, never fatal.
pub fn discover(servers: &[McpServerCfg]) -> (Vec<Arc<dyn Tool>>, Vec<String>) {
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    let mut errors = Vec::new();
    for s in servers {
        let client = Arc::new(McpClient::new(s.clone()));
        match client.list_tools() {
            Ok(list) => {
                for t in list {
                    let Some(tn) = t["name"].as_str() else {
                        continue;
                    };
                    let mut schema = t
                        .get("inputSchema")
                        .cloned()
                        .filter(Value::is_object)
                        .unwrap_or(json!({}));
                    if schema.get("type").is_none() {
                        schema["type"] = json!("object");
                    }
                    if schema.get("properties").is_none() {
                        schema["properties"] = json!({});
                    }
                    tools.push(Arc::new(McpTool {
                        client: client.clone(),
                        tool: tn.to_string(),
                        name: prefixed_name(&s.name, tn),
                        description: t["description"]
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| format!("MCP tool {tn} from {}", s.name)),
                        schema,
                    }));
                }
            }
            Err(e) => errors.push(format!("{}: {e}", s.name)),
        }
    }
    (tools, errors)
}

/// One newline-delimited JSON-RPC message from a stdio MCP server.
///
/// Blank lines and lines that are not JSON are [`None`]. The reader thread
/// uses this and drops those lines.
#[must_use]
pub fn parse_line(line: &str) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// Text kept from a `tools/call` result, before redaction.
///
/// Text blocks are joined with newlines. Another block becomes
/// `[<type> content]`. A block with no `type` is its JSON. When `content`
/// is missing or empty, `structuredContent` is used instead. `isError` is
/// not read here.
#[must_use]
pub fn tool_result_text(result: &Value) -> String {
    let text: Vec<String> = result["content"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| match item["type"].as_str() {
                    Some("text") => item["text"].as_str().unwrap_or("").to_string(),
                    Some(kind) => format!("[{kind} content]"),
                    None => item.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut text = text.join("\n");
    if text.is_empty() {
        if let Some(structured) = result.get("structuredContent") {
            text = structured.to_string();
        }
    }
    text
}

/// FIPS 180-4 SHA-256 (only used for the 64-char name clamp). Shared
/// implementation from galion-common.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    galion_common::sha256::digest(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_upstream() {
        let hex = |b: [u8; 32]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(
            hex(sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            prefixed_name("galion-stack", "stack.status"),
            "mcp__galion_stack__stack_status"
        );
        let long = prefixed_name(&"s".repeat(40), &"t".repeat(40));
        assert_eq!(long.len(), 64);
        assert_eq!(&long[55..56], "_");
        let full = format!("mcp__{}__{}", "s".repeat(40), "t".repeat(40));
        let hex = galion_common::hex::encode(&sha256(full.as_bytes())[..4]);
        assert_eq!(&long[56..], hex);
    }

    #[test]
    fn line_and_tool_result_parse() {
        assert!(parse_line("  ").is_none());
        assert!(parse_line("not-json").is_none());
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        assert_eq!(parse_line(line).unwrap()["id"], 1);

        let blocks = json!({
            "content": [
                {"type": "text", "text": "a"},
                {"type": "image"},
                {"type": "text", "text": "b"}
            ]
        });
        assert_eq!(tool_result_text(&blocks), "a\n[image content]\nb");
        let structured = json!({"content": [], "structuredContent": {"n": 1}});
        assert_eq!(tool_result_text(&structured), "{\"n\":1}");
        let bare = json!({"content": [1]});
        assert_eq!(tool_result_text(&bare), "1");
    }
}
