// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Std-only HTTP/1.1 for the loopback sidecar whose only client is Hermes'
//! urllib: one short-lived thread per connection, `Connection: close`,
//! Content-Length bodies. The accept loop, connection cap, strict parser and
//! deadlines are galion-common's ([`galion_common::http`]); this module keeps
//! the request view, the reply bytes and the JSON error bodies.

use galion_common::http as common;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The memory view of a request parsed by galion-common: method
    /// upper-cased, header names lower-cased, query string dropped and an
    /// absolute-form target (`http://host/path`) reduced to its path.
    pub fn from_common(req: common::Request) -> Request {
        let common::Request { head, body, .. } = req;
        let path = head.target.split('?').next().unwrap_or("/").to_string();
        let path = match path
            .strip_prefix("http://")
            .or_else(|| path.strip_prefix("https://"))
        {
            Some(rest) => rest
                .find('/')
                .map(|i| rest[i..].to_string())
                .unwrap_or_else(|| "/".into()),
            None => path,
        };
        Request {
            method: head.method.to_ascii_uppercase(),
            path,
            headers: head
                .headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body,
        }
    }
}

pub struct Response {
    pub status: u16,
    pub body: String,
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

/// Server limits: the old 64 KiB head cap and the configured body and
/// connection caps, with total deadlines instead of a 30 s per-read timeout.
///
/// `serve` applies `OPENMIND_MEMORY_HTTP_*` overrides (galion-common
/// `Limits::with_env`) on top of these defaults.
pub fn limits(max_body: usize, max_conns: usize) -> common::Limits {
    common::Limits {
        max_head_bytes: 64 * 1024,
        max_headers: 256,
        max_body_bytes: max_body,
        head_timeout: Duration::from_secs(10),
        body_timeout: Duration::from_secs(30),
        write_timeout: Duration::from_secs(30),
        max_connections: max_conns,
        thread_stack: 512 * 1024,
        accept_chunked: false,
        evict_idle: false,
    }
}

fn error_body(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// The reply for a request galion-common refused; `None` closes quietly
/// (the client went away, stalled, or the socket failed), as before.
pub fn refusal(e: common::Error, max_body: usize) -> Option<Response> {
    use common::Error as E;
    let (status, body) = match e {
        E::BodyTooLarge => (
            413,
            error_body(&format!("Request body exceeds {max_body} bytes")),
        ),
        E::TransferEncoding | E::BadChunk => (
            400,
            error_body("chunked bodies are not supported; send Content-Length"),
        ),
        E::HeadTooLarge | E::TooManyHeaders => (400, error_body("headers too large")),
        E::BadContentLength => (400, error_body("bad content-length")),
        E::BadVersion => (400, error_body("unsupported http version")),
        E::BadRequestLine | E::BadHeader | E::Incomplete => (400, error_body("bad request")),
        E::Timeout | E::Truncated | E::Io => return None,
    };
    Some(Response { status, body })
}

/// Wire bytes of a reply.
pub fn response_bytes(resp: &Response) -> Vec<u8> {
    if resp.status == 204 {
        return b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_vec();
    }
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        resp.status,
        reason(resp.status),
        resp.body.len()
    )
    .into_bytes();
    out.extend_from_slice(resp.body.as_bytes());
    out
}

pub fn write_response(mut stream: &TcpStream, resp: &Response) {
    let _ = stream.write_all(&response_bytes(resp));
    let _ = stream.flush();
}

/// Serve forever. `handler` must be cheap to share across threads.
pub fn serve<H>(listener: TcpListener, max_body: usize, max_conns: usize, handler: Arc<H>)
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    // OPENMIND_MEMORY_HTTP_* (galion-common `Limits::with_env`) override the
    // defaults; its MAX_BODY_BYTES also wins over MEMORY_MAX_BODY_BYTES.
    let limits = limits(max_body, max_conns).with_env("OPENMIND_MEMORY");
    let max_body = limits.max_body_bytes;
    let busy = response_bytes(&Response {
        status: 503,
        body: r#"{"error":"Too many connections"}"#.into(),
    });
    let conn: Arc<common::ConnHandler> = Arc::new(move |mut s: TcpStream, l: &common::Limits| {
        let _ = s.set_write_timeout(Some(l.write_timeout));
        match common::read_request(&mut s, l) {
            Ok(Some(req)) => {
                write_response(&s, &handler(Request::from_common(req)));
                let _ = s.shutdown(std::net::Shutdown::Both);
            }
            Ok(None) => {}
            Err(e) => {
                if let Some(r) = refusal(e, max_body) {
                    write_response(&s, &r);
                }
                common::drain(&mut s);
            }
        }
    });
    // `serve_connections` only returns on a listener error it cannot
    // recover from; it never does in practice (accept errors back off).
    let cap = limits.max_connections;
    // nerve: report conn-cap refusals (rate-limited, bounded queue, no-op
    // without NERVE_URL), as nerve-wiring-2 did in its own accept loop.
    let on_busy: Arc<common::BusyHook> = Arc::new(move || {
        nerve_client::cap_hit(
            "openmind-memory",
            "conn_cap",
            &format!("{cap} connections busy: 503"),
        )
    });
    let _ = common::serve_connections_with(listener, limits, busy, conn, Some(on_busy));
}
