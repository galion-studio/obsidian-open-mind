// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--files DIR`: the file explorer. One page (`/files`, phone first), the
//! CodeMirror 6 editor bundle (`/files/editor.js`, MIT, vendored), and the
//! JSON + byte API under `/api/files/`, all reached only after the local API
//! gate passed (console host, then loopback or a paired admin cookie).
//!
//! Every path is relative to DIR and goes through [`galion_files::Root`]
//! (no `..`, no symlinks out). Read-only unless `--files-write`; writes also
//! need the `X-Galion-Files: 1` header and, when the browser sends one, an
//! `Origin` on the same host, so a cross-site form or fetch cannot write.
//!
//! Routes (query `path=` is the relative path, percent-encoded):
//! - `GET list?path=&hidden=1`, `info`, `text`, `hex?offset=&len=`,
//!   `archive` (members), `kinds` (the extension table)
//! - `GET raw?path=&dl=1`: bytes with HTTP Range (video seeking), inline
//!   or as a download; served with `Content-Security-Policy: sandbox`
//! - `GET thumb?path=&w=320`: JPEG poster via the host ffmpeg (`--files-ffmpeg`)
//! - `POST write|mkdir|rename|move|copy|trash|extract` (JSON body)
//! - `PUT upload?dir=&name=`: the raw file body, streamed to disk

use crate::http::{self, RequestHead};
use galion_files::{kinds, Error, Root};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

/// What an unpaired visitor gets at /files (401): no listing, no names.
pub const UNPAIRED_HTML: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Files</title><body style=\"background:#0b0e14;color:#e6e6e6;font:16px system-ui;padding:2rem\"><h1>Files</h1><p>This screen needs a paired console device.</p><p><a style=\"color:#8ab4ff\" href=\"/\">Open the console to pair this device</a></p></body>\n";

pub const PAGE_PATH: &str = "/files";
pub const EDITOR_PATH: &str = "/files/editor.js";
pub const API_PREFIX: &str = "/api/files/";
/// Largest JSON body (an editor save of up to 16 MiB text, JSON-escaped).
pub const MAX_JSON: usize = 40 << 20;

/// CodeMirror 6 (MIT) + the languages the table names, bundled with esbuild.
/// Rebuild: crates/console-static/assets/files/editor/README.md.
pub const EDITOR_JS: &[u8] = include_bytes!("../assets/files/editor/cm6.min.js");

pub const CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self'; frame-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
/// Raw bytes never run as a page on the console origin.
pub const RAW_CSP: &str = "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'; frame-ancestors 'self'";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesCfg {
    pub root: PathBuf,
    pub write: bool,
    pub max_upload: u64,
    pub extract_max: u64,
    pub ffmpeg: Option<PathBuf>,
    pub cache: Option<PathBuf>,
}

impl FilesCfg {
    pub fn new(root: PathBuf) -> FilesCfg {
        FilesCfg {
            root,
            write: false,
            max_upload: 2 << 30,
            extract_max: 8 << 30,
            ffmpeg: None,
            cache: None,
        }
    }
}

pub struct Files {
    pub cfg: FilesCfg,
    root: Option<Root>,
    media: Option<galion_files::media::Tools>,
}

pub fn owns_api(path: &str) -> bool {
    path.starts_with(API_PREFIX)
}

/// Percent-decode a query value (`+` is a space). Invalid UTF-8 -> `None`.
pub fn decode(v: &str) -> Option<String> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// First value of `key` in a query string.
pub fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| decode(v)).flatten()
    })
}

/// A `Range` request that cannot be served (416).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsatisfiable;

/// `Range: bytes=a-b` for a body of `size` bytes. `Ok(None)` = whole body.
pub fn parse_range(h: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, Unsatisfiable> {
    let Some(h) = h else { return Ok(None) };
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    if spec.contains(',') {
        return Ok(None); // multi-range: send it all
    }
    let (a, b) = spec.split_once('-').ok_or(Unsatisfiable)?;
    let (a, b) = (a.trim(), b.trim());
    if size == 0 {
        return Err(Unsatisfiable);
    }
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| Unsatisfiable)?;
        if n == 0 {
            return Err(Unsatisfiable);
        }
        (size.saturating_sub(n), size - 1)
    } else {
        let s: u64 = a.parse().map_err(|_| Unsatisfiable)?;
        let e: u64 = if b.is_empty() {
            size - 1
        } else {
            b.parse().map_err(|_| Unsatisfiable)?
        };
        (s, e.min(size - 1))
    };
    if start > end || start >= size {
        return Err(Unsatisfiable);
    }
    Ok(Some((start, end)))
}

