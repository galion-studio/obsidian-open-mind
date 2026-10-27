// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The `--listen` server: a tiny GET-only HTTP/1.1 responder.
//!
//! Routes: `/health` (`ok`), `/` and `/index.html` (the HTML shell),
//! `/api/objects` (the page JSON, computed once at start) and
//! `/api/read?path=...` (one `.md`/`.txt` file under the read roots).
//! Every other method answers 405 and every other path 404.

use crate::config::Config;
use crate::error::RoomError;
use crate::index::is_held_path;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Largest file `/api/read` returns, in bytes.
pub const MAX_READ_BYTES: u64 = 80_000;
/// Largest request head read, in bytes.
pub const MAX_REQUEST_BYTES: usize = 4096;
/// Most connections handled at once; extra ones get 503.
pub const MAX_CONNECTIONS: usize = 64;
/// Read/write timeout per connection.
pub const IO_TIMEOUT: Duration = Duration::from_secs(8);

/// A response ready to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// `Content-Type` value.
    pub content_type: &'static str,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl Response {
    fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Response {
            status,
            content_type,
            body: body.into(),
        }
    }

    /// The full HTTP/1.1 message (head plus body).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len()
        )
        .into_bytes();
        out.extend_from_slice(&self.body);
        out
    }
}

/// Standard reason phrase for the status codes room uses.
#[must_use]
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// Answers one request line (`GET /path?query HTTP/1.1`). Pure apart from
/// reading the HTML shell and `/api/read` files from disk.
#[must_use]
pub fn route(cfg: &Config, page_json: &str, request_line: &str) -> Response {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let url = parts.next().unwrap_or("/");
    if method != "GET" {
        return Response::new(405, "text/plain", "no\n");
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    match path {
        "/health" => Response::new(200, "text/plain", "ok\n"),
        "/" | "/index.html" => {
            let body = fs::read(cfg.index_html()).unwrap_or_else(|_| b"missing".to_vec());
            Response::new(200, "text/html; charset=utf-8", body)
        }
        "/api/objects" => Response::new(200, "application/json", page_json),
        "/api/read" => {
            let raw = query_param(query, "path").unwrap_or("");
            match read_doc(cfg, &decode(raw)) {
                Some(text) => Response::new(200, "text/plain; charset=utf-8", text),
                None => Response::new(404, "text/plain", "held\n"),
            }
        }
        _ => Response::new(404, "text/plain", "no\n"),
    }
}

/// The raw (still percent-encoded) value of `name` in a query string.
#[must_use]
pub fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// Returns a document's text only if every rule holds: absolute path under a
/// read root, no `..` or NUL, no secret-looking name, `.md`/`.txt`, a regular
/// file of at most [`MAX_READ_BYTES`], valid UTF-8, and its canonical path
/// (symlinks resolved) still inside the same read root.
#[must_use]
pub fn read_doc(cfg: &Config, path: &str) -> Option<String> {
    if path.contains("..") || path.contains('\0') || is_held_path(path) {
        return None;
    }
    let low = path.to_ascii_lowercase();
    if !(low.ends_with(".md") || low.ends_with(".txt")) {
        return None;
    }
    let target = Path::new(path);
    let root = cfg
        .read_roots()
        .into_iter()
        .find(|root| target.starts_with(root) && target != root.as_path())?;
    let real = fs::canonicalize(target).ok()?;
    let real_root = fs::canonicalize(&root).ok()?;
    if !real.starts_with(&real_root) || is_held_path(&real.to_string_lossy()) {
        return None;
    }
    let meta = fs::metadata(&real).ok()?;
    if !meta.is_file() || meta.len() > MAX_READ_BYTES {
        return None;
    }
    fs::read_to_string(&real).ok()
}

/// Percent-decodes a query value (`+` is a space). Invalid escapes are kept
/// literally and invalid UTF-8 becomes U+FFFD. Never panics.
#[must_use]
pub fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if let (Some(hi), Some(lo)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
                    out.push(hi << 4 | lo);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<&u8>) -> Option<u8> {
    match *b? {
        c @ b'0'..=b'9' => Some(c - b'0'),
        c @ b'a'..=b'f' => Some(c - b'a' + 10),
        c @ b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// A bound listener plus the precomputed page.
pub struct Server {
    listener: TcpListener,
    cfg: Arc<Config>,
    page: Arc<str>,
    active: Arc<AtomicUsize>,
}

impl Server {
    /// Binds `cfg.bind`. `127.0.0.1:0` picks a free port (see [`Server::local_addr`]).
    ///
    /// # Errors
    /// [`RoomError::Bind`] when the address is invalid or cannot be bound.
    pub fn bind(cfg: Config, page_json: String) -> Result<Self, RoomError> {
        let listener = TcpListener::bind(&cfg.bind).map_err(|source| RoomError::Bind {
            addr: cfg.bind.clone(),
            source,
        })?;
        Ok(Server {
            listener,
            cfg: Arc::new(cfg),
            page: Arc::from(page_json),
            active: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// The address actually bound.
    ///
    /// # Errors
    /// [`RoomError::Io`] if the OS cannot report the socket address.
    pub fn local_addr(&self) -> Result<SocketAddr, RoomError> {
        Ok(self.listener.local_addr()?)
    }

    /// Accepts connections forever, one short-lived thread per connection.
    /// Failed accepts and failed connections are skipped, never fatal.
    ///
    /// # Errors
    /// None today; the `Result` keeps room for a future shutdown path.
    pub fn run(self) -> Result<(), RoomError> {
        for stream in self.listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            if self.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                self.active.fetch_sub(1, Ordering::SeqCst);
                let busy = Response::new(503, "text/plain", "busy\n");
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let _ = stream.write_all(&busy.to_bytes());
                continue;
            }
            let cfg = Arc::clone(&self.cfg);
            let page = Arc::clone(&self.page);
            let active = Arc::clone(&self.active);
            let spawned = thread::Builder::new()
                .name("room-conn".into())
                .spawn(move || {
                    let _ = handle(stream, &cfg, &page);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            if spawned.is_err() {
                self.active.fetch_sub(1, Ordering::SeqCst);
            }
        }
        Ok(())
    }
}

/// Reads one request head from `stream` and writes the response.
///
/// # Errors
/// Any I/O error from setting timeouts, reading, or writing the stream.
pub fn handle(mut stream: TcpStream, cfg: &Config, page_json: &str) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut buf = [0u8; MAX_REQUEST_BYTES];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let line = head.lines().next().unwrap_or("");
    let t0 = Instant::now();
    let resp = if line.is_empty() {
        Response::new(400, "text/plain", "bad\n")
    } else {
        route(cfg, page_json, line)
    };
    let what = line
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    nerve_client::observe_request("room", &what, resp.status, t0.elapsed());
    stream.write_all(&resp.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Root(PathBuf);
    impl Root {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let dir = std::env::temp_dir()
                .join(format!("room-http-{tag}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).expect("root");
            Root(dir)
        }
        fn put(&self, rel: &str, body: &[u8]) -> String {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(&p, body).expect("write");
            p.to_string_lossy().into_owned()
        }
        fn cfg(&self) -> Config {
            Config {
                bind: "127.0.0.1:0".into(),
                root: self.0.clone(),
            }
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn get(cfg: &Config, url: &str) -> Response {
        route(cfg, "{\"objects\":[]}", &format!("GET {url} HTTP/1.1"))
    }

    #[test]
    fn decode_basics() {
        assert_eq!(decode("a%20b+c"), "a b c");
        assert_eq!(decode("%2Fworkspace%2fx"), "/workspace/x");
        assert_eq!(decode("%C5%BEupa"), "\u{17e}upa");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%4"), "%4");
        assert_eq!(decode("%zz"), "%zz");
        assert_eq!(decode("%FF"), "\u{fffd}");
        assert_eq!(decode(""), "");
    }

    #[test]
    fn decode_never_panics_on_multibyte_after_percent() {
        // The old decoder sliced &raw[i+1..i+3] and panicked here, which with
        // panic=abort took the whole server down.
        assert_eq!(decode("%a\u{e9}"), "%a\u{e9}");
        assert_eq!(decode("%\u{e9}"), "%\u{e9}");
        assert_eq!(decode("x%\u{1f600}"), "x%\u{1f600}");
    }

    #[test]
    fn query_param_lookup() {
        assert_eq!(query_param("path=/a.md", "path"), Some("/a.md"));
        assert_eq!(query_param("x=1&path=%2Fa", "path"), Some("%2Fa"));
        assert_eq!(query_param("pathx=1", "path"), None);
        assert_eq!(query_param("", "path"), None);
    }

    #[test]
    fn reason_phrases_and_wire_format() {
        assert_eq!(reason(200), "OK");
        assert_eq!(reason(404), "Not Found");
        assert_eq!(reason(405), "Method Not Allowed");
        assert_eq!(reason(400), "Bad Request");
        assert_eq!(reason(503), "Service Unavailable");
        assert_eq!(reason(999), "Unknown");
        let wire = Response::new(200, "text/plain", "\u{17e}\n").to_bytes();
        let text = String::from_utf8(wire).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 3\r\n"), "{text}");
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.ends_with("\r\n\r\n\u{17e}\n"));
    }

    #[test]
    fn routes() {
        let r = Root::new("routes");
        let cfg = r.cfg();
        assert_eq!(get(&cfg, "/health").body, b"ok\n");
        assert_eq!(get(&cfg, "/health?x=1").status, 200);
        let objects = get(&cfg, "/api/objects");
        assert_eq!(objects.content_type, "application/json");
        assert_eq!(objects.body, b"{\"objects\":[]}");
        assert_eq!(get(&cfg, "/").body, b"missing");
        r.put("galion-light/room/index.html", b"<h1>room</h1>");
        let index = get(&cfg, "/index.html");
        assert_eq!(index.body, b"<h1>room</h1>");
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert_eq!(get(&cfg, "/nope").status, 404);
        for method in ["POST", "HEAD", "PUT", "DELETE"] {
            let resp = route(&cfg, "", &format!("{method} /health HTTP/1.1"));
            assert_eq!(resp.status, 405, "{method}");
        }
        assert_eq!(route(&cfg, "", "").status, 405);
    }

    #[test]
    fn read_doc_serves_only_allowed_files() {
        let r = Root::new("read");
        let cfg = r.cfg();
        let ok = r.put("galion-src/app/docs/guide.md", "# Guide\n".as_bytes());
        let txt = r.put("galion-light/docs/notes.txt", b"notes");
        let rs = r.put("galion-src/app/main.rs", b"fn main(){}");
        let held = r.put("galion-src/app/secret-plan.md", b"no");
        let tok = r.put("galion-src/app/token.txt", b"no");
        let outside = r.put("other/x.md", b"no");
        let big = r.put(
            "galion-src/app/big.md",
            &vec![b'a'; MAX_READ_BYTES as usize + 1],
        );
        let edge = r.put(
            "galion-src/app/edge.md",
            &vec![b'a'; MAX_READ_BYTES as usize],
        );
        let binary = r.put("galion-src/app/bin.md", &[0xff, 0xfe, 0x00]);
        fs::create_dir_all(r.0.join("galion-src/dir.md")).unwrap();

        assert_eq!(read_doc(&cfg, &ok).as_deref(), Some("# Guide\n"));
        assert_eq!(read_doc(&cfg, &txt).as_deref(), Some("notes"));
        assert!(read_doc(&cfg, &edge).is_some());
        for bad in [&rs, &held, &tok, &outside, &big, &binary] {
            assert!(read_doc(&cfg, bad).is_none(), "{bad}");
        }
        let root = r.0.to_string_lossy();
        for bad in [
            format!("{root}/galion-src/app/docs/../../../other/x.md"),
            format!("{root}/galion-src/dir.md"),
            format!("{root}/galion-src/app/guide.md\0"),
            "galion-src/app/docs/guide.md".to_string(),
            "/etc/passwd.txt".to_string(),
            String::new(),
        ] {
            assert!(read_doc(&cfg, &bad).is_none(), "{bad:?}");
        }
        // Prefix match is by path component, not by string.
        let sibling = r.put("galion-srcX/a.md", b"no");
        assert!(read_doc(&cfg, &sibling).is_none());
    }

    #[test]
    fn read_doc_refuses_symlink_escape() {
        let r = Root::new("escape");
        let cfg = r.cfg();
        let outside = r.put("private/diary.md", b"private");
        r.put("galion-src/keep.md", b"keep");
        let link = r.0.join("galion-src/link.md");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(read_doc(&cfg, &link.to_string_lossy()).is_none());
        let inner = r.0.join("galion-src/alias.md");
        std::os::unix::fs::symlink(r.0.join("galion-src/keep.md"), &inner).unwrap();
        assert_eq!(
            read_doc(&cfg, &inner.to_string_lossy()).as_deref(),
            Some("keep")
        );
    }

    #[test]
    fn api_read_route_decodes_the_path() {
        let r = Root::new("api");
        let cfg = r.cfg();
        let p = r.put("galion-src/d/\u{17e}aba note.md", "\u{17e}".as_bytes());
        let enc: String = p
            .bytes()
            .map(|b| match b {
                b'/' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect();
        let resp = get(&cfg, &format!("/api/read?path={enc}"));
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "\u{17e}".as_bytes());
        let held = get(&cfg, "/api/read?path=%2Fetc%2Fpasswd");
        assert_eq!((held.status, held.body.as_slice()), (404, &b"held\n"[..]));
        assert_eq!(get(&cfg, "/api/read").status, 404);
        assert_eq!(get(&cfg, "/api/read?path=%a\u{e9}").status, 404);
    }

    #[test]
    fn server_over_a_real_socket() {
        let r = Root::new("sock");
        let srv = Server::bind(r.cfg(), "{\"objects\":[]}".into()).expect("bind");
        let addr = srv.local_addr().expect("addr");
        assert!(addr.ip().is_loopback());
        thread::spawn(move || srv.run());
        let ask = |req: &[u8]| {
            let mut s = TcpStream::connect(addr).expect("connect");
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            s.write_all(req).unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let health = ask(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(health.starts_with("HTTP/1.1 200 OK"), "{health}");
        assert!(health.ends_with("\r\n\r\nok\n"));
        let bad = ask(b"\r\n");
        assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
        let post = ask(b"POST /api/objects HTTP/1.1\r\n\r\n");
        assert!(post.starts_with("HTTP/1.1 405"), "{post}");
    }

    #[test]
    fn bind_error_names_the_address() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = taken.local_addr().unwrap().to_string();
        let cfg = Config {
            bind: addr.clone(),
            root: PathBuf::from("/nonexistent"),
        };
        match Server::bind(cfg, String::new()) {
            Err(RoomError::Bind { addr: a, .. }) => assert_eq!(a, addr),
            other => panic!("expected bind error, got {:?}", other.map(|_| ())),
        }
        let junk = Config {
            bind: "not an address".into(),
            root: PathBuf::from("/nonexistent"),
        };
        assert!(matches!(
            Server::bind(junk, String::new()),
            Err(RoomError::Bind { .. })
        ));
    }
}
