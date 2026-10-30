// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! MCP transports for [`crate::mcp`]: stdio child process, Streamable HTTP
//! (one POST per JSON-RPC message; reply is JSON or an SSE stream; the
//! `Mcp-Session-Id` header is kept) and legacy HTTP+SSE (a long GET event
//! stream that names a POST `endpoint`).
//!
//! HTTP goes through the system `curl` like the model provider: `-q -K -`,
//! with URL, headers (tokens) and body written to curl's stdin as a config,
//! so nothing secret is on argv or in /proc/<pid>/cmdline. curl and stdio
//! children drop to `run_as` when the gateway is root.

use crate::config::McpServerCfg;
use crate::mcp::{expand, Secrets};
use crate::provider::curl_quote;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Largest HTTP reply we read (bytes).
pub const MAX_HTTP_BYTES: usize = 16 * 1024 * 1024;
/// Largest single stdio / SSE message (bytes).
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

pub fn is_reconnect_error(e: &str) -> bool {
    e.contains("server exited")
        || e.starts_with("write")
        || e.contains("session expired")
        || e.contains("stream closed")
}

fn drop_privileges(cmd: &mut Command, run_as: Option<(u32, u32)>) {
    #[cfg(unix)]
    if let Some((u, g)) = run_as {
        use std::os::unix::process::CommandExt;
        // std calls setgroups(0) before setgid/setuid when uid is set.
        cmd.gid(g).uid(u);
    }
    #[cfg(not(unix))]
    let _ = (cmd, run_as);
}

fn base_env(cmd: &mut Command) {
    cmd.env_clear();
    cmd.env(
        "PATH",
        std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()),
    );
    for k in ["LANG", "TZ"] {
        if let Ok(v) = std::env::var(k) {
            cmd.env(k, v);
        }
    }
    cmd.env("HOME", "/tmp");
}

// --------------------------------------------------------------- connection

pub enum Conn {
    Pipe(Pipe),
    Http(Http),
}

impl Conn {
    pub fn rpc(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        match self {
            Conn::Pipe(p) => p.rpc(method, params, timeout),
            Conn::Http(h) => h.rpc(method, params, timeout),
        }
    }
    pub fn notify(&mut self, method: &str) -> Result<(), String> {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        match self {
            Conn::Pipe(p) => p.send(&msg),
            Conn::Http(h) => h.exchange(&msg, None, h.connect_timeout).map(|_| ()),
        }
    }
    pub fn set_protocol(&mut self, v: &str) {
        if let Conn::Http(h) = self {
            h.protocol = Some(
                v.chars()
                    .filter(|c| c.is_ascii_graphic())
                    .take(32)
                    .collect(),
            );
        }
    }
}

fn rpc_error(method: &str, e: &Value) -> String {
    let msg = e["message"].as_str().unwrap_or("error");
    format!("{method}: {}", crate::mcp::clean_text(msg, 500))
}

// ------------------------------------------------------- stdio / legacy SSE

enum Sink {
    Stdin(ChildStdin),
    Post(Box<Http>),
}

/// A connection whose replies arrive on a channel (stdio stdout, or the
/// legacy SSE GET stream).
pub struct Pipe {
    child: Child,
    sink: Sink,
    rx: Receiver<Value>,
    next_id: u64,
    stderr: Arc<Mutex<VecDeque<String>>>,
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Pipe {
    fn send(&mut self, msg: &Value) -> Result<(), String> {
        match &mut self.sink {
            Sink::Stdin(w) => writeln!(w, "{msg}")
                .and_then(|_| w.flush())
                .map_err(|e| format!("write: {e}")),
            Sink::Post(h) => {
                let t = h.connect_timeout.max(Duration::from_secs(5));
                h.exchange(msg, None, t).map(|_| ())
            }
        }
    }

    fn tail(&self) -> String {
        let g = match self.stderr.lock() {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
        if g.is_empty() {
            String::new()
        } else {
            format!(
                " (stderr: {})",
                g.iter().cloned().collect::<Vec<_>>().join(" | ")
            )
        }
    }

    fn rpc(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(v) if v.get("method").is_some() && v.get("id").is_some() => {
                    // server -> client request: answer ping, refuse the rest
                    let rid = v["id"].clone();
                    let reply = if v["method"] == "ping" {
                        json!({"jsonrpc": "2.0", "id": rid, "result": {}})
                    } else {
                        json!({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601,
                               "message": "not supported by hermes-rs"}})
                    };
                    let _ = self.send(&reply);
                }
                Ok(v) if v["id"].as_u64() == Some(id) => {
                    if let Some(e) = v.get("error") {
                        return Err(rpc_error(method, e));
                    }
                    return Ok(v["result"].clone());
                }
                Ok(_) => continue, // notifications / stale replies
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("{method}: timed out after {}s", timeout.as_secs()))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    // give the stderr reader a moment
                    std::thread::sleep(Duration::from_millis(50));
                    return Err(format!("{method}: server exited{}", self.tail()));
                }
            }
        }
    }
}