/// `attachment; filename="a.mp4"; filename*=UTF-8''a.mp4`
pub fn disposition(kind: &str, name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{enc}")
}

/// One answer: status, content type, extra headers, body.
pub struct Out {
    pub status: u16,
    pub kind: &'static str,
    pub body: Vec<u8>,
}

fn ok(v: Value) -> Out {
    Out {
        status: 200,
        kind: "application/json",
        body: v.to_string().into_bytes(),
    }
}

pub fn fail(status: u16, msg: &str) -> Out {
    Out {
        status,
        kind: "application/json",
        body: json!({"ok": false, "error": msg}).to_string().into_bytes(),
    }
}

fn from(r: galion_files::Result<Value>) -> Out {
    match r {
        Ok(v) => ok(v),
        Err(e) => fail(e.status(), &e.message()),
    }
}

/// Whether a write request carries the explicit header and a same-host origin.
pub fn write_allowed(head: &RequestHead) -> Result<(), &'static str> {
    if head.header("x-galion-files") != Some("1") {
        return Err("missing X-Galion-Files: 1");
    }
    if let Some(o) = head.header("origin") {
        let host = head.header("host").unwrap_or("");
        let oh = o
            .strip_prefix("https://")
            .or_else(|| o.strip_prefix("http://"))
            .unwrap_or("");
        if o == "null" || !oh.eq_ignore_ascii_case(host) {
            return Err("cross-origin write refused");
        }
    }
    Ok(())
}

/// Reads `carry` then the stream, exactly `left` bytes.
struct Body<'a> {
    carry: Vec<u8>,
    pos: usize,
    stream: &'a mut TcpStream,
    left: u64,
}

impl Read for Body<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 || buf.is_empty() {
            return Ok(0);
        }
        let cap = (buf.len() as u64).min(self.left) as usize;
        let n = if self.pos < self.carry.len() {
            let k = cap.min(self.carry.len() - self.pos);
            buf[..k].copy_from_slice(&self.carry[self.pos..self.pos + k]);
            self.pos += k;
            k
        } else {
            let k = self.stream.read(&mut buf[..cap])?;
            if k == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body cut off"));
            }
            k
        };
        self.left -= n as u64;
        Ok(n)
    }
}

fn content_length(head: &RequestHead) -> Result<u64, Out> {
    if head.header("transfer-encoding").is_some() {
        return Err(fail(411, "send Content-Length (no chunked bodies)"));
    }
    match head.header("content-length") {
        None => Ok(0),
        Some(v) => v
            .trim()
            .parse()
            .map_err(|_| fail(400, "bad Content-Length")),
    }
}

