// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Small, strict HTTP/1.x for loopback services: a request parser, a bounded
//! thread-per-connection server and a one-shot client. Std only.
//!
//! Server guarantees (see [`Limits`]):
//! - one request per connection, `Connection: close` (no keep-alive, no
//!   pipelining);
//! - a total deadline for the request head and another for the body, so a
//!   slowloris client that drips one byte at a time is cut off;
//! - caps on head bytes, header count, body bytes and live connections;
//! - strict parsing: single-space request line, `HTTP/1.0`/`HTTP/1.1` only,
//!   token header names with no whitespace before the colon, no obs-fold,
//!   no control bytes, a single numeric `Content-Length`, and
//!   `Transfer-Encoding` refused (501) so CL/TE smuggling cannot happen;
//! - no panics on any input (fuzz-style tests in `tests/http_fuzz.rs`).

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Server limits. The defaults suit small loopback JSON/static services.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Request line plus headers plus the blank line.
    pub max_head_bytes: usize,
    pub max_headers: usize,
    pub max_body_bytes: usize,
    /// Total time allowed for the whole request head (anti-slowloris).
    pub head_timeout: Duration,
    /// Total time allowed for the body after the head.
    pub body_timeout: Duration,
    pub write_timeout: Duration,
    /// Live connections; the next one gets a 503 and is closed.
    pub max_connections: usize,
    /// Stack size of each connection thread.
    pub thread_stack: usize,
    /// At the connection cap, close the oldest connection that has not yet
    /// delivered a complete request (head + body) instead of refusing the
    /// new one, so idle or slowloris connections cannot lock real clients
    /// out for the head deadline. Live connections stay below twice
    /// `max_connections` while evicted threads wind down. With nothing
    /// to evict, a new connection waits up to [`EVICT_GRACE`] for a slot
    /// before the 503. Off by default.
    pub evict_idle: bool,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_head_bytes: 8 * 1024,
            max_headers: 64,
            max_body_bytes: 1024 * 1024,
            head_timeout: Duration::from_secs(5),
            body_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
            max_connections: 32,
            thread_stack: 256 * 1024,
            evict_idle: false,
        }
    }
}

/// Why a request was refused. [`Error::status`] is the reply code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// More bytes are needed (only from [`parse_head`]).
    Incomplete,
    HeadTooLarge,
    TooManyHeaders,
    BadRequestLine,
    BadVersion,
    BadHeader,
    BadContentLength,
    TransferEncoding,
    BodyTooLarge,
    /// The client closed before the request was complete.
    Truncated,
    Timeout,
    Io,
}