/// Read lines up to `MAX_LINE_BYTES`; longer lines are dropped.
fn read_capped_line(r: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    buf.clear();
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', buf)?;
    if buf.len() > MAX_LINE_BYTES {
        // swallow the rest of the oversized line
        let mut sink = Vec::new();
        r.read_until(b'\n', &mut sink)?;
        buf.clear();
        return Ok(n.max(1));
    }
    Ok(n)
}

fn stderr_ring(err: impl Read + Send + 'static, secrets: Secrets) -> Arc<Mutex<VecDeque<String>>> {
    let ring = Arc::new(Mutex::new(VecDeque::new()));
    let r2 = ring.clone();
    std::thread::spawn(move || {
        let mut rd = BufReader::new(err);
        let mut buf = Vec::new();
        while let Ok(n) = read_capped_line(&mut rd, &mut buf) {
            if n == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&buf);
            let line = crate::mcp::clean_text(&crate::redact(&secrets.scrub(line.trim())), 200);
            if line.is_empty() {
                continue;
            }
            if let Ok(mut g) = r2.lock() {
                if g.len() >= 4 {
                    g.pop_front();
                }
                g.push_back(line);
            }
        }
    });
    ring
}

pub fn spawn_stdio(
    cfg: &McpServerCfg,
    sec: &Secrets,
    run_as: Option<(u32, u32)>,
) -> Result<Conn, String> {
    let mut cmd = Command::new(&cfg.command);
    cmd.args(cfg.effective_args());
    base_env(&mut cmd);
    for (k, v) in &cfg.env {
        cmd.env(k, expand(v, sec).map_err(|e| format!("env {k}: {e}"))?);
    }
    for (k, v) in &sec.vars {
        if cfg.pass_env.as_ref().is_none_or(|p| p.contains(k)) {
            cmd.env(k, v);
        }
    }
    cmd.current_dir(cfg.cwd.as_deref().unwrap_or("/"));
    drop_privileges(&mut cmd, run_as);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", cfg.command))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let ring = stderr_ring(stderr, sec.clone());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut rd = BufReader::new(stdout);
        let mut buf = Vec::new();
        while let Ok(n) = read_capped_line(&mut rd, &mut buf) {
            if n == 0 {
                break;
            }
            if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                if send_msgs(&tx, v).is_err() {
                    break;
                }
            }
        }
    });
    Ok(Conn::Pipe(Pipe {
        child,
        sink: Sink::Stdin(stdin),
        rx,
        next_id: 1,
        stderr: ring,
    }))
}

/// A JSON-RPC message or batch onto the channel.
fn send_msgs(tx: &Sender<Value>, v: Value) -> Result<(), ()> {
    match v {
        Value::Array(a) => {
            for m in a {
                tx.send(m).map_err(|_| ())?;
            }
            Ok(())
        }
        m => tx.send(m).map_err(|_| ()),
    }
}

// --------------------------------------------------------------------- HTTP

pub struct Http {
    url: String,
    headers: Vec<(String, String)>,
    session: Option<String>,
    protocol: Option<String>,
    next_id: u64,
    run_as: Option<(u32, u32)>,
    connect_timeout: Duration,
    secrets: Secrets,
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    messages: Vec<Value>,
    body_preview: String,
}

fn resolved_headers(cfg: &McpServerCfg, sec: &Secrets) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (k, v) in &cfg.headers {
        if k.contains(['\r', '\n', ':']) {
            return Err(format!("bad header name {k:?}"));
        }
        let v = expand(v, sec).map_err(|e| format!("header {k}: {e}"))?;
        if v.contains(['\r', '\n']) {
            return Err(format!("header {k}: value has a newline"));
        }
        out.push((k.clone(), v));
    }
    Ok(out)
}

