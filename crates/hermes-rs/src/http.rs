// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Blocking HTTP/1.0 + 1.1 server: one request per connection,
//! `Connection: close`, close-delimited SSE bodies (works for HTTP/1.0
//! clients like Galion One and for node fetch). The accept loop, connection
//! cap, request parser and deadlines are galion-common's strict server
//! ([`galion_common::http`]); this module keeps the Hermes request view,
//! the reply bytes and the OpenAI-style error envelope.

use galion_common::http as common;
use serde_json::Value;
use std::io::{self, Write};
use std::net::{TcpListener, TcpStream};
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

    /// The Hermes view of a request parsed by galion-common: header names
    /// lower-cased, target split into path and query, body already decoded
    /// (Content-Length or chunked).
    pub fn from_common(req: common::Request) -> Request {
        let common::Request { head, body, .. } = req;
        let (path, query) = match head.target.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (head.target, String::new()),
        };
        Request {
            method: head.method,
            path,
            query,
            headers: head
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body,
        }
    }
}

/// Server limits: the old caps (64 KiB head, 8 MiB body, 64 connections,
/// chunked uploads accepted) plus total deadlines instead of a 30 s
/// per-read timeout.
///
/// `serve` applies `HERMES_RS_HTTP_*` overrides (galion-common
/// `Limits::with_env`) on top of these defaults.
pub fn limits(max_conns: usize) -> common::Limits {
    common::Limits {
        max_head_bytes: MAX_HEADER_BYTES,
        max_headers: 256,
        max_body_bytes: MAX_BODY_BYTES,
        head_timeout: Duration::from_secs(10),
        body_timeout: Duration::from_secs(60),
        write_timeout: Duration::from_secs(30),
        max_connections: max_conns,
        thread_stack: 256 * 1024,
        accept_chunked: true,
        evict_idle: false,
    }
}

/// The Hermes error reply for a request galion-common refused.
pub fn refusal(e: common::Error) -> Response {
    use common::Error as E;
    let (code, msg) = match e {
        E::HeadTooLarge | E::TooManyHeaders => (413, "headers too large"),
        E::BodyTooLarge => (413, "body too large"),
        E::BadContentLength => (400, "bad content-length"),
        E::BadChunk => (400, "bad chunk"),
        E::TransferEncoding => (501, "unsupported transfer-encoding"),
        E::BadVersion => (505, "unsupported http version"),
        E::Timeout => (408, "request timeout"),
        E::Truncated => (400, "truncated request"),
        E::Incomplete | E::BadRequestLine | E::BadHeader | E::Io => (400, "bad request"),
    };
    error(code, msg, "invalid_request_error", None)
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
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

/// Wire bytes of a `Full` reply (`None` for a stream).
pub fn full_bytes(resp: &Response) -> Option<Vec<u8>> {
    let Response::Full {
        status,
        content_type,
        headers,
        body,
    } = resp
    else {
        return None;
    };
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        reason(*status),
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {}\r\n", crate::hdr_value(v, 512)));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    Some(out)
}

pub fn write_response(stream: &mut TcpStream, resp: Response) {
    if let Some(bytes) = full_bytes(&resp) {
        let _ = stream.write_all(&bytes);
        let _ = stream.flush();
    } else if let Response::Stream {
        status,
        headers,
        writer,
    } = resp
    {
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
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

pub type Handler = Arc<dyn Fn(Request) -> Response + Send + Sync>;

/// One connection: strict read (galion-common), route, write, close.
fn connection(mut stream: TcpStream, limits: &common::Limits, h: &Handler) {
    match common::read_request(&mut stream, limits) {
        Ok(Some(req)) => {
            let _ = stream.set_read_timeout(None);
            let resp = h(Request::from_common(req));
            write_response(&mut stream, resp);
        }
        // Connected and sent nothing (or closed): nothing to answer.
        Ok(None) => {}
        Err(e) => {
            if let Some(bytes) = full_bytes(&refusal(e)) {
                let _ = stream.write_all(&bytes);
            }
            common::drain(&mut stream);
        }
    }
}

/// Accept loop. Returns only on listener error.
pub fn serve(listener: TcpListener, handler: Handler, max_conns: usize) -> io::Result<()> {
    let busy = full_bytes(&error(
        503,
        "too many connections",
        "server_busy",
        Some("busy"),
    ))
    .unwrap_or_default();
    let conn: Arc<common::ConnHandler> =
        Arc::new(move |s: TcpStream, l: &common::Limits| connection(s, l, &handler));
    let limits = limits(max_conns).with_env("HERMES_RS");
    let cap = limits.max_connections;
    // nerve: report conn-cap refusals (rate-limited, bounded queue, no-op
    // without NERVE_URL), as nerve-wiring-2 did in its own accept loop.
    let on_busy: Arc<common::BusyHook> = Arc::new(move || {
        nerve_client::cap_hit(
            "hermes-rs",
            "conn_cap",
            &format!("{cap} connections busy: 503"),
        )
    });
    common::serve_connections_with(listener, limits, busy, conn, Some(on_busy))
}