fn json_body(stream: &mut TcpStream, head: &RequestHead, carry: Vec<u8>) -> Result<Value, Out> {
    let n = content_length(head)?;
    if n > MAX_JSON as u64 {
        return Err(fail(413, "body too large"));
    }
    let mut b = Vec::with_capacity(n as usize);
    Body {
        carry,
        pos: 0,
        stream,
        left: n,
    }
    .read_to_end(&mut b)
    .map_err(|_| fail(400, "body cut off"))?;
    serde_json::from_slice(&b).map_err(|_| fail(400, "body must be JSON"))
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

impl Files {
    pub fn new(cfg: FilesCfg) -> Files {
        let root = match Root::new(&cfg.root, cfg.write) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("console-static: --files folder unusable: {}", e.kind());
                None
            }
        };
        let media = cfg.ffmpeg.clone().map(galion_files::media::Tools::new);
        Files { cfg, root, media }
    }

    /// Handle one `/api/files/*` request; returns the status for the log.
    /// The connection is always closed afterwards.
    pub fn handle(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        method: &str,
        path: &str,
        query: &str,
        carry: Vec<u8>,
    ) -> u16 {
        let head_only = method == "HEAD";
        let route = &path[API_PREFIX.len()..];
        let Some(root) = self.root.as_ref() else {
            return send(
                stream,
                fail(503, "the shared folder is not available"),
                head_only,
            );
        };
        let q = |k: &str| param(query, k).unwrap_or_default();
        let get = method == "GET" || head_only;
        let out = match (route, get) {
            ("list", true) => from(root.list(&q("path"), q("hidden") == "1")),
            ("info", true) => {
                let mut r = root.info(&q("path"));
                if let (Ok(v), Some(t)) = (r.as_mut(), self.media.as_ref()) {
                    v["posters"] = json!(true);
                    if matches!(v["kind"].as_str(), Some("video" | "audio")) {
                        if let Ok(abs) = root.existing(&q("path")) {
                            v["media"] = galion_files::media::probe(t, &abs).unwrap_or(Value::Null);
                        }
                    }
                }
                from(r)
            }
            ("text", true) => from(root.read_text(&q("path"), galion_files::TEXT_MAX)),
            ("hex", true) => from(root.hex(
                &q("path"),
                q("offset").parse().unwrap_or(0),
                q("len").parse().unwrap_or(4096),
            )),
            ("archive", true) => from(root.archive_list(&q("path"))),
            ("kinds", true) => ok(kinds::table_json()),
            ("raw", true) => {
                return self.raw(stream, head, root, &q("path"), q("dl") == "1", head_only)
            }
            ("thumb", true) => self.thumb(root, &q("path"), q("w").parse().unwrap_or(320)),
            ("upload", false) if method == "PUT" => self.upload(stream, head, root, query, carry),
            ("write" | "mkdir" | "rename" | "move" | "copy" | "trash" | "extract", false)
                if method == "POST" =>
            {
                self.write_op(stream, head, root, route, carry)
            }
            ("list" | "info" | "text" | "hex" | "archive" | "kinds" | "raw" | "thumb", false) => {
                fail(405, "use GET")
            }
            ("upload", _) => fail(405, "use PUT"),
            ("write" | "mkdir" | "rename" | "move" | "copy" | "trash" | "extract", true) => {
                fail(405, "use POST")
            }
            _ => fail(404, "unknown files route"),
        };
        send(stream, out, head_only)
    }

    fn guard(&self, head: &RequestHead) -> Option<Out> {
        if !self.cfg.write {
            return Some(fail(403, &Error::ReadOnly.message()));
        }
        write_allowed(head).err().map(|m| fail(403, m))
    }

    fn write_op(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        root: &Root,
        route: &str,
        carry: Vec<u8>,
    ) -> Out {
        if let Some(o) = self.guard(head) {
            return o;
        }
        let b = match json_body(stream, head, carry) {
            Ok(b) => b,
            Err(o) => return o,
        };
        let p = s(&b, "path");
        from(match route {
            "write" => root.write_text(
                p,
                s(&b, "content"),
                b.get("version").and_then(Value::as_str),
                b.get("create").and_then(Value::as_bool).unwrap_or(false),
            ),
            "mkdir" => root.mkdir(p, s(&b, "name")),
            "rename" => root.rename(p, s(&b, "name")),
            "move" => root.move_to(p, s(&b, "dest")),
            "copy" => root.copy_to(p, s(&b, "dest")),
            "trash" => root.trash(p),
            _ => root.extract(
                p,
                b.get("dest").and_then(Value::as_str),
                self.cfg.extract_max,
            ),
        })
    }

    fn upload(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        root: &Root,
        query: &str,
        carry: Vec<u8>,
    ) -> Out {
        if let Some(o) = self.guard(head) {
            return o;
        }
        let n = match content_length(head) {
            Ok(n) => n,
            Err(o) => return o,
        };
        if n > self.cfg.max_upload {
            return fail(
                413,
                &format!("upload larger than {} bytes", self.cfg.max_upload),
            );
        }
        let dir = param(query, "dir").unwrap_or_default();
        let name = param(query, "name").unwrap_or_default();
        // Uploads can take a while on a phone link.
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(120)));
        let body = Body {
            carry,
            pos: 0,
            stream,
            left: n,
        };
        from(root.upload(&dir, &name, body, self.cfg.max_upload))
    }

    fn thumb(&self, root: &Root, rel: &str, w: u32) -> Out {
        let Some(t) = self.media.as_ref() else {
            return fail(404, "no posters: --files-ffmpeg is not set");
        };
        let abs = match root.existing(rel) {
            Ok(a) => a,
            Err(e) => return fail(e.status(), &e.message()),
        };
        let meta = match std::fs::metadata(&abs) {
            Ok(m) if m.is_file() => m,
            _ => return fail(404, "not a file"),
        };
        let key = crate::sha256::hex(
            format!(
                "{}|{}|{:?}|{w}",
                abs.display(),
                meta.len(),
                meta.modified().ok()
            )
            .as_bytes(),
        );
        let cached = self
            .cfg
            .cache
            .as_ref()
            .map(|c| c.join(format!("{key}.jpg")));
        if let Some(Ok(b)) = cached.as_ref().map(std::fs::read) {
            return Out {
                status: 200,
                kind: "image/jpeg",
                body: b,
            };
        }
        match galion_files::media::poster(t, &abs, w, 1.0) {
            Ok(b) => {
                if let Some(c) = cached {
                    if let Some(d) = c.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = std::fs::write(&c, &b);
                }
                Out {
                    status: 200,
                    kind: "image/jpeg",
                    body: b,
                }
            }
            Err(e) => fail(e.status(), &e.message()),
        }
    }

    fn raw(
        &self,
        stream: &mut TcpStream,
        head: &RequestHead,
        root: &Root,
        rel: &str,
        dl: bool,
        head_only: bool,
    ) -> u16 {
        let (mut f, size, t, name) = match root.open(rel) {
            Ok(x) => x,
            Err(e) => return send(stream, fail(e.status(), &e.message()), head_only),
        };
        // Text kinds (html, svg source, scripts) go out as text/plain.
        let mime = if t.kind.is_text() {
            "text/plain; charset=utf-8"
        } else {
            t.mime
        };
        let mime = if dl { "application/octet-stream" } else { mime };
        let disp = disposition(if dl { "attachment" } else { "inline" }, &name);
        let csp = if t.kind == kinds::Kind::Pdf && !dl {
            "default-src 'none'; frame-ancestors 'self'"
        } else {
            RAW_CSP
        };
        let mut extra = format!(
            "Content-Type: {mime}\r\nContent-Disposition: {disp}\r\nAccept-Ranges: bytes\r\nCache-Control: private, no-cache\r\nContent-Security-Policy: {csp}\r\nCross-Origin-Resource-Policy: same-origin\r\n"
        );
        let (code, start, len) = match parse_range(head.header("range"), size) {
            Ok(None) => (200, 0, size),
            Ok(Some((a, b))) => {
                extra.push_str(&format!("Content-Range: bytes {a}-{b}/{size}\r\n"));
                (206, a, b - a + 1)
            }
            Err(Unsatisfiable) => {
                let e = format!("Content-Range: bytes */{size}\r\nContent-Type: text/plain\r\n");
                let _ = http::write_head(stream, 416, &e, 0, false);
                return 416;
            }
        };
        if http::write_head(stream, code, &extra, len, false).is_err() || head_only {
            return code;
        }
        use std::io::Seek;
        if f.seek(io::SeekFrom::Start(start)).is_err() {
            return code;
        }
        let mut buf = vec![0u8; 64 << 10];
        let mut left = len;
        while left > 0 {
            let k = (buf.len() as u64).min(left) as usize;
            match f.read(&mut buf[..k]) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if stream.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    left -= n as u64;
                }
            }
        }
        let _ = stream.flush();
        code
    }
}

