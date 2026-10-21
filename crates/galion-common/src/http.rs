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
//!   `Transfer-Encoding` refused (501) so CL/TE smuggling cannot happen.
//!   A server that must keep accepting chunked uploads opts in with
//!   [`Limits::accept_chunked`]: then exactly one `Transfer-Encoding:
//!   chunked` and no `Content-Length` is accepted, decoded strictly within
//!   `max_body_bytes` and `body_timeout`;
//! - no panics on any input (fuzz-style tests in `tests/http_fuzz.rs`).
//!
//! Building blocks for servers that need more than [`serve`] (streaming
//! replies, their own error bodies): [`serve_connections`] (the bounded
//! accept loop), [`read_request`], [`handle_connection_with`] and [`drain`].

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
    /// Accept `Transfer-Encoding: chunked` request bodies (off by default;
    /// any other transfer coding is still refused with 501).
    pub accept_chunked: bool,
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
            accept_chunked: false,
        }
    }
}

impl Limits {
    /// The environment variables [`Limits::with_env`] reads, after
    /// `<PREFIX>_HTTP_`. Sizes and counts are plain numbers, times are
    /// milliseconds.
    pub const ENV_KEYS: [&'static str; 7] = [
        "MAX_HEAD_BYTES",
        "MAX_HEADERS",
        "MAX_BODY_BYTES",
        "HEAD_TIMEOUT_MS",
        "BODY_TIMEOUT_MS",
        "WRITE_TIMEOUT_MS",
        "MAX_CONNECTIONS",
    ];

    /// Override limits from `<PREFIX>_HTTP_<KEY>` environment variables
    /// (keys in [`Limits::ENV_KEYS`]), e.g. `ROOM_HTTP_HEAD_TIMEOUT_MS=30000`.
    /// Unset variables keep the value from `self`. A value that is not a
    /// whole number above zero is ignored, with a warning on stderr.
    pub fn with_env(self, prefix: &str) -> Limits {
        self.with_vars(prefix, |k| std::env::var(k).ok())
    }

