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
//! HTTPS goes through the system `curl` (no TLS crate); the token travels
//! in the curl config on stdin, never in argv. The legacy HTTP+SSE
//! transport (protocol 2024-11-05) is not implemented.

use crate::config::{header_safe, valid_env_name, McpServerCfg, ToolAllow};
use crate::provider::curl_quote;
use crate::tools::{Tool, ToolCtx, ToolOutput};
use galion_common::http::{self, ClientRequest};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSION: &str = "2025-03-26";
pub const PREFIX: &str = "mcp__";

/// Largest streamable-HTTP response body Hermes will accept.
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

#[derive(Debug)]
struct Endpoint {
    https: bool,
    host: String,
    port: u16,
    path: String,
}

struct HttpConn {
    ep: Endpoint,
    session_id: Option<String>,
    next_id: u64,
}

enum Link {
    Stdio(StdioConn),
    Http(HttpConn),
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub struct McpClient {
    pub cfg: McpServerCfg,
    conn: Mutex<Option<Link>>,
    env: EnvFn,
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
        let init = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "hermes-rs", "version": crate::VERSION}
        });
        self.http_rpc(&mut link, "initialize", init, t)?;
        self.http_notify(&mut link, t)?;
        Ok(link)
    }

    fn open(&self) -> Result<Link, String> {
        if self.cfg.url.is_some() {
            self.open_http().map(Link::Http)
        } else if !self.cfg.command.is_empty() {
            self.spawn_stdio().map(Link::Stdio)
        } else {
            Err("mcp server has no command and no url".into())
        }
    }

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

    fn post(
        &self,
        link: &HttpConn,
        body: &str,
        timeout: Duration,
        initialize: bool,
    ) -> Result<Reply, String> {
        let bearer = self.token()?;
        let headers = self.headers_for(link, bearer.as_deref());
        let reply = if link.ep.https {
            self.post_https(body, &headers, timeout)?
        } else {
            post_loopback(link, body, &headers, timeout)?
        };
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

    fn post_https(
        &self,
        body: &str,
        headers: &[(String, String)],
        timeout: Duration,
    ) -> Result<Reply, String> {
        let url = self.cfg.url.as_deref().unwrap_or("");
        let secs = timeout.as_secs().max(1);
        let connect = secs.min(self.cfg.connect_timeout_s.max(1));
        let mut child = Command::new(&self.curl)
            .args([
                "-sS",
                "-i",
                "-N",
                "--http1.1",
                "--max-redirs",
                "0",
                "--proto",
                "=https",
                "--connect-timeout",
            ])
            .arg(connect.to_string())
            .arg("--max-time")
            .arg(secs.to_string())
            .args(["-K", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        let write_result = (|| -> Result<(), String> {
            let mut si = child.stdin.take().ok_or("curl stdin")?;
            let cfg = curl_config(url, body, headers);
            si.write_all(cfg.as_bytes())
                .map_err(|e| format!("curl: {e}"))
        })();
        if let Err(e) = write_result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        let stdout = child.stdout.take().ok_or("curl stdout")?;
        let stderr = child.stderr.take();
        let err_thread = std::thread::spawn(move || {
            let mut e = String::new();
            if let Some(mut p) = stderr {
                let _ = std::io::Read::read_to_string(&mut p, &mut e);
            }
            e
        });
        let buf = match read_capped(stdout, MAX_HTTP_BYTES) {
            Ok(b) => b,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = err_thread.join();
                return Err(e);
            }
        };
        let status = child.wait().map_err(|e| format!("curl: {e}"))?;
        let err = err_thread.join().unwrap_or_default();
        if buf.is_empty() {
            return Err(crate::redact(&format!(
                "curl: exit {:?} {}",
                status.code(),
                err.trim()
            )));
        }
        split_http(&buf)
    }

    fn http_notify(&self, link: &mut HttpConn, timeout: Duration) -> Result<(), String> {
        let msg = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        // A notification has no id. 202 with an empty body is success.
        let _ = self.post(link, &msg.to_string(), timeout, false)?;
        Ok(())
    }

    fn http_rpc(
        &self,
        link: &mut HttpConn,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = link.next_id;
        link.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let reply = self.post(link, &msg.to_string(), timeout, method == "initialize")?;
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
            Link::Http(c) => self.http_rpc(c, method, params, timeout),
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
    e.contains("server exited")
        || e.starts_with("write")
        || e.contains("http: connect")
        || e.contains("http: read")
        || e.contains("http: write")
        || e.starts_with("curl:")
}

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

fn post_loopback(
    link: &HttpConn,
    body: &str,
    headers: &[(String, String)],
    timeout: Duration,
) -> Result<Reply, String> {
    let addr = match link.ep.host.as_str() {
        "127.0.0.1" | "localhost" => SocketAddr::from((Ipv4Addr::LOCALHOST, link.ep.port)),
        "::1" => SocketAddr::from((Ipv6Addr::LOCALHOST, link.ep.port)),
        _ => return Err("cleartext mcp url is only allowed for loopback".into()),
    };
    let host_header = if link.ep.host == "::1" {
        format!("[::1]:{}", link.ep.port)
    } else if link.ep.port == 80 {
        link.ep.host.clone()
    } else {
        format!("{}:{}", link.ep.host, link.ep.port)
    };
    let mut req_headers = vec![
        ("Host".into(), host_header),
        ("Content-Length".into(), body.len().to_string()),
        ("Connection".into(), "close".into()),
    ];
    req_headers.extend(headers.iter().cloned());
    let req = ClientRequest {
        method: "POST",
        path: &link.ep.path,
        minor: 1,
        headers: req_headers,
        body: body.as_bytes(),
    };
    let resp = http::send(addr, &req, timeout, MAX_HTTP_BYTES).map_err(|e| format!("http: {e}"))?;
    Ok(Reply {
        status: resp.status,
        headers: resp.headers,
        body: resp.body,
    })
}

fn curl_config(url: &str, body: &str, headers: &[(String, String)]) -> String {
    let mut cfg = format!("url = {}\nrequest = \"POST\"\n", curl_quote(url));
    for (k, v) in headers {
        cfg.push_str(&format!("header = {}\n", curl_quote(&format!("{k}: {v}"))));
    }
    cfg.push_str(&format!("data-binary = {}\n", curl_quote(body)));
    cfg
}

fn read_capped(mut r: impl Read, max: usize) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = r.read(&mut chunk).map_err(|e| format!("curl: {e}"))?;
        if n == 0 {
            break;
        }
        if buf.len() + n > max {
            return Err("http: response too large".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

fn split_http(raw: &[u8]) -> Result<Reply, String> {
    let mut rest = raw;
    while !rest.is_empty() {
        let Some((status, _reason, headers, head_len)) = http::parse_response_head(rest) else {
            break;
        };
        let after = &rest[head_len..];
        let (body, next) = take_body(&headers, after);
        if status >= 200 {
            return Ok(Reply {
                status,
                headers,
                body,
            });
        }
        if next.len() == rest.len() {
            break;
        }
        rest = next;
    }
    Err("bad http response".into())
}

fn take_body<'a>(headers: &[(String, String)], after: &'a [u8]) -> (Vec<u8>, &'a [u8]) {
    if let Some(cl) = http::header(headers, "content-length").and_then(|s| s.parse::<usize>().ok())
    {
        let n = cl.min(after.len());
        return (after[..n].to_vec(), &after[n..]);
    }
    if http::header(headers, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        if let Some(b) = http::dechunk(after) {
            return (b, &[]);
        }
    }
    (after.to_vec(), &[])
}

fn session_from(headers: &[(String, String)]) -> Option<String> {
    let v = http::header(headers, "mcp-session-id")?;
    if (1..=256).contains(&v.len())
        && header_safe(v)
        && v.bytes().all(|c| (0x21..=0x7e).contains(&c))
    {
        Some(v.to_string())
    } else {
        None
    }
}

fn messages_from(reply: &Reply) -> Result<Vec<Value>, String> {
    if reply.status == 202 && reply.body.is_empty() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&reply.body);
    let ct = http::header(&reply.headers, "content-type").unwrap_or("");
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

fn push_sse_data(data: &mut String, out: &mut Vec<Value>) {
    if data.is_empty() {
        return;
    }
    let text = std::mem::take(data);
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        out.push(v);
    }
}

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
        assert!(cfg.contains(&format!("url = {}", curl_quote(GITHUB_MCP_URL))));
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
}