impl Error {
    pub fn status(self) -> u16 {
        match self {
            Error::HeadTooLarge | Error::TooManyHeaders => 431,
            Error::BadVersion => 505,
            Error::TransferEncoding => 501,
            Error::BodyTooLarge => 413,
            Error::Timeout => 408,
            Error::Io => 500,
            Error::Incomplete
            | Error::BadRequestLine
            | Error::BadHeader
            | Error::BadContentLength
            | Error::Truncated => 400,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// A parsed request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub target: String,
    /// 0 for HTTP/1.0, 1 for HTTP/1.1.
    pub minor: u8,
    /// Header names as sent; values trimmed of SP/HTAB.
    pub headers: Vec<(String, String)>,
    pub content_length: Option<u64>,
    /// Bytes of the head including the blank line.
    pub len: usize,
}

/// A full request as handed to a [`serve`] handler.
#[derive(Debug, Clone)]
pub struct Request {
    pub head: Head,
    /// The raw head bytes (request line through blank line).
    pub raw_head: Vec<u8>,
    pub body: Vec<u8>,
    pub peer: Option<SocketAddr>,
}

impl Request {
    pub fn method(&self) -> &str {
        &self.head.method
    }
    pub fn target(&self) -> &str {
        &self.head.target
    }
    /// Target without the query string.
    pub fn path(&self) -> &str {
        self.head.target.split('?').next().unwrap_or("")
    }
    /// Query string without `?` (empty when absent).
    pub fn query(&self) -> &str {
        self.head.target.split_once('?').map_or("", |(_, q)| q)
    }
    /// First header with this name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.head.headers, name)
    }
}

/// First header with this name (case-insensitive).
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// RFC 9110 `tchar`.
pub fn is_tchar(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}

fn is_token(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(|&c| is_tchar(c))
}

/// End of the head: index just past the blank line (`\r\n\r\n` or `\n\n`,
/// mixed allowed), and the index where the header text ends.
fn find_head_end(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' {
            match (buf.get(i + 1), buf.get(i + 2)) {
                (Some(b'\n'), _) => return Some((i, i + 2)),
                (Some(b'\r'), Some(b'\n')) => return Some((i, i + 3)),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Parse a request head from the start of `buf`.
///
/// `Err(Error::Incomplete)` means "read more"; every other error is final.
/// Never panics, whatever the bytes.
pub fn parse_head(buf: &[u8], limits: &Limits) -> Result<Head, Error> {
    let Some((text_end, len)) = find_head_end(buf) else {
        return Err(if buf.len() > limits.max_head_bytes {
            Error::HeadTooLarge
        } else {
            Error::Incomplete
        });
    };
    if len > limits.max_head_bytes {
        return Err(Error::HeadTooLarge);
    }
    let mut lines = buf[..text_end].split(|&c| c == b'\n').map(|l| match l {
        [rest @ .., b'\r'] => rest,
        l => l,
    });
    let first = lines.next().unwrap_or(&[]);
    let mut parts = first.split(|&c| c == b' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::BadRequestLine);
    };
    if !is_token(method) || method.len() > 32 {
        return Err(Error::BadRequestLine);
    }
    if target.is_empty() || !target.iter().all(|&c| (0x21..=0x7e).contains(&c)) {
        return Err(Error::BadRequestLine);
    }
    let minor = match version {
        b"HTTP/1.1" => 1,
        b"HTTP/1.0" => 0,
        v if v.starts_with(b"HTTP/") => return Err(Error::BadVersion),
        _ => return Err(Error::BadRequestLine),
    };
    let mut headers = Vec::new();
    let mut content_length: Option<u64> = None;
    for line in lines {
        if headers.len() >= limits.max_headers {
            return Err(Error::TooManyHeaders);
        }
        let colon = line
            .iter()
            .position(|&c| c == b':')
            .ok_or(Error::BadHeader)?;
        let (name, value) = (&line[..colon], &line[colon + 1..]);
        // Rejects obs-fold (leading SP/HTAB) and "Name : v" too.
        if !is_token(name) {
            return Err(Error::BadHeader);
        }
        if !value
            .iter()
            .all(|&c| c == b'\t' || (0x20..=0x7e).contains(&c) || c >= 0x80)
        {
            return Err(Error::BadHeader);
        }
        let value = trim_ows(value);
        let name = String::from_utf8_lossy(name).into_owned();
        let value = String::from_utf8_lossy(value).into_owned();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(Error::TransferEncoding);
        }
        if name.eq_ignore_ascii_case("content-length") {
            let n = parse_len(&value).ok_or(Error::BadContentLength)?;
            if content_length.is_some_and(|old| old != n) {
                return Err(Error::BadContentLength);
            }
            content_length = Some(n);
        }
        headers.push((name, value));
    }
    Ok(Head {
        method: String::from_utf8_lossy(method).into_owned(),
        target: String::from_utf8_lossy(target).into_owned(),
        minor,
        headers,
        content_length,
        len,
    })
}

fn trim_ows(mut v: &[u8]) -> &[u8] {
    while let [b' ' | b'\t', rest @ ..] = v {
        v = rest;
    }
    while let [rest @ .., b' ' | b'\t'] = v {
        v = rest;
    }
    v
}

fn parse_len(v: &str) -> Option<u64> {
    if v.is_empty() || v.len() > 18 || !v.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    v.parse().ok()
}

/// Reason phrase for the codes this stack uses.
pub fn reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

/// A reply. Either structured (status, headers, body) or raw bytes that the
/// handler built itself (for byte-exact ports of older servers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    raw: Option<Vec<u8>>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        Response {
            status,
            headers: vec![("content-type".into(), content_type.into())],
            body: body.into(),
            raw: None,
        }
    }
    pub fn text(status: u16, body: impl Into<Vec<u8>>) -> Response {
        Response::new(status, "text/plain; charset=utf-8", body)
    }
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Response {
        Response::new(status, "application/json", body)
    }
    /// Exact bytes to write; nothing is added.
    pub fn raw(bytes: Vec<u8>) -> Response {
        Response {
            status: 0,
            headers: vec![],
            body: vec![],
            raw: Some(bytes),
        }
    }
    /// Close without writing anything.
    pub fn none() -> Response {
        Response::raw(Vec::new())
    }
    pub fn with_header(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.into(), value.into()));
        self
    }
    /// Wire bytes. Headers with CR, LF or NUL are dropped (no injection).
    /// `head_only` (HEAD) keeps `content-length` but omits the body.
    pub fn to_bytes(&self, head_only: bool) -> Vec<u8> {
        if let Some(raw) = &self.raw {
            return raw.clone();
        }
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (k, v) in &self.headers {
            let bad = |s: &str| s.bytes().any(|c| matches!(c, b'\r' | b'\n' | 0));
            if bad(k) || bad(v) || !is_token(k.as_bytes()) {
                continue;
            }
            if k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("connection") {
                continue;
            }
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        out.push_str(&format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            self.body.len()
        ));
        let mut out = out.into_bytes();
        if !head_only {
            out.extend_from_slice(&self.body);
        }
        out
    }
}