fn send(stream: &mut TcpStream, o: Out, head_only: bool) -> u16 {
    let extra = format!("Content-Type: {}\r\nCache-Control: no-store\r\n", o.kind);
    let _ = http::write_head(stream, o.status, &extra, o.body.len() as u64, false)
        .and_then(|_| {
            if head_only {
                Ok(())
            } else {
                stream.write_all(&o.body)
            }
        })
        .and_then(|_| stream.flush());
    o.status
}

/// CSP for `/files` inside the console shell: [`CSP`] plus the console's own
/// stylesheet and icon sprite (`'self'`).
pub const SHELL_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; media-src 'self'; frame-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Page-level rules only the standalone page needs (inside the console the
/// shell stylesheet owns `html`/`body`).
const STANDALONE_CSS: &str = "html,body{margin:0;background:#05060f;color:#e8eaff;font:15px/1.4 system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif;-webkit-text-size-adjust:100%}\n.fx{padding:max(.6rem,env(safe-area-inset-top)) .7rem 1rem;min-height:100vh}\n@media (min-width:860px){.fx{height:100vh}}\n";

fn tail_into(s: &mut String) {
    s.push_str("<script src=\"/files/editor.js?v=");
    s.push_str(&crate::sha256::hex(EDITOR_JS)[..12]);
    s.push_str("\" defer></script>\n<script>\n");
    s.push_str(include_str!("../assets/files/files.js"));
    s.push_str("</script>\n");
}

/// The standalone `/files` page (used only when the Rust console UI is off).
pub fn page() -> &'static str {
    static PAGE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PAGE.get_or_init(|| {
        let mut s = String::with_capacity(64 << 10);
        s.push_str(include_str!("../assets/files/head.html"));
        s.push_str("<style>\n");
        s.push_str(STANDALONE_CSS);
        s.push_str(include_str!("../assets/files/files.css"));
        s.push_str("</style>\n</head>\n<body>\n");
        s.push_str(include_str!("../assets/files/body.html"));
        tail_into(&mut s);
        s.push_str("</body>\n</html>\n");
        s
    })
}

