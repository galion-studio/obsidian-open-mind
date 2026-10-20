// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Minimal blocking HTTP/1.0 + 1.1 server: one request per connection,
//! `Connection: close`, close-delimited SSE bodies (works for HTTP/1.0
//! clients like Galion One and for node fetch). Thread per connection,
//! bounded by `max_conns`.

use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let n = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Result<Value, String> {
        if self.body.is_empty() {
            return Ok(Value::Object(Default::default()));
        }
        serde_json::from_slice(&self.body).map_err(|_| "Invalid JSON".to_string())
    }

    pub fn query_param(&self, key: &str) -> Option<String> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (k == key).then(|| v.to_string())
        })
    }
}

pub fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let mut total = 0usize;
    reader.read_line(&mut line)?;
    total += line.len();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    if method.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty request"));
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        let n = reader.read_line(&mut h)?;
        total += n;
        if total > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "headers too large",
            ));
        }
        let h = h.trim_end_matches(['\r', '\n']);
        if n == 0 || h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let mut req = Request {
        method,
        path,
        query,
        headers,
        body: Vec::new(),
    };
    let chunked = req
        .header("transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        loop {
            let mut size_line = String::new();
            reader.read_line(&mut size_line)?;
            let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk"))?;
            if size == 0 {
                let mut end = String::new();
                let _ = reader.read_line(&mut end);
                break;
            }
            if req.body.len() + size > MAX_BODY_BYTES {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
            }
            let start = req.body.len();
            req.body.resize(start + size, 0);
            reader.read_exact(&mut req.body[start..])?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf)?;
        }
    } else if let Some(len) = req.header("content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad content-length"))?;
        if len > MAX_BODY_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
        }
        req.body.resize(len, 0);
        reader.read_exact(&mut req.body)?;
    }
    Ok(req)
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// A response the router returns. `Stream` hands the socket to a closure
/// after the SSE headers are written.
pub enum Response {
    Full {
        status: u16,
        content_type: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Stream {
        status: u16,
        headers: Vec<(String, String)>,
        writer: Box<dyn FnOnce(&mut SseWriter) + Send>,
    },
}

impl Response {
    pub fn json(status: u16, v: &Value) -> Response {
        Response::Full {
            status,
            content_type: "application/json; charset=utf-8".into(),
            headers: Vec::new(),
            body: serde_json::to_vec(v).unwrap_or_default(),
        }
    }

    pub fn with_header(mut self, k: &str, v: &str) -> Response {
        match &mut self {
            Response::Full { headers, .. } | Response::Stream { headers, .. } => {
                headers.push((k.into(), v.into()))
            }
        }
        self
    }

    pub fn status(&self) -> u16 {
        match self {
            Response::Full { status, .. } | Response::Stream { status, .. } => *status,
        }
    }
}

/// OpenAI-style error envelope (upstream `_openai_error`).
pub fn error(status: u16, message: &str, err_type: &str, code: Option<&str>) -> Response {
    Response::json(
        status,
        &serde_json::json!({"error": {"message": crate::redact(message), "type": err_type, "param": null, "code": code}}),
    )
}

pub struct SseWriter<'a> {
    stream: &'a mut (dyn Write + Send),
    pub broken: bool,
}

impl<'a> SseWriter<'a> {
    pub fn new(stream: &'a mut (dyn Write + Send)) -> Self {
        SseWriter {
            stream,
            broken: false,
        }
    }

    pub fn raw(&mut self, bytes: &[u8]) -> bool {
        if self.broken {
            return false;
        }
        if self
            .stream
            .write_all(bytes)
            .and_then(|_| self.stream.flush())
            .is_err()
        {
            self.broken = true;
        }
        !self.broken
    }

    /// `[event: name\n]data: <json>\n\n` (upstream `_sse_frame`).
    pub fn frame(&mut self, event: Option<&str>, data: &Value) -> bool {
        let mut s = String::new();
        if let Some(e) = event {
            s.push_str("event: ");
            s.push_str(e);
            s.push('\n');
        }
        s.push_str("data: ");
        s.push_str(&data.to_string());
        s.push_str("\n\n");
        self.raw(s.as_bytes())
    }
}

pub fn write_response(stream: &mut TcpStream, resp: Response) {
    match resp {
        Response::Full {
            status,
            content_type,
            headers,
            body,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
                reason(status),
                body.len()
            );
            for (k, v) in headers {
                head.push_str(&format!("{k}: {}\r\n", crate::hdr_value(&v, 512)));
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
        Response::Stream {
            status,
            headers,
            writer,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nX-Accel-Buffering: no\r\n",
                reason(status)
            );
            for (k, v) in headers {
                head.push_str(&format!("{k}: {}\r\n", crate::hdr_value(&v, 512)));
            }
            head.push_str("\r\n");
            if stream.write_all(head.as_bytes()).is_err() {
                return;
            }
            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
            let mut w = SseWriter::new(stream);
            writer(&mut w);
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

/// Accept loop. Returns only on listener error.
pub fn serve(listener: TcpListener, handler: Handler, max_conns: usize) -> io::Result<()> {
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let mut stream = match conn {
            Ok(s) => s,
            Err(_) => continue,
        };
        if active.load(Ordering::SeqCst) >= max_conns {
            nerve_client::cap_hit(
                "hermes-rs",
                "conn_cap",
                &format!("{max_conns} connections busy: 503"),
            );
            let resp = error(503, "too many connections", "server_busy", Some("busy"));
            write_response(&mut stream, resp);
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let h = handler.clone();
        let a = active.clone();
        let spawned = std::thread::Builder::new()
            .name("hermes-conn".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                match read_request(&mut stream) {
                    Ok(req) => {
                        let _ = stream.set_read_timeout(None);
                        let resp = h(req);
                        write_response(&mut stream, resp);
                    }
                    Err(e) => {
                        let code = if e.to_string().contains("too large") {
                            413
                        } else {
                            400
                        };
                        write_response(
                            &mut stream,
                            error(code, &e.to_string(), "invalid_request_error", None),
                        );
                    }
                }
                a.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
    Ok(())
}
