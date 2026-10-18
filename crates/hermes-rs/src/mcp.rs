// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

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
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Upstream `mcp_prefixed_tool_name`.
pub fn prefixed_name(server: &str, tool: &str) -> String {
    let full = format!("{PREFIX}{}__{}", sanitize(server), sanitize(tool));
    if full.len() <= 64 {
        return full;
    }
    let hex: String = sha256(full.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let suffix = format!("_{}", &hex[..8]);
    format!("{}{}", &full[..64 - suffix.len()], suffix)
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
                if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
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
                let text: Vec<String> = r["content"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|c| match c["type"].as_str() {
                                Some("text") => c["text"].as_str().unwrap_or("").to_string(),
                                Some(k) => format!("[{k} content]"),
                                None => c.to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut s = text.join("\n");
                if s.is_empty() {
                    if let Some(sc) = r.get("structuredContent") {
                        s = sc.to_string();
                    }
                }
                let s = crate::redact(&s);
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

/// FIPS 180-4 SHA-256 (only used for the 64-char name clamp).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
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
    }
}
