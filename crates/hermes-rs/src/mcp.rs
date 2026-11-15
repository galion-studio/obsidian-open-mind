// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! MCP client for Hermes: stdio (newline-delimited JSON-RPC 2.0) and
//! streamable HTTP (protocol 2025-03-26).
//!
//! `initialize` -> `notifications/initialized` -> `tools/list` (paged) ->
//! `tools/call`. Tools register as `mcp__<server>__<tool>` exactly like
//! upstream `mcp_prefixed_tool_name` (sanitised, clamped to 64 chars with a
//! sha256 suffix). Each server has a tool allowlist. The GitHub entry is
//! clamped to read-only tools; its token is read from the environment at
//! request time and is never stored on the config.
//!
//! Streamable HTTP sends one POST per JSON-RPC message, accepts
//! `application/json` or `text/event-stream`, and sends `Mcp-Session-Id`
//! back when the server assigned one. Cleartext HTTP is loopback only.
//! Both schemes go through the system `curl` (no TLS crate, and no second
//! HTTP stack in the gateway). The token travels in the curl config on
//! stdin, never in argv. A URL that already contains a token is dropped
//! in config and is not stored. The legacy HTTP+SSE
//! transport (protocol 2024-11-05) is not implemented.

use crate::config::{header_safe, valid_env_name, McpServerCfg, ToolAllow};
use crate::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
#[cfg(any(test, feature = "mcp-http"))]
use std::io::Read;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: &str = "2025-03-26";
pub const PREFIX: &str = "mcp__";

/// Largest streamable-HTTP response body Hermes will accept.
#[cfg(any(test, feature = "mcp-http"))]
const MAX_HTTP_BYTES: usize = 8 * 1024 * 1024;

pub type EnvFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

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

struct StdioConn {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_id: u64,
}

impl Drop for StdioConn {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(any(test, feature = "mcp-http"))]
#[derive(Debug)]
struct Endpoint {
    https: bool,
    /// Checked by `parse_endpoint` and by the unit tests. `curl` is given
    /// the original URL, so these fields are not read on the request path.
    #[cfg_attr(not(test), allow(dead_code))]
    host: String,
    #[cfg_attr(not(test), allow(dead_code))]
    port: u16,
    #[cfg_attr(not(test), allow(dead_code))]
    path: String,
}

#[cfg(any(test, feature = "mcp-http"))]
struct HttpConn {
    ep: Endpoint,
    session_id: Option<String>,
    next_id: u64,
}

enum Link {
    Stdio(StdioConn),
    #[cfg(any(test, feature = "mcp-http"))]
    Http(HttpConn),
}

#[cfg(any(test, feature = "mcp-http"))]
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub struct McpClient {
    pub cfg: McpServerCfg,
    conn: Mutex<Option<Link>>,
    env: EnvFn,
    #[cfg(any(test, feature = "mcp-http"))]
    curl: String,
}

impl McpClient {
    pub fn new(cfg: McpServerCfg) -> Self {
        Self::with_env(cfg, Arc::new(|k| std::env::var(k).ok()))
    }

    pub fn with_env(cfg: McpServerCfg, env: EnvFn) -> Self {
        McpClient {
            cfg,
            conn: Mutex::new(None),
            env,
            #[cfg(any(test, feature = "mcp-http"))]
            curl: "curl".into(),
        }
    }