/// `/files` drawn inside the console shell (same header, menu and dock as
/// every console room): extra `<head>` markup, the main area, and the
/// end-of-body scripts.
pub fn shell_parts() -> (&'static str, &'static str, &'static str) {
    static P: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
    let (head, tail) = P.get_or_init(|| {
        let head = format!(
            "<meta name=\"referrer\" content=\"no-referrer\"><style>\n{}</style>",
            include_str!("../assets/files/files.css")
        );
        let mut tail = String::with_capacity(32 << 10);
        tail_into(&mut tail);
        (head, tail)
    });
    (head, include_str!("../assets/files/body.html"), tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_decoding() {
        assert_eq!(
            param("path=ai%20video%20Instagram.rar&x=1", "path").unwrap(),
            "ai video Instagram.rar"
        );
        assert_eq!(param("a=1&path=a+b%2Fc", "path").unwrap(), "a b/c");
        assert_eq!(param("path=%2e%2e%2fetc", "path").unwrap(), "../etc");
        assert!(param("path=%zz", "path").is_none());
        assert!(param("path=%ff", "path").is_none());
        assert!(param("x=1", "path").is_none());
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=0-"), 100), Ok(Some((0, 99))));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Ok(Some((10, 19))));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Err(Unsatisfiable));
        assert_eq!(parse_range(Some("bytes=5-1"), 100), Err(Unsatisfiable));
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Ok(None));
    }

    #[test]
    fn dispositions() {
        assert_eq!(
            disposition("attachment", "ai video Instagram.rar"),
            "attachment; filename=\"ai video Instagram.rar\"; filename*=UTF-8''ai%20video%20Instagram.rar"
        );
        assert!(disposition("inline", "a\"b\r\n.txt").starts_with("inline; filename=\"a_b__.txt\""));
    }

    #[test]
    fn page_has_editor_and_no_external_urls() {
        let p = page();
        assert!(p.contains("/files/editor.js?v="));
        assert!(!p.contains("http://") && !p.contains("https://"));
        assert!(EDITOR_JS.len() > 100_000);
        let (head, main, tail) = shell_parts();
        assert!(head.contains("<style>") && !head.contains("html,body"));
        assert!(main.contains("id=\"fx-list\"") && !main.contains("<body"));
        assert!(tail.contains("/files/editor.js?v=") && !tail.contains("</body>"));
        assert!(!p.contains("href=\"/phone\""), "no separate-site home link");
    }
}