fn error_bytes(e: Error) -> Vec<u8> {
    let code = e.status();
    let body = format!("{}\n", reason(code));
    format!(
        "HTTP/1.1 {code} {}\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        reason(code),
        body.len()
    )
    .into_bytes()
}

fn remaining(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
}

fn timed_out(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Read one request from `s`. `Ok(None)` when the client sent nothing at
/// all (closed or idle until the head deadline): the caller closes quietly.
pub fn read_request(s: &mut TcpStream, limits: &Limits) -> Result<Option<Request>, Error> {
    let peer = s.peer_addr().ok();
    let deadline = Instant::now() + limits.head_timeout;
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let head = loop {
        match parse_head(&buf, limits) {
            Ok(h) => break h,
            Err(Error::Incomplete) => {}
            Err(e) => return Err(e),
        }
        let Some(left) = remaining(deadline) else {
            return if buf.is_empty() {
                Ok(None)
            } else {
                Err(Error::Timeout)
            };
        };
        s.set_read_timeout(Some(left)).map_err(|_| Error::Io)?;
        let cap = (limits.max_head_bytes + 1)
            .saturating_sub(buf.len())
            .clamp(1, chunk.len());
        match s.read(&mut chunk[..cap]) {
            Ok(0) => {
                return if buf.is_empty() {
                    Ok(None)
                } else {
                    Err(Error::Truncated)
                }
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if timed_out(&e) => {
                return if buf.is_empty() {
                    Ok(None)
                } else {
                    Err(Error::Timeout)
                }
            }
            Err(_) => return Err(Error::Io),
        }
    };
    let want = head.content_length.unwrap_or(0);
    if want > limits.max_body_bytes as u64 {
        return Err(Error::BodyTooLarge);
    }
    let want = want as usize;
    let raw_head = buf[..head.len].to_vec();
    let mut body = buf[head.len..].to_vec();
    body.truncate(want);
    if body.len() < want {
        let expect =
            header(&head.headers, "expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue"));
        if expect && head.minor == 1 {
            let _ = s.set_write_timeout(Some(limits.write_timeout));
            let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
        }
        let deadline = Instant::now() + limits.body_timeout;
        body.reserve_exact(want - body.len());
        while body.len() < want {
            let left = remaining(deadline).ok_or(Error::Timeout)?;
            s.set_read_timeout(Some(left)).map_err(|_| Error::Io)?;
            let cap = (want - body.len()).min(chunk.len());
            match s.read(&mut chunk[..cap]) {
                Ok(0) => return Err(Error::Truncated),
                Ok(n) => body.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if timed_out(&e) => return Err(Error::Timeout),
                Err(_) => return Err(Error::Io),
            }
        }
    }
    Ok(Some(Request {
        head,
        raw_head,
        body,
        peer,
    }))
}

/// After an error reply, read and discard what the client is still sending
/// for a moment so the kernel does not answer with RST before our reply is
/// read. Bounded by time and bytes.
fn drain(s: &mut TcpStream) {
    let _ = s.shutdown(Shutdown::Write);
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut sink = [0u8; 4096];
    let mut total = 0usize;
    while total < 256 * 1024 {
        let Some(left) = remaining(deadline) else {
            return;
        };
        if s.set_read_timeout(Some(left)).is_err() {
            return;
        }
        match s.read(&mut sink) {
            Ok(0) | Err(_) => return,
            Ok(n) => total += n,
        }
    }
}

/// Request handler for [`serve`].
pub type Handler = dyn Fn(&Request) -> Response + Send + Sync;

/// Serve one connection: read, handle, write, close.
pub fn handle_connection<F>(s: TcpStream, limits: &Limits, handler: &F)
where
    F: Fn(&Request) -> Response + ?Sized,
{
    serve_one(s, limits, handler, &|| {});
}

/// [`handle_connection`] with a hook called as soon as the request has been
/// read (or reading failed), before the handler runs.
fn serve_one<F>(mut s: TcpStream, limits: &Limits, handler: &F, read_done: &dyn Fn())
where
    F: Fn(&Request) -> Response + ?Sized,
{
    let _ = s.set_nodelay(true);
    let _ = s.set_write_timeout(Some(limits.write_timeout));
    let got = read_request(&mut s, limits);
    read_done();
    match got {
        Ok(Some(req)) => {
            let reply = handler(&req);
            let bytes = reply.to_bytes(req.head.method == "HEAD");
            if !bytes.is_empty() {
                let _ = s.write_all(&bytes);
            }
            let _ = s.flush();
            let _ = s.shutdown(Shutdown::Both);
        }
        Ok(None) => {}
        Err(e) => {
            let _ = s.write_all(&error_bytes(e));
            drain(&mut s);
        }
    }
}

/// Connections still reading their request, oldest first (for
/// [`Limits::evict_idle`]).
type Reading = Mutex<VecDeque<(u64, TcpStream)>>;

fn reading_remove(r: &Reading, id: u64) {
    let mut g = r.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = g.iter().position(|(k, _)| *k == id) {
        g.remove(i);
    }
}

/// Close the oldest connection that is still reading its request. Its
/// blocked read returns at once and its thread exits on its own.
fn evict_oldest(r: &Reading) -> bool {
    let oldest = r.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
    match oldest {
        Some((_, s)) => {
            let _ = s.shutdown(Shutdown::Both);
            true
        }
        None => false,
    }
}

/// At the cap with [`Limits::evict_idle`]: evict the oldest connection still
/// reading its request; if none, queue the new one for up to
/// [`EVICT_GRACE`] while finished connections wind down. `false` = refuse.
fn make_room(limits: &Limits, live: &AtomicUsize, reading: &Reading) -> bool {
    let deadline = Instant::now() + EVICT_GRACE;
    loop {
        let n = live.load(Ordering::SeqCst);
        if n < limits.max_connections {
            return true;
        }
        if n < limits.max_connections.saturating_mul(2) && evict_oldest(reading) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// How long [`serve`] holds a new connection at the cap (evict mode) before
/// it answers 503.
pub const EVICT_GRACE: Duration = Duration::from_millis(100);

/// Accept forever on `listener`, one thread per connection, at most
/// `limits.max_connections` at once (the rest get a 503, or with
/// [`Limits::evict_idle`] the oldest connection still reading its request
/// is closed to make room).
pub fn serve(listener: TcpListener, limits: Limits, handler: Arc<Handler>) -> io::Result<()> {
    let limits = Arc::new(limits);
    let live = Arc::new(AtomicUsize::new(0));
    let reading: Arc<Reading> = Arc::new(Mutex::new(VecDeque::new()));
    let mut next_id = 0u64;
    loop {
        let s = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                // EMFILE and friends: back off instead of spinning.
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        if live.load(Ordering::SeqCst) >= limits.max_connections
            && !(limits.evict_idle && make_room(&limits, &live, &reading))
        {
            reject_busy(s);
            continue;
        }
        next_id = next_id.wrapping_add(1);
        let id = next_id;
        if limits.evict_idle {
            if let Ok(c) = s.try_clone() {
                reading
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push_back((id, c));
            }
        }
        live.fetch_add(1, Ordering::SeqCst);
        let (limits2, live2, handler2, reading2) = (
            limits.clone(),
            live.clone(),
            handler.clone(),
            reading.clone(),
        );
        let mut slot = Some(s);
        let spawned = std::thread::Builder::new()
            .stack_size(limits.thread_stack)
            .spawn(move || {
                if let Some(s) = slot.take() {
                    serve_one(s, &limits2, &*handler2, &|| reading_remove(&reading2, id));
                }
                reading_remove(&reading2, id);
                live2.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            reading_remove(&reading, id);
            live.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn reject_busy(mut s: TcpStream) {
    let _ = s.set_write_timeout(Some(Duration::from_millis(200)));
    let body = "busy\n";
    let _ = s.write_all(
        format!(
            "HTTP/1.1 503 Service Unavailable\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nretry-after: 1\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    );
    let _ = s.shutdown(Shutdown::Both);
}

// ---------------------------------------------------------------- client

/// A request for [`send`]. Nothing is added: the caller lists every header
/// in wire order (use [`ClientRequest::get`] / [`ClientRequest::post`] for
/// the usual `Host` / `Content-Length` / `Connection: close` set).
#[derive(Debug, Clone)]
pub struct ClientRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    /// 0 = HTTP/1.0, 1 = HTTP/1.1.
    pub minor: u8,
    pub headers: Vec<(String, String)>,
    pub body: &'a [u8],
}

impl<'a> ClientRequest<'a> {
    pub fn get(path: &'a str, host: &str) -> ClientRequest<'a> {
        ClientRequest {
            method: "GET",
            path,
            minor: 0,
            headers: vec![
                ("Host".into(), host.into()),
                ("Connection".into(), "close".into()),
            ],
            body: &[],
        }
    }
    pub fn post(path: &'a str, host: &str, content_type: &str, body: &'a [u8]) -> Self {
        ClientRequest {
            method: "POST",
            path,
            minor: 0,
            headers: vec![
                ("Host".into(), host.into()),
                ("Content-Type".into(), content_type.into()),
                ("Content-Length".into(), body.len().to_string()),
                ("Connection".into(), "close".into()),
            ],
            body,
        }
    }
    /// Exact wire bytes, or why the request is unsafe to send.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ClientError> {
        if !is_token(self.method.as_bytes()) {
            return Err(ClientError::Invalid("method"));
        }
        if self.path.is_empty() || !self.path.bytes().all(|c| (0x21..=0x7e).contains(&c)) {
            return Err(ClientError::Invalid("path"));
        }
        if self.minor > 1 {
            return Err(ClientError::Invalid("version"));
        }
        let mut out = format!("{} {} HTTP/1.{}\r\n", self.method, self.path, self.minor);
        for (k, v) in &self.headers {
            if !is_token(k.as_bytes()) {
                return Err(ClientError::Invalid("header name"));
            }
            if v.bytes().any(|c| matches!(c, b'\r' | b'\n' | 0)) {
                return Err(ClientError::Invalid("header value"));
            }
            out.push_str(k);
            out.push_str(": ");
            out.push_str(v);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        let mut out = out.into_bytes();
        out.extend_from_slice(self.body);
        Ok(out)
    }
}

/// A response from [`send`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientResponse {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// False when the deadline hit after the head arrived (body may be short).
    pub complete: bool,
}

impl ClientResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Client failure. The `Display` text matches the old hand-rolled clients
/// (`connect: ...`, `write: ...`, `read: ...`, `bad status line`).
#[derive(Debug)]
pub enum ClientError {
    Connect(io::Error),
    Write(io::Error),
    Read(io::Error),
    BadStatus,
    TooLarge,
    Invalid(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Connect(e) => write!(f, "connect: {e}"),
            ClientError::Write(e) => write!(f, "write: {e}"),
            ClientError::Read(e) => write!(f, "read: {e}"),
            ClientError::BadStatus => write!(f, "bad status line"),
            ClientError::TooLarge => write!(f, "response too large"),
            ClientError::Invalid(what) => write!(f, "invalid request {what}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Parse a response head (`HTTP/1.x SSS reason`). `None` when incomplete or
/// malformed; never panics.
pub fn parse_response_head(buf: &[u8]) -> ParsedHead {
    let (text_end, len) = find_head_end(buf)?;
    let text = String::from_utf8_lossy(&buf[..text_end]);
    let mut lines = text.lines();
    let status_line = lines.next()?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    let code = parts.next()?;
    if code.len() != 3 || !code.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let status = code.parse().ok()?;
    let reason = parts.next().unwrap_or("").trim().to_string();
    let headers = lines
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    Some((status, reason, headers, len))
}

/// Decode a chunked body. `None` when malformed or incomplete.
pub fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let nl = b.iter().position(|&c| c == b'\n')?;
        let line = std::str::from_utf8(&b[..nl]).ok()?.trim();
        let size = line.split(';').next()?.trim();
        let n = usize::from_str_radix(size, 16).ok()?;
        b = &b[nl + 1..];
        if n == 0 {
            return Some(out);
        }
        if b.len() < n {
            return None;
        }
        out.extend_from_slice(&b[..n]);
        b = &b[n..];
        b = b.strip_prefix(b"\r\n").or_else(|| b.strip_prefix(b"\n"))?;
    }
}

/// One request, one response, connection closed. `timeout` bounds connect
/// and the whole exchange; `max_response` caps the bytes read.
pub fn send(
    addr: SocketAddr,
    req: &ClientRequest<'_>,
    timeout: Duration,
    max_response: usize,
) -> Result<ClientResponse, ClientError> {
    let bytes = req.to_bytes()?;
    let deadline = Instant::now() + timeout;
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(ClientError::Connect)?;
    s.set_write_timeout(Some(timeout))
        .map_err(ClientError::Write)?;
    s.write_all(&bytes).map_err(ClientError::Write)?;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let mut head = None;
    loop {
        if head.is_none() {
            head = parse_response_head(&buf);
        }
        if let Some((_, _, h, len)) = &head {
            if let Some(cl) = header(h, "content-length").and_then(parse_len) {
                if (buf.len() - len) as u64 >= cl {
                    break;
                }
            }
        }
        let Some(left) = remaining(deadline) else {
            if head.is_some() {
                return finish(buf, head, false);
            }
            return Err(ClientError::Read(io::ErrorKind::TimedOut.into()));
        };
        s.set_read_timeout(Some(left)).map_err(ClientError::Read)?;
        let cap = (max_response + 1)
            .saturating_sub(buf.len())
            .clamp(1, chunk.len());
        match s.read(&mut chunk[..cap]) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > max_response {
                    return Err(ClientError::TooLarge);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if timed_out(&e) && head.is_some() => return finish(buf, head, false),
            Err(e) => {
                if head.is_none() && buf.is_empty() {
                    return Err(ClientError::Read(e));
                }
                break;
            }
        }
    }
    if head.is_none() {
        head = parse_response_head(&buf);
        if head.is_none() {
            // A head with no blank line yet (server closed early): accept
            // the status line alone, like the old clients did.
            let line_end = buf.iter().position(|&c| c == b'\n').unwrap_or(buf.len());
            let mut tmp = buf[..line_end].to_vec();
            tmp.extend_from_slice(b"\r\n\r\n");
            let (st, r, h, _) = parse_response_head(&tmp).ok_or(ClientError::BadStatus)?;
            return Ok(ClientResponse {
                status: st,
                reason: r,
                headers: h,
                body: vec![],
                complete: false,
            });
        }
    }
    finish(buf, head, true)
}

/// (status, reason, headers, head length).
pub type ParsedHead = Option<(u16, String, Vec<(String, String)>, usize)>;

fn finish(buf: Vec<u8>, head: ParsedHead, complete: bool) -> Result<ClientResponse, ClientError> {
    let (status, reason, headers, len) = head.ok_or(ClientError::BadStatus)?;
    let mut body = buf[len.min(buf.len())..].to_vec();
    if let Some(cl) = header(&headers, "content-length").and_then(parse_len) {
        body.truncate(cl.min(usize::MAX as u64) as usize);
    }
    let chunked = header(&headers, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        if let Some(b) = dechunk(&body) {
            body = b;
        }
    }
    Ok(ClientResponse {
        status,
        reason,
        headers,
        body,
        complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Head, Error> {
        parse_head(s.as_bytes(), &Limits::default())
    }

    #[test]
    fn parses_good_heads() {
        let h = p("GET /a?b=1 HTTP/1.1\r\nHost: x\r\nX-A:  v \t\r\n\r\nBODY").unwrap();
        assert_eq!(h.method, "GET");
        assert_eq!(h.target, "/a?b=1");
        assert_eq!(h.minor, 1);
        assert_eq!(h.headers[1], ("X-A".into(), "v".into()));
        assert_eq!(
            h.len,
            "GET /a?b=1 HTTP/1.1\r\nHost: x\r\nX-A:  v \t\r\n\r\n".len()
        );
        let h = p("POST / HTTP/1.0\nContent-Length: 3\n\nabc").unwrap();
        assert_eq!(h.content_length, Some(3));
        assert_eq!(h.minor, 0);
        let h = p("GET / HTTP/1.1\r\nA: 1\r\nContent-Length: 2\r\ncontent-length: 2\r\n\r\n");
        assert_eq!(h.unwrap().content_length, Some(2));
    }

    #[test]
    fn rejects_bad_heads() {
        use Error::*;
        let cases: &[(&str, Error)] = &[
            ("GET / HTTP/1.1\r\n", Incomplete),
            ("GET /  HTTP/1.1\r\n\r\n", BadRequestLine),
            ("GET / HTTP/1.1 x\r\n\r\n", BadRequestLine),
            ("G(T / HTTP/1.1\r\n\r\n", BadRequestLine),
            ("GET /\x01 HTTP/1.1\r\n\r\n", BadRequestLine),
            ("GET / HTTP/2.0\r\n\r\n", BadVersion),
            ("GET / FTP/1.0\r\n\r\n", BadRequestLine),
            ("\r\n\r\n", BadRequestLine),
            ("GET / HTTP/1.1\r\nHost : x\r\n\r\n", BadHeader),
            ("GET / HTTP/1.1\r\nNoColon\r\n\r\n", BadHeader),
            ("GET / HTTP/1.1\r\nA: b\r\n folded\r\n\r\n", BadHeader),
            ("GET / HTTP/1.1\r\nA: b\rc\r\n\r\n", BadHeader),
            ("GET / HTTP/1.1\r\nA: b\x00\r\n\r\n", BadHeader),
            (
                "GET / HTTP/1.1\r\nContent-Length: -1\r\n\r\n",
                BadContentLength,
            ),
            (
                "GET / HTTP/1.1\r\nContent-Length: 1, 1\r\n\r\n",
                BadContentLength,
            ),
            (
                "GET / HTTP/1.1\r\nContent-Length: 99999999999999999999\r\n\r\n",
                BadContentLength,
            ),
            (
                "GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
                BadContentLength,
            ),
            (
                "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
                TransferEncoding,
            ),
        ];
        for (req, want) in cases {
            assert_eq!(p(req), Err(*want), "{req:?}");
        }
        let big = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(9000));
        assert_eq!(p(&big), Err(HeadTooLarge));
        assert_eq!(p(&"a".repeat(9000)), Err(HeadTooLarge));
        let many: String = (0..70).map(|i| format!("H{i}: v\r\n")).collect();
        assert_eq!(
            p(&format!("GET / HTTP/1.1\r\n{many}\r\n")),
            Err(TooManyHeaders)
        );
    }

    #[test]
    fn response_bytes() {
        let r = Response::text(200, "hi").with_header("x-bad", "a\r\nb: c");
        let b = String::from_utf8(r.to_bytes(false)).unwrap();
        assert_eq!(
            b,
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: 2\r\nconnection: close\r\n\r\nhi"
        );
        assert!(String::from_utf8(r.to_bytes(true))
            .unwrap()
            .ends_with("\r\n\r\n"));
        assert_eq!(Response::raw(b"x".to_vec()).to_bytes(true), b"x");
    }

    #[test]
    fn client_request_bytes_and_injection() {
        let r = ClientRequest::post("/x", "127.0.0.1:1", "application/json", b"{}");
        assert_eq!(
            r.to_bytes().unwrap(),
            b"POST /x HTTP/1.0\r\nHost: 127.0.0.1:1\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"
        );
        let mut bad = ClientRequest::get("/x\r\nEvil: 1", "h");
        assert!(bad.to_bytes().is_err());
        bad.path = "/ok";
        bad.headers.push(("A".into(), "1\r\nEvil: 1".into()));
        assert!(bad.to_bytes().is_err());
    }

    #[test]
    fn response_head_and_chunks() {
        let (st, r, h, len) =
            parse_response_head(b"HTTP/1.0 404 Not Found\r\nA: b\r\n\r\nxx").unwrap();
        assert_eq!((st, r.as_str(), h.len(), len), (404, "Not Found", 1, 32));
        assert!(parse_response_head(b"HTTP/1.0 4x4 X\r\n\r\n").is_none());
        assert!(parse_response_head(b"garbage\r\n\r\n").is_none());
        assert_eq!(
            dechunk(b"3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n").unwrap(),
            b"abcde"
        );
        assert!(dechunk(b"zz\r\n").is_none());
        assert!(dechunk(b"10\r\nabc").is_none());
    }
}