    fn token(&self) -> Result<Option<String>, String> {
        let Some(name) = &self.cfg.token_env else {
            return Ok(None);
        };
        if !valid_env_name(name) {
            return Err("token_env is not an environment variable name".into());
        }
        match (self.env)(name)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            Some(v) if header_safe(&v) => Ok(Some(v)),
            Some(_) => Err(format!("{name} has unusable characters")),
            None => Err(format!("{name} is unset")),
        }
    }

    #[cfg(any(test, feature = "mcp-http"))]
    fn loopback_only(&self) -> bool {
        (self.env)("HERMES_RS_PROVIDER_LOOPBACK_ONLY")
            .is_some_and(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
    }

    fn spawn_stdio(&self) -> Result<StdioConn, String> {
        let mut cmd = Command::new(&self.cfg.command);
        cmd.args(&self.cfg.args).env_clear();
        for k in ["PATH", "HOME", "LANG", "TZ", "USER", "HERMES_HOME"] {
            if let Ok(v) = std::env::var(k) {
                cmd.env(k, v);
            }
        }
        if let Some(tok) = self.token()? {
            if let Some(name) = &self.cfg.token_env {
                cmd.env(name, tok);
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
        let mut c = StdioConn {
            child,
            stdin,
            rx,
            next_id: 1,
        };
        let init = json!({"protocolVersion": PROTOCOL_VERSION, "capabilities": {},
                          "clientInfo": {"name": "hermes-rs", "version": crate::VERSION}});
        stdio_rpc(
            &mut c,
            "initialize",
            &init,
            Duration::from_secs(self.cfg.connect_timeout_s.max(1)),
        )?;
        stdio_notify(&mut c, "notifications/initialized")?;
        Ok(c)
    }

    #[cfg(any(test, feature = "mcp-http"))]
    #[inline(never)]
    fn open_http(&self) -> Result<HttpConn, String> {
        let ep = parse_endpoint(self.cfg.url.as_deref().unwrap_or(""))?;
        if ep.https && self.loopback_only() {
            return Err(
                "HERMES_RS_PROVIDER_LOOPBACK_ONLY is set: refusing non-loopback MCP URL".into(),
            );
        }
        let mut link = HttpConn {
            ep,
            session_id: None,
            next_id: 1,
        };
        let t = Duration::from_secs(self.cfg.connect_timeout_s.max(1));
        let mut init = String::from(r#"{"protocolVersion":""#);
        init.push_str(PROTOCOL_VERSION);
        init.push_str(r#"","capabilities":{},"clientInfo":{"name":"hermes-rs","version":""#);
        init.push_str(crate::VERSION);
        init.push_str(r#""}}"#);
        self.http_rpc(&mut link, "initialize", &init, t)?;
        self.http_notify(&mut link, t)?;
        Ok(link)
    }

    #[cfg(any(test, feature = "mcp-http"))]
    fn open_url(&self) -> Result<Link, String> {
        self.open_http().map(Link::Http)
    }

    #[cfg(not(any(test, feature = "mcp-http")))]
    fn open_url(&self) -> Result<Link, String> {
        Err("streamable HTTP needs the mcp-http feature".into())
    }

    fn open(&self) -> Result<Link, String> {
        if self.cfg.url.is_some() {
            self.open_url()
        } else if !self.cfg.command.is_empty() {
            self.spawn_stdio().map(Link::Stdio)
        } else {
            Err("mcp server has no command and no url".into())
        }
    }

    #[cfg(any(test, feature = "mcp-http"))]
    fn headers_for(&self, link: &HttpConn, bearer: Option<&str>) -> Vec<(String, String)> {
        let mut h = vec![
            ("Content-Type".into(), "application/json".into()),
            (
                "Accept".into(),
                "application/json, text/event-stream".into(),
            ),
            ("MCP-Protocol-Version".into(), PROTOCOL_VERSION.into()),
        ];
        if let Some(id) = &link.session_id {
            h.push(("Mcp-Session-Id".into(), id.clone()));
        }
        if let Some(tok) = bearer {
            h.push(("Authorization".into(), format!("Bearer {tok}")));
        }
        for (k, v) in &self.cfg.headers {
            if extra_header(k) {
                h.push((k.clone(), v.clone()));
            }
        }
        h
    }

    #[cfg(any(test, feature = "mcp-http"))]
    #[inline(never)]
    fn post(
        &self,
        link: &HttpConn,
        body: &str,
        timeout: Duration,
        initialize: bool,
    ) -> Result<Reply, String> {
        let bearer = self.token()?;
        let headers = self.headers_for(link, bearer.as_deref());
        let reply = self.curl_post(body, &headers, link.ep.https, timeout)?;
        if reply.status == 404 && !initialize {
            return Err("server exited".into());
        }
        if !(200..300).contains(&reply.status) {
            let snippet: String = String::from_utf8_lossy(&reply.body)
                .chars()
                .take(200)
                .collect();
            return Err(crate::redact(&format!("http {}: {snippet}", reply.status)));
        }
        Ok(reply)
    }

    /// One POST through `curl` for both loopback `http` and `https`. The
    /// gateway already links `curl` for model calls. Using it here keeps the
    /// galion-common HTTP client out of this binary.
    #[cfg(any(test, feature = "mcp-http"))]
    #[inline(never)]
    fn curl_post(
        &self,
        body: &str,
        headers: &[(String, String)],
        https: bool,
        timeout: Duration,
    ) -> Result<Reply, String> {
        let url = self.cfg.url.as_deref().unwrap_or("");
        let secs = timeout.as_secs().max(1);
        let connect = secs.min(self.cfg.connect_timeout_s.max(1));
        let args = curl_args(https, connect, secs);
        let mut child = Command::new(&self.curl)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("write: curl {e}"))?;
        let cfg = curl_config(url, body, headers);
        let wrote = match child.stdin.take() {
            Some(mut si) => si
                .write_all(cfg.as_bytes())
                .map_err(|e| format!("write: curl {e}")),
            None => Err("write: curl stdin".into()),
        };
        if let Err(e) = wrote {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        let mut buf = Vec::new();
        let read = match child.stdout.take() {
            Some(mut so) => read_capped(&mut so, &mut buf, MAX_HTTP_BYTES),
            None => Err("write: curl stdout".into()),
        };
        let _ = child.wait();
        read?;
        if buf.is_empty() {
            return Err("write: curl empty".into());
        }
        split_http(&buf)
    }

    #[cfg(any(test, feature = "mcp-http"))]
    fn http_notify(&self, link: &mut HttpConn, timeout: Duration) -> Result<(), String> {
        // A notification has no id. 202 with an empty body is success.
        let _ = self.post(
            link,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            timeout,
            false,
        )?;
        Ok(())
    }

    #[cfg(any(test, feature = "mcp-http"))]
    #[inline(never)]
    fn http_rpc(
        &self,
        link: &mut HttpConn,
        method: &str,
        params: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = link.next_id;
        link.next_id += 1;
        let mut msg = String::from(r#"{"jsonrpc":"2.0","id":"#);
        msg.push_str(&id.to_string());
        msg.push_str(r#","method":""#);
        msg.push_str(method);
        msg.push_str(r#"","params":"#);
        msg.push_str(params);
        msg.push('}');
        let reply = self.post(link, &msg, timeout, method == "initialize")?;
        if let Some(sid) = session_from(&reply.headers) {
            link.session_id = Some(sid);
        }
        let messages = messages_from(&reply)?;
        pick(&messages, id).map_err(|e| format!("{method}: {e}"))
    }

    fn rpc(
        &self,
        link: &mut Link,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        match link {
            Link::Stdio(c) => stdio_rpc(c, method, &params, timeout),
            #[cfg(any(test, feature = "mcp-http"))]
            Link::Http(c) => {
                let params = serde_json::to_string(&params).map_err(|e| e.to_string())?;
                self.http_rpc(c, method, &params, timeout)
            }
        }
    }

    /// One request, (re)connecting as needed; retries once after a dead pipe
    /// or a streamable-HTTP session the server has forgotten (HTTP 404).
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let mut g = self.conn.lock().map_err(|_| "lock")?;
        for attempt in 0..2 {
            if g.is_none() {
                *g = Some(self.open()?);
            }
            let link = g.as_mut().ok_or("no conn")?;
            match self.rpc(link, method, params.clone(), timeout) {
                Err(e) if attempt == 0 && dead(&e) => *g = None,
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

fn stdio_notify(c: &mut StdioConn, method: &str) -> Result<(), String> {
    let msg = json!({"jsonrpc": "2.0", "method": method});
    writeln!(c.stdin, "{msg}")
        .and_then(|_| c.stdin.flush())
        .map_err(|e| e.to_string())
}

fn stdio_rpc(
    c: &mut StdioConn,
    method: &str,
    params: &Value,
    timeout: Duration,
) -> Result<Value, String> {
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
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!("{method}: timed out after {}s", timeout.as_secs()))
            }
            Err(RecvTimeoutError::Disconnected) => return Err(format!("{method}: server exited")),
        }
    }
}

fn dead(e: &str) -> bool {
    e.contains("server exited") || e.starts_with("write")
}

#[cfg(any(test, feature = "mcp-http"))]
fn extra_header(name: &str) -> bool {
    !matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "host"
            | "content-length"
            | "content-type"
            | "accept"
            | "connection"
            | "mcp-protocol-version"
            | "mcp-session-id"
    )
}

#[cfg(any(test, feature = "mcp-http"))]
fn push_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// curl arguments for one MCP POST. Redirects stay off (`--max-redirs 0`)
/// and `--proto` locks the scheme parsed from the URL. The URL and the
/// token are not arguments; they go in the config on stdin.
#[cfg(any(test, feature = "mcp-http"))]
fn curl_args(https: bool, connect_s: u64, max_s: u64) -> Vec<String> {
    vec![
        "-sS".into(),
        "-i".into(),
        "--http1.1".into(),
        "--max-redirs".into(),
        "0".into(),
        "--proto".into(),
        if https { "=https" } else { "=http" }.into(),
        "--connect-timeout".into(),
        connect_s.to_string(),
        "--max-time".into(),
        max_s.to_string(),
        "-K".into(),
        "-".into(),
    ]
}

#[cfg(any(test, feature = "mcp-http"))]
fn curl_config(url: &str, body: &str, headers: &[(String, String)]) -> String {
    let mut cfg = String::with_capacity(64 + url.len() + body.len());
    cfg.push_str("url = ");
    push_quoted(&mut cfg, url);
    cfg.push_str("\nrequest = \"POST\"\n");
    for (k, v) in headers {
        cfg.push_str("header = \"");
        cfg.push_str(k);
        cfg.push_str(": ");
        cfg.push_str(v);
        cfg.push_str("\"\n");
    }
    cfg.push_str("data-binary = ");
    push_quoted(&mut cfg, body);
    cfg.push('\n');
    cfg
}

#[cfg(any(test, feature = "mcp-http"))]
fn read_capped(r: &mut impl Read, buf: &mut Vec<u8>, max: usize) -> Result<(), String> {
    let mut chunk = [0u8; 4096];
    loop {
        let n = r.read(&mut chunk).map_err(|e| format!("write: curl {e}"))?;
        if n == 0 {
            return Ok(());
        }
        if buf.len() + n > max {
            return Err("http: response too large".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

#[cfg(any(test, feature = "mcp-http"))]
fn header_get<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[cfg(any(test, feature = "mcp-http"))]
fn split_http(raw: &[u8]) -> Result<Reply, String> {
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
        .ok_or("bad http response")?;
    let head = String::from_utf8_lossy(&raw[..head_end]);
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("bad http response")?;
    let mut headers = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r');
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let mut body = raw[head_end..].to_vec();
    if let Some(n) = header_get(&headers, "content-length").and_then(|s| s.parse().ok()) {
        body.truncate(n);
    }
    Ok(Reply {
        status,
        headers,
        body,
    })
}

#[cfg(any(test, feature = "mcp-http"))]
fn session_from(headers: &[(String, String)]) -> Option<String> {
    let v = header_get(headers, "mcp-session-id")?;
    if (1..=256).contains(&v.len())
        && header_safe(v)
        && v.bytes().all(|c| (0x21..=0x7e).contains(&c))
    {
        Some(v.to_string())
    } else {
        None
    }
}

#[cfg(any(test, feature = "mcp-http"))]
fn messages_from(reply: &Reply) -> Result<Vec<Value>, String> {
    if reply.status == 202 && reply.body.is_empty() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&reply.body);
    let ct = header_get(&reply.headers, "content-type").unwrap_or("");
    if ct.to_ascii_lowercase().contains("text/event-stream") {
        return Ok(parse_sse(&text));
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Array(a)) => Ok(a),
        Ok(v) => Ok(vec![v]),
        Err(_) if ct.is_empty() => Ok(parse_sse(&text)),
        Err(e) => Err(format!("bad json-rpc: {e}")),
    }
}

#[cfg(any(test, feature = "mcp-http"))]
fn pick(messages: &[Value], id: u64) -> Result<Value, String> {
    for m in messages {
        if m.get("id").and_then(|v| v.as_u64()) != Some(id) {
            continue;
        }
        if let Some(e) = m.get("error") {
            return Err(e["message"].as_str().unwrap_or("error").to_string());
        }
        return Ok(m["result"].clone());
    }
    Err("no response".into())
}

#[cfg(any(test, feature = "mcp-http"))]
fn push_sse_data(data: &mut String, out: &mut Vec<Value>) {
    if data.is_empty() {
        return;
    }
    let text = std::mem::take(data);
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        out.push(v);
    }
}

#[cfg(any(test, feature = "mcp-http"))]
fn parse_sse(body: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut data = String::new();
    for line in body.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            push_sse_data(&mut data, &mut out);
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    push_sse_data(&mut data, &mut out);
    out
}

#[cfg(any(test, feature = "mcp-http"))]
#[inline(never)]
fn parse_endpoint(url: &str) -> Result<Endpoint, String> {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("mcp url must start with http:// or https://".into());
    };
    if rest.is_empty() || rest.contains('@') || rest.contains('#') {
        return Err("mcp url must not contain userinfo or a fragment".into());
    }
    let (authority, path_and_query) = match rest.find('/') {
        Some(0) => return Err("bad mcp url".into()),
        Some(i) => (&rest[..i], &rest[i..]),
        None => match rest.find('?') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        },
    };
    let path = if path_and_query.is_empty() {
        "/".to_string()
    } else if let Some(q) = path_and_query.strip_prefix('?') {
        format!("/?{q}")
    } else {
        path_and_query.to_string()
    };
    if path.bytes().any(|c| !(0x21..=0x7e).contains(&c)) {
        return Err("bad mcp url".into());
    }
    let (host, port) = split_host_port(authority, https)?;
    if !https && !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return Err("cleartext mcp url is only allowed for loopback".into());
    }
    Ok(Endpoint {
        https,
        host,
        port,
        path,
    })
}

#[cfg(any(test, feature = "mcp-http"))]
fn split_host_port(authority: &str, https: bool) -> Result<(String, u16), String> {
    let default = if https { 443 } else { 80 };
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or("bad mcp url")?;
        if host.is_empty() {
            return Err("bad mcp url".into());
        }
        let port = if after.is_empty() {
            default
        } else {
            let p = after.strip_prefix(':').ok_or("bad mcp url")?;
            parse_port(p)?
        };
        return Ok((host.to_string(), port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            return Err("bad mcp url".into());
        }
        return Ok((host.to_string(), parse_port(port)?));
    }
    if authority.is_empty() {
        return Err("bad mcp url".into());
    }
    Ok((authority.to_string(), default))
}

#[cfg(any(test, feature = "mcp-http"))]
fn parse_port(port: &str) -> Result<u16, String> {
    if port.is_empty() || !port.bytes().all(|c| c.is_ascii_digit()) {
        return Err("bad mcp port".into());
    }
    let port: u16 = port.parse().map_err(|_| "bad mcp port".to_string())?;
    if port == 0 {
        return Err("bad mcp port".into());
    }
    Ok(port)
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
        if !self.client.cfg.allow.permits(&self.tool) {
            return ToolOutput::err(format!(
                "MCP {}: tool {} is not on the allowlist",
                self.client.cfg.name, self.tool
            ));
        }
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
/// returned per server, never fatal. Tools outside the server's allowlist
/// are not registered.
pub fn discover(servers: &[McpServerCfg]) -> (Vec<Arc<dyn Tool>>, Vec<String>) {
    discover_with(servers, Arc::new(|k| std::env::var(k).ok()))
}

pub fn discover_with(servers: &[McpServerCfg], env: EnvFn) -> (Vec<Arc<dyn Tool>>, Vec<String>) {
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    let mut errors = Vec::new();
    for s in servers {
        if let ToolAllow::Only(names) = &s.allow {
            if names.is_empty() {
                errors.push(format!("{}: allowlist is empty", s.name));
                continue;
            }
        }
        let client = Arc::new(McpClient::with_env(s.clone(), env.clone()));
        match client.list_tools() {
            Ok(list) => {
                let listed = !list.is_empty();
                let mut added = 0;
                for t in list {
                    let Some(tn) = t["name"].as_str() else {
                        continue;
                    };
                    if !s.allow.permits(tn) {
                        continue;
                    }
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
                    added += 1;
                }
                if added == 0 && listed {
                    errors.push(format!("{}: no listed tool is on the allowlist", s.name));
                }
            }
            Err(e) => errors.push(format!("{}: {e}", s.name)),
        }
    }
    (tools, errors)
}

/// FIPS 180-4 SHA-256 (only used for the 64-char name clamp). Shared
/// implementation from galion-common.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    galion_common::sha256::digest(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{github_readonly_server, GITHUB_MCP_URL};
    use crate::tools::ToolCtx;
    use std::sync::atomic::AtomicBool;

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

    #[test]
    fn endpoint_rules() {
        let gh = parse_endpoint(GITHUB_MCP_URL).unwrap();
        assert!(gh.https);
        assert_eq!(gh.host, "api.githubcopilot.com");
        assert_eq!(gh.port, 443);
        assert_eq!(gh.path, "/mcp/x/all/readonly");
        let local = parse_endpoint("http://127.0.0.1/mcp").unwrap();
        assert!(!local.https);
        assert_eq!(local.port, 80);
        assert_eq!(local.path, "/mcp");
        let v6 = parse_endpoint("http://[::1]:9/mcp").unwrap();
        assert_eq!(v6.host, "::1");
        assert_eq!(v6.port, 9);
        assert!(parse_endpoint("http://example.com/mcp")
            .unwrap_err()
            .contains("loopback"));
        assert!(parse_endpoint("http://user:secret@127.0.0.1/mcp").is_err());
        assert!(parse_endpoint("ftp://127.0.0.1/mcp").is_err());
    }

    #[test]
    fn sse_skips_notifications_and_joins_data() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n: keep-alive\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":7,\"result\":{\"ok\":true}}\n\n";
        let msgs = parse_sse(body);
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].get("id").is_none());
        assert_eq!(msgs[1]["id"], 7);
        assert_eq!(msgs[1]["result"]["ok"], true);
    }

    #[test]
    fn curl_args_disable_redirects_and_lock_the_scheme() {
        let https = curl_args(true, 15, 60);
        let http = curl_args(false, 5, 5);
        assert_eq!(
            https
                .iter()
                .position(|a| a == "--max-redirs")
                .map(|i| https[i + 1].as_str()),
            Some("0")
        );
        assert_eq!(
            https
                .iter()
                .position(|a| a == "--proto")
                .map(|i| https[i + 1].as_str()),
            Some("=https")
        );
        assert_eq!(
            http.iter()
                .position(|a| a == "--proto")
                .map(|i| http[i + 1].as_str()),
            Some("=http")
        );
        assert!(https.iter().any(|a| a == "-K"));
        assert!(https
            .iter()
            .all(|a| !a.contains("Bearer") && !a.contains("://")));
        assert!(http
            .iter()
            .all(|a| !a.contains("Bearer") && !a.contains("://")));
    }

    #[test]
    fn session_id_rejects_whitespace_and_overlong_values() {
        let ok = session_from(&[("Mcp-Session-Id".into(), "sess-1".into())]);
        assert_eq!(ok.as_deref(), Some("sess-1"));
        assert!(session_from(&[("Mcp-Session-Id".into(), "bad id".into())]).is_none());
        assert!(session_from(&[("Mcp-Session-Id".into(), "bad\nid".into())]).is_none());
        assert!(session_from(&[("Mcp-Session-Id".into(), String::new())]).is_none());
        assert!(session_from(&[("Mcp-Session-Id".into(), "a".repeat(257))]).is_none());
    }

    #[test]
    fn empty_allowlist_does_not_spawn() {
        let cfg = McpServerCfg {
            allow: ToolAllow::Only(vec![]),
            ..McpServerCfg::stdio("empty", "/nonexistent/mcp", vec![])
        };
        let (tools, errors) = discover(&[cfg]);
        assert!(tools.is_empty());
        assert_eq!(errors, vec!["empty: allowlist is empty".to_string()]);
    }

    #[test]
    fn github_write_only_allowlist_does_not_connect() {
        let mut cfg = McpServerCfg::http("github", "http://localhost:1/mcp");
        cfg.allow = ToolAllow::Only(vec!["push_files".into()]);
        crate::config::clamp_github_readonly(&mut cfg);
        let (tools, errors) = discover(&[cfg]);
        assert!(tools.is_empty());
        assert_eq!(errors, vec!["github: allowlist is empty".to_string()]);
    }

    #[test]
    fn curl_config_keeps_the_token_out_of_the_url() {
        let cfg = curl_config(
            GITHUB_MCP_URL,
            "{\"a\":\"b\"}",
            &[
                (
                    "Accept".into(),
                    "application/json, text/event-stream".into(),
                ),
                ("Authorization".into(), "Bearer secret-token".into()),
            ],
        );
        assert!(cfg.contains(&format!(
            "url = {}",
            crate::provider::curl_quote(GITHUB_MCP_URL)
        )));
        assert!(cfg.contains("Authorization: Bearer secret-token"));
        assert!(cfg.contains("data-binary"));
        assert!(!cfg.lines().next().unwrap().contains("secret-token"));
        assert!(cfg.contains("\\\"a\\\""));
    }

    #[test]
    fn call_refuses_a_tool_outside_the_allowlist_without_connecting() {
        let mut cfg = McpServerCfg::http("mock", "http://127.0.0.1:1/mcp");
        cfg.allow = ToolAllow::Only(vec!["get_file_contents".into()]);
        let client = Arc::new(McpClient::new(cfg));
        let tool = McpTool {
            client,
            tool: "push_files".into(),
            name: "mcp__mock__push_files".into(),
            description: "write".into(),
            schema: json!({}),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let emit = |_e| {};
        let ctx = ToolCtx {
            session_id: "s",
            spend_session: "s",
            depth: 0,
            user_text: "",
            cancel: &cancel,
            emit: &emit,
        };
        let out = tool.call(&json!({}), &ctx);
        assert!(out.is_error, "{out:?}");
        assert!(out.content.contains("allowlist"), "{out:?}");
        assert!(!out.content.contains("connect"), "{out:?}");
    }

    #[test]
    fn github_entry_does_not_dial_when_loopback_only() {
        let env: EnvFn = Arc::new(|k| match k {
            "HERMES_RS_PROVIDER_LOOPBACK_ONLY" => Some("1".into()),
            _ => None,
        });
        let mut client = McpClient::with_env(github_readonly_server(), env);
        client.curl = "/nonexistent/curl-mcp".into();
        let err = client.list_tools().unwrap_err();
        assert!(err.contains("HERMES_RS_PROVIDER_LOOPBACK_ONLY"), "{err}");
        assert!(!err.contains(concat!("gh", "p_")), "{err}");
    }

    #[test]
    fn unusable_token_is_refused_and_not_echoed() {
        let bad = concat!("bad\n", "token-value");
        let env: EnvFn = Arc::new(move |k| {
            if k == "HERMES_RS_TEST_MCP_TOKEN" {
                Some(bad.to_string())
            } else {
                None
            }
        });
        let mut cfg = McpServerCfg::http("mock", "http://127.0.0.1:1/mcp");
        cfg.token_env = Some("HERMES_RS_TEST_MCP_TOKEN".into());
        let client = McpClient::with_env(cfg, env);
        let err = client.list_tools().unwrap_err();
        assert!(err.contains("unusable"), "{err}");
        assert!(!err.contains("token-value"), "{err}");
    }

    include!("mcp_http_live.rs");
}