fn warn_plain_http(url: &str) -> Result<(), String> {
    let rest = url.strip_prefix("http://");
    if let Some(r) = rest {
        let host = r.split(['/', '?']).next().unwrap_or("");
        let host = host.rsplit_once('@').map(|x| x.1).unwrap_or(host);
        let h = if host.starts_with('[') {
            host.split(']').next().unwrap_or("").trim_start_matches('[')
        } else {
            host.split(':').next().unwrap_or("")
        };
        if !matches!(h, "127.0.0.1" | "localhost" | "::1") {
            return Err(format!(
                "plain http:// is only allowed to loopback (got host {h}); use https://"
            ));
        }
    }
    Ok(())
}

pub fn open_http(
    cfg: &McpServerCfg,
    sec: &Secrets,
    run_as: Option<(u32, u32)>,
) -> Result<Conn, String> {
    let url = cfg.url.clone().ok_or("no url")?;
    warn_plain_http(&url)?;
    Ok(Conn::Http(Http {
        url,
        headers: resolved_headers(cfg, sec)?,
        session: None,
        protocol: None,
        next_id: 1,
        run_as,
        connect_timeout: Duration::from_secs(cfg.connect_timeout_s.max(1)),
        secrets: sec.clone(),
    }))
}

impl Http {
    fn curl_config(
        &self,
        method: &str,
        body: Option<&str>,
        max_time: Option<Duration>,
        accept: &str,
    ) -> String {
        let mut c = String::new();
        let mut line = |k: &str, v: &str| {
            c.push_str(k);
            c.push_str(" = ");
            c.push_str(&curl_quote(v));
            c.push('\n');
        };
        line("url", &self.url);
        line("request", method);
        line("header", &format!("Accept: {accept}"));
        if body.is_some() {
            line("header", "Content-Type: application/json");
        }
        if let Some(s) = &self.session {
            line("header", &format!("Mcp-Session-Id: {s}"));
        }
        if let Some(p) = &self.protocol {
            line("header", &format!("MCP-Protocol-Version: {p}"));
        }
        for (k, v) in &self.headers {
            line("header", &format!("{k}: {v}"));
        }
        if let Some(b) = body {
            line("data-binary", b);
        }
        line("proto", "=http,https");
        line(
            "connect-timeout",
            &self.connect_timeout.as_secs().max(1).to_string(),
        );
        if let Some(t) = max_time {
            line("max-time", &t.as_secs().max(1).to_string());
        }
        c.push_str("silent\nshow-error\ninclude\nno-buffer\n");
        c
    }

    fn spawn_curl(&self, config: &str) -> Result<Child, String> {
        let mut cmd = Command::new("curl");
        cmd.args(["-q", "-K", "-"]);
        base_env(&mut cmd);
        cmd.current_dir("/");
        drop_privileges(&mut cmd, self.run_as);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("spawn curl: {e}"))?;
        if let Some(mut w) = child.stdin.take() {
            w.write_all(config.as_bytes())
                .map_err(|e| format!("curl config: {e}"))?;
        }
        Ok(child)
    }

    /// POST one message. With `want` set, stop reading as soon as the reply
    /// with that id arrives (SSE replies may stay open).
    fn exchange(
        &mut self,
        msg: &Value,
        want: Option<u64>,
        timeout: Duration,
    ) -> Result<Vec<Value>, String> {
        let r = self.post(msg, want, timeout)?;
        if let Some(sid) = header(&r.headers, "mcp-session-id") {
            let sid: String = sid
                .chars()
                .filter(|c| c.is_ascii_graphic())
                .take(256)
                .collect();
            self.session = Some(sid);
        }
        match r.status {
            200..=299 => Ok(r.messages),
            404 if self.session.is_some() => {
                self.session = None;
                Err("session expired (HTTP 404)".into())
            }
            401 | 403 => Err(format!(
                "unauthorized (HTTP {}): check the token in env_file / the headers{}",
                r.status,
                header(&r.headers, "www-authenticate")
                    .map(|w| format!("; server says: {}", crate::mcp::clean_text(&w, 200)))
                    .unwrap_or_default()
            )),
            s => Err(format!(
                "HTTP {s}: {}",
                crate::redact(&self.secrets.scrub(&r.body_preview))
            )),
        }
    }

    fn post(&self, msg: &Value, want: Option<u64>, timeout: Duration) -> Result<Reply, String> {
        let body = msg.to_string();
        let cfg = self.curl_config(
            "POST",
            Some(&body),
            Some(timeout),
            "application/json, text/event-stream",
        );
        let mut child = self.spawn_curl(&cfg)?;
        let out = child.stdout.take().ok_or("no stdout")?;
        let mut err = child.stderr.take();
        let mut rd = BufReader::new(out);
        let res = read_reply(&mut rd, want);
        let _ = child.kill();
        let _ = child.wait();
        match res {
            Ok(r) => Ok(r),
            Err(e) => {
                let mut es = String::new();
                if let Some(e2) = err.as_mut() {
                    let _ = e2.take(2000).read_to_string(&mut es);
                }
                Err(format!(
                    "http: {e}{}",
                    if es.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", crate::mcp::clean_text(es.trim(), 300))
                    }
                ))
            }
        }
    }

    fn rpc(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msgs = self.exchange(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            Some(id),
            timeout,
        )?;
        for v in msgs {
            if v["id"].as_u64() == Some(id) && v.get("method").is_none() {
                if let Some(e) = v.get("error") {
                    return Err(rpc_error(method, e));
                }
                return Ok(v["result"].clone());
            }
        }
        Err(format!("{method}: no reply in the HTTP response"))
    }
}