    /// [`Limits::with_env`] with an explicit lookup (for tests and config
    /// files).
    pub fn with_vars(mut self, prefix: &str, get: impl Fn(&str) -> Option<String>) -> Limits {
        for key in Self::ENV_KEYS {
            let name = format!("{prefix}_HTTP_{key}");
            let Some(raw) = get(&name) else { continue };
            let Some(n) = raw.trim().parse::<u64>().ok().filter(|n| *n > 0) else {
                eprintln!("galion-common: ignoring {name}: want a whole number above 0");
                continue;
            };
            let size = usize::try_from(n).unwrap_or(usize::MAX);
            let ms = Duration::from_millis(n);
            match key {
                "MAX_HEAD_BYTES" => self.max_head_bytes = size,
                "MAX_HEADERS" => self.max_headers = size,
                "MAX_BODY_BYTES" => self.max_body_bytes = size,
                "HEAD_TIMEOUT_MS" => self.head_timeout = ms,
                "BODY_TIMEOUT_MS" => self.body_timeout = ms,
                "WRITE_TIMEOUT_MS" => self.write_timeout = ms,
                _ => self.max_connections = size,
            }
        }
        self
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
    /// Malformed chunked body (only with [`Limits::accept_chunked`]).
    BadChunk,
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
            | Error::BadChunk
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
    /// `Transfer-Encoding: chunked` (only with [`Limits::accept_chunked`]).
    pub chunked: bool,
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
    let mut chunked = false;
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
            if !limits.accept_chunked || chunked || !value.eq_ignore_ascii_case("chunked") {
                return Err(Error::TransferEncoding);
            }
            chunked = true;
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
    if chunked && content_length.is_some() {
        // RFC 9112 6.3: a sender must not send both; refuse rather than pick.
        return Err(Error::BadContentLength);
    }
    Ok(Head {
        method: String::from_utf8_lossy(method).into_owned(),
        target: String::from_utf8_lossy(target).into_owned(),
        minor,
        headers,
        content_length,
        chunked,
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
    let raw_head = buf[..head.len].to_vec();
    let leftover = buf[head.len..].to_vec();
    let body = if head.chunked {
        continue_100(s, &head, limits, leftover.is_empty());
        read_chunked(s, leftover, limits)?
    } else {
        let want = head.content_length.unwrap_or(0);
        if want > limits.max_body_bytes as u64 {
            return Err(Error::BodyTooLarge);
        }
        let want = want as usize;
        let mut body = leftover;
        body.truncate(want);
        if body.len() < want {
            continue_100(s, &head, limits, true);
            let deadline = Instant::now() + limits.body_timeout;
            body.reserve_exact(want - body.len());
            while body.len() < want {
                let cap = (want - body.len()).min(chunk.len());
                let n = read_some(s, &mut chunk[..cap], deadline)?;
                body.extend_from_slice(&chunk[..n]);
            }
        }
        body
    };
    Ok(Some(Request {
        head,
        raw_head,
        body,
        peer,
    }))
}

/// `100 Continue` for an HTTP/1.1 client that asked for it and has not
/// started sending the body yet.
fn continue_100(s: &mut TcpStream, head: &Head, limits: &Limits, waiting: bool) {
    let expect =
        header(&head.headers, "expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue"));
    if waiting && expect && head.minor == 1 {
        let _ = s.set_write_timeout(Some(limits.write_timeout));
        let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
}

/// One read before `deadline`; never returns 0 (that is `Truncated`).
fn read_some(s: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> Result<usize, Error> {
    loop {
        let left = remaining(deadline).ok_or(Error::Timeout)?;
        s.set_read_timeout(Some(left)).map_err(|_| Error::Io)?;
        match s.read(buf) {
            Ok(0) => return Err(Error::Truncated),
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if timed_out(&e) => return Err(Error::Timeout),
            Err(_) => return Err(Error::Io),
        }
    }
}

/// Longest chunk-size line accepted (size, extensions and CRLF).
const MAX_CHUNK_LINE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkState {
    Size,
    Data(u64),
    DataEnd,
    Trailer,
    Done,
}

/// Incremental RFC 9112 section 7.1 decoder. `feed` consumes what it can
/// from `data` and returns how many bytes it used. Strict: hex sizes only
/// (OWS around them tolerated), CRLF or bare LF line ends, CRLF after every
/// chunk, bounded size lines and trailer section; trailers are discarded.
#[derive(Debug)]
pub struct ChunkDecoder {
    state: ChunkState,
    max_body: usize,
    max_trailer: usize,
    trailer_bytes: usize,
    pub body: Vec<u8>,
}

impl ChunkDecoder {
    pub fn new(max_body: usize, max_trailer: usize) -> ChunkDecoder {
        ChunkDecoder {
            state: ChunkState::Size,
            max_body,
            max_trailer,
            trailer_bytes: 0,
            body: Vec::new(),
        }
    }

    pub fn done(&self) -> bool {
        self.state == ChunkState::Done
    }

    /// Consume bytes from `data`; returns the count used. Stops at the end
    /// of the message (trailing bytes are left alone).
    pub fn feed(&mut self, data: &[u8]) -> Result<usize, Error> {
        let mut pos = 0;
        loop {
            let rest = &data[pos..];
            match self.state {
                ChunkState::Done => return Ok(pos),
                ChunkState::Size => {
                    let Some(nl) = rest.iter().position(|&c| c == b'\n') else {
                        if rest.len() > MAX_CHUNK_LINE {
                            return Err(Error::BadChunk);
                        }
                        return Ok(pos);
                    };
                    if nl + 1 > MAX_CHUNK_LINE {
                        return Err(Error::BadChunk);
                    }
                    let line = rest[..nl].strip_suffix(b"\r").unwrap_or(&rest[..nl]);
                    let size = line.split(|&c| c == b';').next().unwrap_or(&[]);
                    let size = trim_ows(size);
                    if size.is_empty() || size.len() > 15 || !size.iter().all(u8::is_ascii_hexdigit)
                    {
                        return Err(Error::BadChunk);
                    }
                    if line.iter().any(|&c| c < 0x20 && c != b'\t') {
                        return Err(Error::BadChunk);
                    }
                    let n = std::str::from_utf8(size)
                        .ok()
                        .and_then(|t| u64::from_str_radix(t, 16).ok())
                        .ok_or(Error::BadChunk)?;
                    if (self.body.len() as u64).saturating_add(n) > self.max_body as u64 {
                        return Err(Error::BodyTooLarge);
                    }
                    pos += nl + 1;
                    self.state = if n == 0 {
                        ChunkState::Trailer
                    } else {
                        self.body.reserve(n as usize);
                        ChunkState::Data(n)
                    };
                }
                ChunkState::Data(n) => {
                    if rest.is_empty() {
                        return Ok(pos);
                    }
                    let take = (n.min(rest.len() as u64)) as usize;
                    self.body.extend_from_slice(&rest[..take]);
                    pos += take;
                    let left = n - take as u64;
                    self.state = if left == 0 {
                        ChunkState::DataEnd
                    } else {
                        ChunkState::Data(left)
                    };
                }
                ChunkState::DataEnd => match rest {
                    [] | [b'\r'] => return Ok(pos),
                    [b'\r', b'\n', ..] => {
                        pos += 2;
                        self.state = ChunkState::Size;
                    }
                    [b'\n', ..] => {
                        pos += 1;
                        self.state = ChunkState::Size;
                    }
                    _ => return Err(Error::BadChunk),
                },
                ChunkState::Trailer => {
                    let Some(nl) = rest.iter().position(|&c| c == b'\n') else {
                        if self.trailer_bytes + rest.len() > self.max_trailer {
                            return Err(Error::HeadTooLarge);
                        }
                        return Ok(pos);
                    };
                    self.trailer_bytes += nl + 1;
                    if self.trailer_bytes > self.max_trailer {
                        return Err(Error::HeadTooLarge);
                    }
                    let line = rest[..nl].strip_suffix(b"\r").unwrap_or(&rest[..nl]);
                    pos += nl + 1;
                    if line.is_empty() {
                        self.state = ChunkState::Done;
                    } else if !line.contains(&b':') || line.iter().any(|&c| c < 0x20 && c != b'\t')
                    {
                        return Err(Error::BadChunk);
                    }
                }
            }
        }
    }
}

fn read_chunked(s: &mut TcpStream, leftover: Vec<u8>, limits: &Limits) -> Result<Vec<u8>, Error> {
    let mut dec = ChunkDecoder::new(limits.max_body_bytes, limits.max_head_bytes);
    let mut pending = leftover;
    let deadline = Instant::now() + limits.body_timeout;
    let mut chunk = [0u8; 4096];
    loop {
        let used = dec.feed(&pending)?;
        pending.drain(..used);
        if dec.done() {
            return Ok(dec.body);
        }
        let n = read_some(s, &mut chunk, deadline)?;
        pending.extend_from_slice(&chunk[..n]);
    }
}

/// After an error reply, read and discard what the client is still sending
/// for a moment so the kernel does not answer with RST before our reply is
/// read. Bounded by time and bytes.
pub fn drain(s: &mut TcpStream) {
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

/// Per-connection callback for [`serve_connections`]: it owns the socket
/// and must close it (dropping it is enough).
pub type ConnHandler = dyn Fn(TcpStream, &Limits) + Send + Sync;

/// The shared reply for a request that failed to parse or arrive in time.
pub fn error_reply(e: Error) -> Vec<u8> {
    error_bytes(e)
}

/// Serve one connection: read, handle, write, close.
pub fn handle_connection<F>(s: TcpStream, limits: &Limits, handler: &F)
where
    F: Fn(&Request) -> Response + ?Sized,
{
    handle_connection_with(s, limits, handler, &error_bytes)
}

/// Like [`handle_connection`], but `on_error` builds the bytes sent for a
/// refused request (a crate's own JSON error envelope, for example).
pub fn handle_connection_with<F, E>(mut s: TcpStream, limits: &Limits, handler: &F, on_error: &E)
where
    F: Fn(&Request) -> Response + ?Sized,
    E: Fn(Error) -> Vec<u8> + ?Sized,
{
    let _ = s.set_nodelay(true);
    let _ = s.set_write_timeout(Some(limits.write_timeout));
    match read_request(&mut s, limits) {
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
            let bytes = on_error(e);
            if !bytes.is_empty() {
                let _ = s.write_all(&bytes);
            }
            drain(&mut s);
        }
    }
}

/// Accept forever on `listener`, one thread per connection, at most
/// `limits.max_connections` at once (the rest get a 503).
pub fn serve(listener: TcpListener, limits: Limits, handler: Arc<Handler>) -> io::Result<()> {
    let conn: Arc<ConnHandler> =
        Arc::new(move |s: TcpStream, l: &Limits| handle_connection(s, l, &*handler));
    serve_connections(listener, limits, busy_bytes(), conn)
}

/// Called on the accept thread each time a connection is refused at the cap
/// (before the 503 is written). Must be cheap and never block: it runs on the
/// accept loop.
pub type BusyHook = dyn Fn() + Send + Sync;

/// The bounded accept loop under [`serve`]: one thread per connection up to
/// `limits.max_connections`; over the cap `busy` is written (200 ms write
/// timeout) and the socket closed, without spawning anything.
pub fn serve_connections(
    listener: TcpListener,
    limits: Limits,
    busy: Vec<u8>,
    conn: Arc<ConnHandler>,
) -> io::Result<()> {
    serve_connections_with(listener, limits, busy, conn, None)
}

/// [`serve_connections`] plus an optional [`BusyHook`] run on every refusal
/// at the connection cap (used to report cap hits, e.g. to the nerve).
pub fn serve_connections_with(
    listener: TcpListener,
    limits: Limits,
    busy: Vec<u8>,
    conn: Arc<ConnHandler>,
    on_busy: Option<Arc<BusyHook>>,
) -> io::Result<()> {
    let limits = Arc::new(limits);
    let live = Arc::new(AtomicUsize::new(0));
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
        if live.load(Ordering::SeqCst) >= limits.max_connections {
            if let Some(h) = &on_busy {
                h();
            }
            reject_busy(s, &busy);
            continue;
        }
        live.fetch_add(1, Ordering::SeqCst);
        let (limits2, live2, conn2) = (limits.clone(), live.clone(), conn.clone());
        let mut slot = Some(s);
        let spawned = std::thread::Builder::new()
            .stack_size(limits.thread_stack)
            .spawn(move || {
                if let Some(s) = slot.take() {
                    conn2(s, &limits2);
                }
                live2.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            live.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn busy_bytes() -> Vec<u8> {
    let body = "busy\n";
    format!(
        "HTTP/1.1 503 Service Unavailable\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nretry-after: 1\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn reject_busy(mut s: TcpStream, busy: &[u8]) {
    let _ = s.set_write_timeout(Some(Duration::from_millis(200)));
    let _ = s.write_all(busy);
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
    /// No `HTTP/x.y NNN` status line; carries the first line received
    /// (lossy UTF-8, CR/LF stripped) for diagnostics. `Display` stays
    /// `bad status line`.
    BadStatus(String),
    TooLarge,
    Invalid(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Connect(e) => write!(f, "connect: {e}"),
            ClientError::Write(e) => write!(f, "write: {e}"),
            ClientError::Read(e) => write!(f, "read: {e}"),
            ClientError::BadStatus(_) => write!(f, "bad status line"),
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
/// and the whole exchange; `max_response` caps the bytes read (more is
/// [`ClientError::TooLarge`]).
pub fn send(
    addr: SocketAddr,
    req: &ClientRequest<'_>,
    timeout: Duration,
    max_response: usize,
) -> Result<ClientResponse, ClientError> {
    exchange(addr, req, timeout, max_response, false)
}

/// Like [`send`], but reading stops quietly after `max_bytes` (head plus
/// body): the reply so far comes back with `complete == false`. For probes
/// that only want the status line and the first bytes of the body.
pub fn send_prefix(
    addr: SocketAddr,
    req: &ClientRequest<'_>,
    timeout: Duration,
    max_bytes: usize,
) -> Result<ClientResponse, ClientError> {
    exchange(addr, req, timeout, max_bytes, true)
}

fn exchange(
    addr: SocketAddr,
    req: &ClientRequest<'_>,
    timeout: Duration,
    max_response: usize,
    prefix: bool,
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
    let mut complete = true;
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
        if prefix && buf.len() >= max_response {
            complete = false;
            break;
        }
        let Some(left) = remaining(deadline) else {
            if head.is_some() {
                return finish(buf, head, false);
            }
            return Err(ClientError::Read(io::ErrorKind::TimedOut.into()));
        };
        s.set_read_timeout(Some(left)).map_err(ClientError::Read)?;
        let cap = if prefix {
            max_response - buf.len()
        } else {
            max_response + 1 - buf.len().min(max_response)
        }
        .clamp(1, chunk.len());
        match s.read(&mut chunk[..cap]) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if !prefix && buf.len() > max_response {
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
            // A head with no blank line yet (server closed early, or the
            // prefix cap): accept the status line alone, like the old
            // clients did.
            let line_end = buf.iter().position(|&c| c == b'\n').unwrap_or(buf.len());
            let mut tmp = buf[..line_end].to_vec();
            tmp.extend_from_slice(b"\r\n\r\n");
            let (st, r, h, _) = parse_response_head(&tmp).ok_or_else(|| {
                ClientError::BadStatus(
                    String::from_utf8_lossy(&buf[..line_end])
                        .trim_end_matches('\r')
                        .to_string(),
                )
            })?;
            return Ok(ClientResponse {
                status: st,
                reason: r,
                headers: h,
                body: vec![],
                complete: false,
            });
        }
    }
    finish(buf, head, complete)
}

/// (status, reason, headers, head length).
pub type ParsedHead = Option<(u16, String, Vec<(String, String)>, usize)>;

fn finish(buf: Vec<u8>, head: ParsedHead, complete: bool) -> Result<ClientResponse, ClientError> {
    let (status, reason, headers, len) =
        head.ok_or_else(|| ClientError::BadStatus(String::new()))?;
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

    #[test]
    fn limits_from_env_vars() {
        let vars = |k: &str| match k {
            "X_HTTP_HEAD_TIMEOUT_MS" => Some("30000".to_string()),
            "X_HTTP_MAX_CONNECTIONS" => Some(" 512 ".to_string()),
            "X_HTTP_MAX_BODY_BYTES" => Some("0".to_string()),
            "X_HTTP_MAX_HEADERS" => Some("lots".to_string()),
            _ => None,
        };
        let base = Limits::default();
        let l = base.clone().with_vars("X", vars);
        assert_eq!(l.head_timeout, Duration::from_secs(30));
        assert_eq!(l.max_connections, 512);
        // 0 and non-numbers are ignored; unset keys keep the default.
        assert_eq!(l.max_body_bytes, base.max_body_bytes);
        assert_eq!(l.max_headers, base.max_headers);
        assert_eq!(l.body_timeout, base.body_timeout);
        assert_eq!(l.write_timeout, base.write_timeout);
        assert_eq!(l.max_head_bytes, base.max_head_bytes);
        // Nothing set: identical.
        let same = base.clone().with_vars("X", |_| None);
        assert_eq!(format!("{same:?}"), format!("{base:?}"));
    }
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

    fn chunked_limits() -> Limits {
        Limits {
            accept_chunked: true,
            ..Limits::default()
        }
    }

    #[test]
    fn chunked_is_opt_in_and_exclusive() {
        let te = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(p(te), Err(Error::TransferEncoding));
        let l = chunked_limits();
        assert!(parse_head(te.as_bytes(), &l).unwrap().chunked);
        let both = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 3\r\n\r\n";
        assert_eq!(
            parse_head(both.as_bytes(), &l),
            Err(Error::BadContentLength)
        );
        for bad in [
            "POST / HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
            "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n",
            "POST / HTTP/1.1\r\nTransfer-Encoding: identity\r\n\r\n",
        ] {
            assert_eq!(
                parse_head(bad.as_bytes(), &l),
                Err(Error::TransferEncoding),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn chunk_decoder_whole_and_bytewise() {
        let msg = b"3\r\nabc\r\n2;ext=1\r\nde\r\n 1 \nf\n0\r\nX-T: 1\r\n\r\nNEXT";
        let mut d = ChunkDecoder::new(100, 100);
        let used = d.feed(msg).unwrap();
        assert!(d.done());
        assert_eq!(d.body, b"abcdef");
        assert_eq!(&msg[used..], b"NEXT");
        // One byte at a time gives the same result.
        let mut d = ChunkDecoder::new(100, 100);
        let mut pending = Vec::new();
        for &b in msg.iter() {
            pending.push(b);
            let used = d.feed(&pending).unwrap();
            pending.drain(..used);
            if d.done() {
                break;
            }
        }
        assert_eq!(d.body, b"abcdef");
    }

    #[test]
    fn chunk_decoder_rejects() {
        let cases: &[(&[u8], Error)] = &[
            (b"zz\r\n", Error::BadChunk),
            (b"\r\n", Error::BadChunk),
            (b"-1\r\n", Error::BadChunk),
            (b"1234567890abcdef\r\n", Error::BadChunk),
            (b"3\r\nabcX", Error::BadChunk),
            (b"0\r\nno colon\r\n\r\n", Error::BadChunk),
            (b"65\r\n", Error::BodyTooLarge),
            (b"fffffffffffffff\r\n", Error::BodyTooLarge),
        ];
        for (input, want) in cases {
            let mut d = ChunkDecoder::new(100, 64);
            assert_eq!(d.feed(input), Err(*want), "{input:?}");
        }
        let mut d = ChunkDecoder::new(100, 64);
        let long = vec![b'1'; MAX_CHUNK_LINE + 1];
        assert_eq!(d.feed(&long), Err(Error::BadChunk));
        let mut d = ChunkDecoder::new(100, 16);
        assert_eq!(
            d.feed(b"0\r\nX-Long-Trailer: aaaaaaaa\r\n"),
            Err(Error::HeadTooLarge)
        );
    }

    #[test]
    fn busy_hook_runs_on_each_cap_refusal() {
        use std::io::Read;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = std::sync::Mutex::new(release_rx);
        let (held_tx, held_rx) = mpsc::channel::<()>();
        let held_tx = std::sync::Mutex::new(held_tx);
        let conn: std::sync::Arc<super::ConnHandler> =
            std::sync::Arc::new(move |_s: std::net::TcpStream, _l: &super::Limits| {
                let _ = held_tx.lock().unwrap().send(());
                let _ = release_rx.lock().unwrap().recv();
            });
        let limits = super::Limits {
            max_connections: 1,
            ..super::Limits::default()
        };
        let on_busy: std::sync::Arc<super::BusyHook> = std::sync::Arc::new(move || {
            h2.fetch_add(1, Ordering::SeqCst);
        });
        std::thread::spawn(move || {
            super::serve_connections_with(listener, limits, b"BUSY".to_vec(), conn, Some(on_busy))
        });
        let _first = std::net::TcpStream::connect(addr).unwrap();
        held_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for n in 1..=3 {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut got = Vec::new();
            let _ = s.read_to_end(&mut got);
            assert_eq!(got, b"BUSY");
            assert_eq!(hits.load(Ordering::SeqCst), n);
        }
        let _ = release_tx.send(());
    }
}
