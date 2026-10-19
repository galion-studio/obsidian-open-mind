// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! std HTTP for the Hermes memory subset.
//!
//! Bind is loopback only. Port 8420 is refused because that is the live
//! gateway. The server thread stops when [`Server`] is dropped.
//!
//! Selected-provider fixtures, all JSON:
//! `POST /recall`, `POST /session/end`, `POST /v2/offload/query-mmd`,
//! `POST /v2/offload/ingest`, and `POST /v2/offload/read-ref`.
//! Offload does not call a model and does not dial the live gateway.

use crate::index::{
    clamp_limit, field_limit, field_str, owned_result_ref, valid_offload_session, Error, Hit,
    Index, Kind, PutRequest, SearchRequest, ToolPairRow,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_BODY: usize = 1_048_576;
const VERSION: &str = env!("CARGO_PKG_VERSION");

static REQUESTS: AtomicU64 = AtomicU64::new(1);

fn request_id() -> String {
    let n = REQUESTS.fetch_add(1, Ordering::Relaxed);
    format!("om-{n}")
}

/// Accept only `127.0.0.1`. Port 0 is the ephemeral bind. Port 8420 is the
/// live memory gateway and is refused.
pub fn parse_bind(addr: &str) -> Result<SocketAddr, Error> {
    let parsed: SocketAddr = addr
        .parse()
        .map_err(|_| Error::Bind(format!("not a socket address: {addr}")))?;
    match parsed.ip() {
        IpAddr::V4(ip) if ip == Ipv4Addr::LOCALHOST => {}
        _ => {
            return Err(Error::Bind(
                "only 127.0.0.1 is allowed; refusing a public or wildcard bind".into(),
            ))
        }
    }
    if parsed.port() == 8420 {
        return Err(Error::Bind("refusing live memory-gateway port 8420".into()));
    }
    Ok(parsed)
}

/// Loopback listener. Drop closes the socket and joins the thread.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Server {
    /// Bind `127.0.0.1:0` and serve until drop.
    pub fn spawn(index: Arc<Mutex<Index>>) -> Result<Self, Error> {
        Self::bind("127.0.0.1:0", index)
    }

    pub fn bind(addr: &str, index: Arc<Mutex<Index>>) -> Result<Self, Error> {
        let want = parse_bind(addr)?;
        let listener = TcpListener::bind(want).map_err(|err| Error::Bind(err.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|err| Error::Bind(err.to_string()))?;
        let bound = listener
            .local_addr()
            .map_err(|err| Error::Bind(err.to_string()))?;
        if bound.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || bound.port() == 8420 {
            return Err(Error::Bind(
                "refusing a non-loopback or live-gateway socket".into(),
            ));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let started = Instant::now();
        let join = thread::spawn(move || serve_loop(listener, index, flag, started));
        Ok(Self {
            addr: bound,
            stop,
            join: Some(join),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn serve_loop(
    listener: TcpListener,
    index: Arc<Mutex<Index>>,
    stop: Arc<AtomicBool>,
    started: Instant,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(err) = stream.set_nonblocking(false) {
                    let _ = err;
                    continue;
                }
                let _ = handle_client(stream, &index, started);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_client(
    mut stream: TcpStream,
    index: &Mutex<Index>,
    started: Instant,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let raw = match read_request(&mut stream) {
        Ok(raw) => raw,
        Err(fail) => {
            let body = fail.body();
            return write_json(&mut stream, fail.status, &body);
        }
    };
    let (status, body) = {
        let mut guard = index.lock().unwrap_or_else(|err| err.into_inner());
        dispatch(&mut guard, started, &raw)
    };
    write_json(&mut stream, status, &body)
}

struct RawRequest {
    method: String,
    path: String,
    authorization: String,
    service_id: String,
    body: Vec<u8>,
}

struct HttpFail {
    status: u16,
    message: String,
    v2: bool,
}

impl HttpFail {
    fn plain(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            v2: false,
        }
    }

    fn body(&self) -> Value {
        if self.v2 {
            json!({
                "code": self.status,
                "message": self.message,
                "request_id": request_id(),
            })
        } else {
            json!({ "error": self.message })
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<RawRequest, HttpFail> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_at = loop {
        if buf.len() > 65_536 {
            return Err(HttpFail::plain(400, "headers too large"));
        }
        let n = stream
            .read(&mut tmp)
            .map_err(|_| HttpFail::plain(400, "read failed"))?;
        if n == 0 {
            return Err(HttpFail::plain(400, "empty request"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };
    let header = String::from_utf8_lossy(&buf[..header_at]).into_owned();
    let mut rest = buf[header_at + 4..].to_vec();
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path_raw = parts.next().unwrap_or("");
    let path = path_raw.split('?').next().unwrap_or("").to_string();
    let mut content_length = 0usize;
    let mut authorization = String::new();
    let mut service_id = String::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().unwrap_or(0);
        } else if name.eq_ignore_ascii_case("authorization") {
            authorization = value.to_string();
        } else if name.eq_ignore_ascii_case("x-tdai-service-id") {
            service_id = value.to_string();
        }
    }
    if content_length > MAX_BODY {
        return Err(HttpFail::plain(413, "payload too large"));
    }
    while rest.len() < content_length {
        let n = stream
            .read(&mut tmp)
            .map_err(|_| HttpFail::plain(400, "body read failed"))?;
        if n == 0 {
            break;
        }
        rest.extend_from_slice(&tmp[..n]);
        if rest.len() > MAX_BODY {
            return Err(HttpFail::plain(413, "payload too large"));
        }
    }
    if rest.len() < content_length {
        return Err(HttpFail::plain(400, "short body"));
    }
    rest.truncate(content_length);
    Ok(RawRequest {
        method,
        path,
        authorization,
        service_id,
        body: rest,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_json(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(body).unwrap_or_else(|_| br#"{"error":"encode"}"#.to_vec());
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()?;
    Ok(())
}

fn dispatch(index: &mut Index, started: Instant, raw: &RawRequest) -> (u16, Value) {
    let method = raw.method.as_str();
    let path = raw.path.as_str();
    if method == "GET" && path == "/health" {
        return (200, health(started));
    }
    if path.starts_with("/v2/") {
        return dispatch_v2(index, raw);
    }
    if method != "POST" {
        return (
            404,
            json!({ "error": format!("Not found: {method} {path}") }),
        );
    }
    let body = match parse_body(&raw.body) {
        Ok(value) => value,
        Err(msg) => return (400, json!({ "error": msg })),
    };
    match path {
        "/capture" => capture(index, &body),
        "/recall" => recall(index, &body),
        "/session/end" => session_end(index, &body),
        "/search/memories" => search_memories(index, &body),
        "/search/conversations" => search_conversations(index, &body),
        _ => (
            404,
            json!({ "error": format!("Not found: {method} {path}") }),
        ),
    }
}

fn dispatch_v2(index: &mut Index, raw: &RawRequest) -> (u16, Value) {
    let req_id = request_id();
    if raw.method != "POST" {
        return (
            404,
            json!({ "error": format!("Not found: {} {}", raw.method, raw.path) }),
        );
    }
    if !known_v2(&raw.path) {
        return (
            404,
            json!({ "error": format!("Not found: POST {}", raw.path) }),
        );
    }
    if bearer(&raw.authorization).is_none() {
        return v2_err(
            401,
            "Missing or invalid Authorization header. Expected: Bearer {api_key}",
            &req_id,
        );
    }
    if raw.service_id.trim().is_empty() {
        return v2_err(401, "Missing x-tdai-service-id header", &req_id);
    }
    let body = match parse_body(&raw.body) {
        Ok(value) => value,
        Err(msg) => return v2_err(400, &msg, &req_id),
    };
    match raw.path.as_str() {
        "/v2/conversation/add" => v2_add(index, &body, &req_id),
        "/v2/atomic/search" => v2_atomic_search(index, &body, &req_id),
        "/v2/conversation/search" => v2_conversation_search(index, &body, &req_id),
        "/v2/offload/ingest" => offload_ingest(index, &body, &req_id),
        "/v2/offload/query-mmd" => offload_query_mmd(index, &body, &req_id),
        "/v2/offload/read-ref" => offload_read_ref(index, &body, &req_id),
        _ => (
            404,
            json!({ "error": format!("Not found: POST {}", raw.path) }),
        ),
    }
}

fn known_v2(path: &str) -> bool {
    matches!(
        path,
        "/v2/conversation/add"
            | "/v2/atomic/search"
            | "/v2/conversation/search"
            | "/v2/offload/ingest"
            | "/v2/offload/query-mmd"
            | "/v2/offload/read-ref"
    )
}

fn bearer(header: &str) -> Option<&str> {
    let rest = header.strip_prefix("Bearer ")?;
    let token = rest.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn health(started: Instant) -> Value {
    json!({
        "status": "ok",
        "version": VERSION,
        "uptime": started.elapsed().as_secs(),
        "stores": {
            "vectorStore": true,
            "embeddingService": false
        },
        "services": {
            "timerScanner": null,
            "pipelineWorker": null,
            "stateBackend": "none"
        },
        "embeddingService": false,
        "implementation": "openmind-memory"
    })
}

fn capture(index: &mut Index, body: &Value) -> (u16, Value) {
    let Some(session) = field_str(body, "session_key") else {
        return (
            400,
            json!({ "error": "Missing required fields: user_content, assistant_content, session_key" }),
        );
    };
    let Some(user) = field_str(body, "user_content") else {
        return (
            400,
            json!({ "error": "Missing required fields: user_content, assistant_content, session_key" }),
        );
    };
    let Some(assistant) = field_str(body, "assistant_content") else {
        return (
            400,
            json!({ "error": "Missing required fields: user_content, assistant_content, session_key" }),
        );
    };
    match index.capture(session, user, assistant) {
        Ok(out) => (
            200,
            json!({
                "l0_recorded": out.l0_recorded,
                "scheduler_notified": false
            }),
        ),
        Err(err) => map_err(err),
    }
}

fn search_memories(index: &Index, body: &Value) -> (u16, Value) {
    let mut req = match search_req(body, Some(Kind::Memory)) {
        Ok(req) => req,
        Err(err) => return map_err(err),
    };
    req.mem_type = field_str(body, "type").map(str::to_string);
    req.scene = field_str(body, "scene").map(str::to_string);
    match index.search(&req) {
        Ok(hits) => (
            200,
            json!({
                "results": format_memories(&hits),
                "total": hits.len(),
                "strategy": "fts"
            }),
        ),
        Err(err) => map_err(err),
    }
}

fn search_conversations(index: &Index, body: &Value) -> (u16, Value) {
    let mut req = match search_req(body, Some(Kind::Conversation)) {
        Ok(req) => req,
        Err(err) => return map_err(err),
    };
    req.session_key = field_str(body, "session_key").map(str::to_string);
    match index.search(&req) {
        Ok(hits) => (
            200,
            json!({
                "results": format_conversations(&hits),
                "total": hits.len()
            }),
        ),
        Err(err) => map_err(err),
    }
}

fn recall(index: &Index, body: &Value) -> (u16, Value) {
    let Some(query) = field_str(body, "query") else {
        return (
            400,
            json!({ "error": "Missing required fields: query, session_key" }),
        );
    };
    let Some(session) = field_str(body, "session_key") else {
        return (
            400,
            json!({ "error": "Missing required fields: query, session_key" }),
        );
    };
    // `user_id` is optional and is not copied into the response.
    match index.recall(query, session) {
        Ok(out) => (
            200,
            json!({
                "context": out.context,
                "strategy": out.strategy,
                "memory_count": out.memory_count,
                "code": 0,
                "message": "ok",
                "retryable": false
            }),
        ),
        Err(err) => map_err(err),
    }
}

fn session_end(index: &mut Index, body: &Value) -> (u16, Value) {
    let Some(session) = field_str(body, "session_key") else {
        return (
            400,
            json!({ "error": "Missing required field: session_key" }),
        );
    };
    match index.end_session(session) {
        Ok(()) => (200, json!({ "flushed": true })),
        Err(err) => map_err(err),
    }
}

fn offload_ingest(index: &mut Index, body: &Value, req_id: &str) -> (u16, Value) {
    let Some(session) = field_str(body, "session_id") else {
        return v2_err(400, "missing or invalid session_id", req_id);
    };
    if !valid_offload_session(session) {
        return v2_err(400, "missing or invalid session_id", req_id);
    }
    if body.get("prompt").is_some() && field_str(body, "prompt").is_none() {
        return v2_err(400, "prompt must not be empty or whitespace-only", req_id);
    }
    let prompt = field_str(body, "prompt");
    let pairs = match body.get("tool_pairs") {
        None => Vec::new(),
        Some(Value::Array(items)) => match parse_tool_pairs(items) {
            Ok(rows) => rows,
            Err(msg) => return v2_err(400, &msg, req_id),
        },
        Some(_) => return v2_err(400, "tool_pairs must be an array", req_id),
    };
    if pairs.is_empty() && prompt.is_none() {
        return v2_err(
            400,
            "Either tool_pairs must be non-empty or prompt must be provided",
            req_id,
        );
    }
    // Prompt-only ingest does not invent a canvas. Internal sessions are skipped.
    if pairs.is_empty() || skip_offload_session(session) {
        return v2_ok(req_id, json!({}));
    }
    match index.ingest_offload(session, &pairs) {
        Ok(node_ids) => v2_ok(req_id, json!({ "node_ids": node_ids })),
        Err(err) => v2_err(status_of(&err), &err.to_string(), req_id),
    }
}

fn offload_query_mmd(index: &Index, body: &Value, req_id: &str) -> (u16, Value) {
    let Some(session) = field_str(body, "session_id") else {
        return v2_err(400, "missing or invalid session_id in body", req_id);
    };
    if !valid_offload_session(session) {
        return v2_err(400, "missing or invalid session_id in body", req_id);
    }
    if let Some(limit) = body.get("limit") {
        match limit.as_u64() {
            Some(n) if n >= 1 => {}
            _ => return v2_err(400, "limit must be an integer >= 1", req_id),
        }
    }
    match index.offload_canvas(session) {
        Ok(Some(canvas)) => v2_ok(
            req_id,
            json!({
                "mmds": [{
                    "filename": "canvas.mmd",
                    "content": canvas.content,
                    "version": canvas.recorded_at
                }],
                "currentMmd": "canvas.mmd"
            }),
        ),
        Ok(None) => v2_ok(req_id, json!({ "mmds": [], "currentMmd": null })),
        Err(err) => v2_err(status_of(&err), &err.to_string(), req_id),
    }
}

fn offload_read_ref(index: &Index, body: &Value, req_id: &str) -> (u16, Value) {
    let Some(session) = field_str(body, "session_id") else {
        return v2_err(400, "missing or invalid session_id", req_id);
    };
    if !valid_offload_session(session) {
        return v2_err(400, "missing or invalid session_id", req_id);
    }
    let Some(result_ref) = field_str(body, "result_ref") else {
        return v2_err(400, "Missing required field: result_ref", req_id);
    };
    let Some(owned) = owned_result_ref(session, result_ref) else {
        return v2_err(404, "result_ref not found", req_id);
    };
    let full = match body.get("full") {
        None => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return v2_err(400, "full must be a boolean", req_id),
    };
    match index.read_offload_ref(&owned) {
        Ok(Some(raw)) => {
            let (content, truncated) = if full { (raw, false) } else { clip_ref(&raw) };
            v2_ok(
                req_id,
                json!({
                    "result_ref": owned,
                    "content": content,
                    "truncated": truncated
                }),
            )
        }
        Ok(None) => v2_err(404, "result_ref not found", req_id),
        Err(err) => v2_err(status_of(&err), &err.to_string(), req_id),
    }
}

fn v2_ok(req_id: &str, data: Value) -> (u16, Value) {
    (
        200,
        json!({
            "code": 0,
            "message": "ok",
            "request_id": req_id,
            "data": data
        }),
    )
}

fn parse_tool_pairs(items: &[Value]) -> Result<Vec<ToolPairRow>, String> {
    if items.len() > 100 {
        return Err("tool_pairs must hold at most 100 items".into());
    }
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        if !item.is_object() {
            return Err("tool pair must be an object".into());
        }
        let Some(tool_name) = field_str(item, "tool_name") else {
            return Err("tool pair tool_name is empty".into());
        };
        if tool_name.len() > 256 {
            return Err("tool pair tool_name is too long".into());
        }
        let Some(tool_call_id) = item.get("tool_call_id").and_then(Value::as_str) else {
            return Err("tool pair tool_call_id must be a string".into());
        };
        if tool_call_id.len() > 256 {
            return Err("tool pair tool_call_id is too long".into());
        }
        // Caller stamps are not stored. They may contain a `Z` suffix.
        if field_str(item, "timestamp").is_none() {
            return Err("tool pair timestamp is missing".into());
        }
        let params_json = match item.get("params") {
            None => String::new(),
            Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
        };
        let result_text = match item.get("result") {
            None => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(value) => serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()),
        };
        let error = match item.get("error") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => return Err("tool pair error must be a string".into()),
        };
        rows.push(ToolPairRow {
            tool_name: tool_name.to_string(),
            tool_call_id: tool_call_id.to_string(),
            params_json,
            result_text,
            error,
        });
    }
    Ok(rows)
}

fn skip_offload_session(session_id: &str) -> bool {
    session_id.contains("subagent") || internal_memory_session(session_id)
}

fn internal_memory_session(session_id: &str) -> bool {
    let Some(start) = session_id.find("memory-") else {
        return false;
    };
    let after = &session_id[start + "memory-".len()..];
    let Some(pos) = after.find("-session-") else {
        return false;
    };
    after[pos + "-session-".len()..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
}

fn clip_ref(text: &str) -> (String, bool) {
    const MAX: usize = 6400;
    if text.len() <= MAX {
        return (text.to_string(), false);
    }
    let mut end = MAX;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

fn v2_add(index: &mut Index, body: &Value, req_id: &str) -> (u16, Value) {
    let session = field_str(body, "session_id").unwrap_or("").to_string();
    if session.is_empty() {
        return v2_err(400, "Missing required field: session_id", req_id);
    }
    let Some(messages) = body.get("messages").and_then(Value::as_array) else {
        return v2_err(400, "Missing required field: messages", req_id);
    };
    if messages.is_empty() || messages.len() > 100 {
        return v2_err(400, "messages must hold 1 to 100 items", req_id);
    }
    let mut rows = Vec::new();
    for message in messages {
        let Some(role) = field_str(message, "role") else {
            return v2_err(400, "message role is empty", req_id);
        };
        let Some(content) = field_str(message, "content") else {
            return v2_err(400, "message content is empty", req_id);
        };
        rows.push(PutRequest::conversation(&session, role, content));
        let mut note = PutRequest::memory(content);
        note.session_key = session.clone();
        rows.push(note);
    }
    // Incoming message timestamps are ignored. Callers may send a `Z` stamp.
    // Every stored clock value is Europe/Zagreb.
    match index.put_all(&rows) {
        Ok(stored) => {
            let accepted: Vec<&str> = stored
                .iter()
                .step_by(2)
                .map(|row| row.id.as_str())
                .collect();
            (
                200,
                json!({
                    "code": 0,
                    "message": "ok",
                    "request_id": req_id,
                    "data": {
                        "accepted_ids": accepted,
                        "total_count": accepted.len()
                    }
                }),
            )
        }
        Err(err) => v2_err(status_of(&err), &err.to_string(), req_id),
    }
}

fn v2_atomic_search(index: &Index, body: &Value, req_id: &str) -> (u16, Value) {
    let mut req = match search_req(body, Some(Kind::Memory)) {
        Ok(req) => req,
        Err(err) => return v2_err(status_of(&err), &err.to_string(), req_id),
    };
    req.mem_type = field_str(body, "type").map(str::to_string);
    match index.search(&req) {
        Ok(hits) => {
            let items: Vec<Value> = hits
                .iter()
                .map(|hit| {
                    json!({
                        "id": hit.id,
                        "type": hit.mem_type,
                        "content": hit.content,
                        "created_at": hit.recorded_at,
                        "updated_at": hit.recorded_at,
                        "score": hit.score
                    })
                })
                .collect();
            (
                200,
                json!({
                    "code": 0,
                    "message": "ok",
                    "request_id": req_id,
                    "data": { "items": items }
                }),
            )
        }
        Err(err) => v2_err(status_of(&err), &err.to_string(), req_id),
    }
}

fn v2_conversation_search(index: &Index, body: &Value, req_id: &str) -> (u16, Value) {
    let mut req = match search_req(body, Some(Kind::Conversation)) {
        Ok(req) => req,
        Err(err) => return v2_err(status_of(&err), &err.to_string(), req_id),
    };
    req.session_key = field_str(body, "session_id").map(str::to_string);
    match index.search(&req) {
        Ok(hits) => {
            let messages: Vec<Value> = hits
                .iter()
                .map(|hit| {
                    json!({
                        "id": hit.id,
                        "role": hit.role,
                        "content": hit.content,
                        "timestamp": hit.recorded_at,
                        "score": hit.score
                    })
                })
                .collect();
            (
                200,
                json!({
                    "code": 0,
                    "message": "ok",
                    "request_id": req_id,
                    "data": { "messages": messages }
                }),
            )
        }
        Err(err) => v2_err(status_of(&err), &err.to_string(), req_id),
    }
}

fn search_req(body: &Value, kind: Option<Kind>) -> Result<SearchRequest, Error> {
    let Some(query) = field_str(body, "query") else {
        return Err(Error::BadRequest("Missing required field: query".into()));
    };
    Ok(SearchRequest {
        query: query.to_string(),
        limit: clamp_limit(field_limit(body)),
        kind,
        session_key: None,
        mem_type: None,
        scene: None,
    })
}

fn parse_body(bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() {
        return Err("invalid json".into());
    }
    serde_json::from_slice(bytes).map_err(|_| "invalid json".to_string())
}

fn map_err(err: Error) -> (u16, Value) {
    (status_of(&err), json!({ "error": err.to_string() }))
}

fn status_of(err: &Error) -> u16 {
    match err {
        Error::BadRequest(_) => 400,
        _ => 500,
    }
}

fn v2_err(status: u16, message: &str, req_id: &str) -> (u16, Value) {
    (
        status,
        json!({
            "code": status,
            "message": message,
            "request_id": req_id
        }),
    )
}

fn format_memories(hits: &[Hit]) -> String {
    if hits.is_empty() {
        return "No matching memories found.".to_string();
    }
    let mut lines = vec![
        format!("Found {} matching memories:", hits.len()),
        String::new(),
    ];
    for hit in hits {
        let scene = if hit.scene.is_empty() {
            String::new()
        } else {
            format!(" [scene: {}]", hit.scene)
        };
        lines.push(format!(
            "- **[{}]** (priority: 0){scene} (score: {:.3})",
            hit.mem_type, hit.score
        ));
        lines.push(format!("  {}", hit.content));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn format_conversations(hits: &[Hit]) -> String {
    if hits.is_empty() {
        return "No matching conversation messages found.".to_string();
    }
    let mut lines = vec![
        format!("Found {} matching message(s):", hits.len()),
        String::new(),
    ];
    for hit in hits {
        lines.push("---".to_string());
        lines.push(format!(
            "**[{}]** Session: {} [{}] (score: {:.3})",
            hit.role, hit.session_key, hit.recorded_at, hit.score
        ));
        lines.push(String::new());
        lines.push(hit.content.clone());
        lines.push(String::new());
    }
    lines.join("\n")
}