fn header(h: &[(String, String)], name: &str) -> Option<String> {
    h.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

/// Status + headers of the final response (skips 1xx and proxy CONNECT blocks).
fn read_head(rd: &mut impl BufRead) -> Result<(u16, Vec<(String, String)>), String> {
    let mut buf = Vec::new();
    loop {
        let n = read_capped_line(rd, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("no HTTP response".into());
        }
        let status_line = String::from_utf8_lossy(&buf).trim().to_string();
        if status_line.is_empty() {
            continue;
        }
        if !status_line.starts_with("HTTP/") {
            return Err("bad HTTP status line".into());
        }
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or("bad HTTP status")?;
        let mut headers = Vec::new();
        loop {
            let n = read_capped_line(rd, &mut buf).map_err(|e| e.to_string())?;
            let l = String::from_utf8_lossy(&buf).trim_end().to_string();
            if n == 0 || l.is_empty() {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
        }
        let connect = status_line
            .to_ascii_lowercase()
            .contains("connection established");
        if (100..200).contains(&status) || connect {
            continue;
        }
        return Ok((status, headers));
    }
}

/// One SSE event at a time: (event name, data).
fn next_event(rd: &mut impl BufRead) -> Result<Option<(String, String)>, String> {
    let mut buf = Vec::new();
    let mut event = String::new();
    let mut data = String::new();
    let mut any = false;
    loop {
        let n = read_capped_line(rd, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(any.then_some((event, data)));
        }
        let l = String::from_utf8_lossy(&buf);
        let l = l.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            if any {
                return Ok(Some((event, data)));
            }
            continue;
        }
        if l.starts_with(':') {
            continue;
        }
        let (k, v) = l.split_once(':').unwrap_or((l, ""));
        let v = v.strip_prefix(' ').unwrap_or(v);
        match k {
            "event" => {
                event = v.to_string();
                any = true;
            }
            "data" => {
                if !data.is_empty() {
                    data.push('\n');
                }
                if data.len() + v.len() > MAX_LINE_BYTES {
                    return Err("SSE event too large".into());
                }
                data.push_str(v);
                any = true;
            }
            _ => {}
        }
    }
}

fn read_reply(rd: &mut impl BufRead, want: Option<u64>) -> Result<Reply, String> {
    let (status, headers) = read_head(rd)?;
    let ctype = header(&headers, "content-type")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut messages = Vec::new();
    let mut body_preview = String::new();
    if ctype.starts_with("text/event-stream") && (200..300).contains(&status) {
        while let Some((ev, data)) = next_event(rd)? {
            if !(ev.is_empty() || ev == "message") || data.trim().is_empty() {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            let batch = match v {
                Value::Array(a) => a,
                m => vec![m],
            };
            let done = batch
                .iter()
                .any(|m| want.is_some() && m["id"].as_u64() == want && m.get("method").is_none());
            messages.extend(batch);
            if done {
                break;
            }
        }
    } else {
        let mut body = Vec::new();
        rd.take(MAX_HTTP_BYTES as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| e.to_string())?;
        if body.len() > MAX_HTTP_BYTES {
            return Err("HTTP reply too large".into());
        }
        let text = String::from_utf8_lossy(&body);
        if ctype.contains("json") && !text.trim().is_empty() {
            match serde_json::from_str::<Value>(&text) {
                Ok(Value::Array(a)) => messages.extend(a),
                Ok(m) => messages.push(m),
                Err(e) => return Err(format!("bad JSON reply: {e}")),
            }
        }
        body_preview = crate::mcp::clean_text(text.trim(), 300);
    }
    Ok(Reply {
        status,
        headers,
        messages,
        body_preview,
    })
}

// --------------------------------------------------------------- legacy SSE

fn join_url(base: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = base[scheme_end..]
        .find('/')
        .map(|i| i + scheme_end)
        .unwrap_or(base.len());
    if rel.starts_with('/') {
        format!("{}{rel}", &base[..host_end])
    } else {
        let dir_end = base
            .rfind('/')
            .filter(|i| *i >= host_end)
            .unwrap_or(host_end);
        format!("{}/{rel}", &base[..dir_end])
    }
}

fn same_origin(a: &str, b: &str) -> bool {
    let origin = |u: &str| {
        let s = u.find("://").map(|i| i + 3).unwrap_or(0);
        let e = u[s..].find('/').map(|i| i + s).unwrap_or(u.len());
        u[..e].to_ascii_lowercase()
    };
    origin(a) == origin(b)
}

pub fn open_sse(
    cfg: &McpServerCfg,
    sec: &Secrets,
    run_as: Option<(u32, u32)>,
    connect: Duration,
) -> Result<Conn, String> {
    let url = cfg.url.clone().ok_or("no url")?;
    warn_plain_http(&url)?;
    let http = Http {
        url: url.clone(),
        headers: resolved_headers(cfg, sec)?,
        session: None,
        protocol: None,
        next_id: 1,
        run_as,
        connect_timeout: Duration::from_secs(cfg.connect_timeout_s.max(1)),
        secrets: sec.clone(),
    };
    let conf = http.curl_config("GET", None, None, "text/event-stream");
    let mut child = http.spawn_curl(&conf)?;
    let out = child.stdout.take().ok_or("no stdout")?;
    let err = child.stderr.take().ok_or("no stderr")?;
    let ring = stderr_ring(err, sec.clone());
    let (tx, rx) = channel::<Value>();
    let (etx, erx) = channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let mut rd = BufReader::new(out);
        match read_head(&mut rd) {
            Ok((200, _)) => {}
            Ok((s, _)) => {
                let _ = etx.send(Err(format!("SSE GET: HTTP {s}")));
                return;
            }
            Err(e) => {
                let _ = etx.send(Err(format!("SSE GET: {e}")));
                return;
            }
        }
        while let Ok(Some((ev, data))) = next_event(&mut rd) {
            if ev == "endpoint" {
                let _ = etx.send(Ok(data.trim().to_string()));
            } else if ev.is_empty() || ev == "message" {
                if let Ok(v) = serde_json::from_str::<Value>(&data) {
                    if send_msgs(&tx, v).is_err() {
                        break;
                    }
                }
            }
        }
        // dropping tx = "server exited" for the waiting rpc
    });
    let endpoint = match erx.recv_timeout(connect) {
        Ok(Ok(e)) => join_url(&url, &e),
        Ok(Err(e)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("SSE: no endpoint event (stream closed or timed out)".into());
        }
    };
    if !same_origin(&endpoint, &url) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("SSE: endpoint is on another origin; refused".into());
    }
    let mut post = http;
    post.url = endpoint;
    Ok(Conn::Pipe(Pipe {
        child,
        sink: Sink::Post(Box::new(post)),
        rx,
        next_id: 1,
        stderr: ring,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_origin() {
        assert_eq!(
            join_url("http://127.0.0.1:9/sse", "/messages?s=1"),
            "http://127.0.0.1:9/messages?s=1"
        );
        assert_eq!(join_url("http://h/a/sse", "m"), "http://h/a/m");
        assert!(same_origin("http://h:1/x", "http://h:1/y"));
        assert!(!same_origin("http://h:1/x", "http://evil/y"));
        assert!(warn_plain_http("http://127.0.0.1:8/mcp").is_ok());
        assert!(warn_plain_http("http://example.com/mcp").is_err());
        assert!(warn_plain_http("https://example.com/mcp").is_ok());
    }

    #[test]
    fn parses_json_and_sse_replies() {
        let raw = "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: abc\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}";
        let r = read_reply(&mut BufReader::new(raw.as_bytes()), Some(1)).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(header(&r.headers, "mcp-session-id").as_deref(), Some("abc"));
        assert_eq!(r.messages.len(), 1);
        let sse = "HTTP/2 200\r\ncontent-type: text/event-stream\r\n\r\n: hi\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"n\"}\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\n data: \"result\":{\"ok\":1}}\n\n";
        // the second event spans two data lines
        let sse = sse.replace("\n data:", "\ndata:");
        let r = read_reply(&mut BufReader::new(sse.as_bytes()), Some(7)).unwrap();
        assert_eq!(r.messages.len(), 2);
        assert_eq!(r.messages[1]["result"]["ok"], 1);
    }
}
