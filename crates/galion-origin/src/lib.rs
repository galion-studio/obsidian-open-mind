// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-origin: what origin.py actually served, minus what is retired.
//!
//! origin.py proxied every path not under /desk or /grinder to the console
//! on 8080, so its JSON catalog routes were unreachable. Reachable then, and
//! kept here: `/grinder[/..]` (static, STL as download), `/desk` (desk-admin
//! page), `/desk?n=<note>` (build-session notes viewer), `/desk/live`
//! (state/link.json), `/soon` and the host splash. New: `/health`.
//! Retired: the admin proxy (replaced by an optional 302 to
//! `ORIGIN_CONSOLE_URL`), spawning persist-boot.sh, binding 0.0.0.0 on eight
//! ports, the hard-coded RunPod pod id and the public-IP lookup.

use galion_common::http;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Config {
    pub addr: String,
    /// galion-light root: desk-admin.html, docs/build-session, state/link.json.
    pub root: PathBuf,
    pub grinder_root: PathBuf,
    /// Where non-origin paths go (302). None = splash.
    pub console_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Config {
        let v = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
        Config {
            addr: v("ORIGIN_ADDR").unwrap_or_else(|| "127.0.0.1:8655".into()),
            root: v("ORIGIN_ROOT").map_or_else(|| PathBuf::from("."), PathBuf::from),
            grinder_root: v("ORIGIN_GRINDER_ROOT")
                .map_or_else(|| PathBuf::from("grinder"), PathBuf::from),
            console_url: v("ORIGIN_CONSOLE_URL"),
        }
    }
}

pub struct Reply {
    pub code: u16,
    pub ctype: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    fn new(code: u16, ctype: &str, body: impl Into<Vec<u8>>) -> Reply {
        Reply {
            code,
            ctype: ctype.into(),
            headers: vec![],
            body: body.into(),
        }
    }
    fn json(code: u16, body: String) -> Reply {
        let mut r = Reply::new(code, "application/json", body);
        r.headers.push(("cache-control".into(), "no-store".into()));
        r
    }
    fn html(body: String) -> Reply {
        Reply::new(200, "text/html; charset=utf-8", body)
    }
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
                {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| percent_decode(v))
    })
}

pub fn splash(host: &str) -> String {
    let h = if host.is_empty() {
        "galion".to_string()
    } else {
        escape(host)
    };
    format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>Galion</title><style>body{{margin:0;background:#08090b;color:#e8eaee;font:16px/1.5 system-ui,sans-serif;min-height:100vh}}\
main{{max-width:42rem;margin:0 auto;padding:4.2rem 1.25rem}}h1{{font-weight:500;letter-spacing:-.04em;font-size:2.75rem;margin:.5rem 0 1rem}}\
p{{color:#8b919c}}.k{{font:11px/1 ui-monospace,monospace;letter-spacing:.18em;text-transform:uppercase;color:#5c6370}}</style>\
<body><main><p class=\"k\">{h} · Obsidian Open Mind, made by Galion</p><h1>Soon online.</h1>\
<p>The origin is live. This host is not serving a site right now.</p></main></body></html>"
    )
}

fn mime(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "stl" => "model/stl",
        _ => "application/octet-stream",
    }
}

/// Resolve `rel` under `root`; None when it escapes or is not a file.
pub fn safe_file(root: &Path, rel: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let dest = root.join(rel.trim_start_matches('/')).canonicalize().ok()?;
    (dest.starts_with(&root) && dest.is_file()).then_some(dest)
}

fn grinder(cfg: &Config, path: &str) -> Reply {
    let rel = path
        .strip_prefix("/grinder")
        .unwrap_or("")
        .trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    match safe_file(&cfg.grinder_root, rel).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
        Some((p, body)) => {
            let mut r = Reply::new(200, mime(&p), body);
            r.headers.push(("cache-control".into(), "no-store".into()));
            if r.ctype == "model/stl" {
                r.headers.push((
                    "content-disposition".into(),
                    "attachment; filename=\"grinder-50mm.stl\"".into(),
                ));
            }
            r
        }
        None => Reply::new(404, "text/plain; charset=utf-8", "Not found"),
    }
}

fn notes(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                if let Ok(r) = p.strip_prefix(base) {
                    out.push(r.to_string_lossy().into_owned());
                }
            }
        }
    }
    let mut v = Vec::new();
    walk(root, root, &mut v);
    v.sort();
    v
}

/// origin.py `desk_page()`.
pub fn desk_page(cfg: &Config, name: &str) -> String {
    let root = cfg.root.join("docs/build-session");
    let list = notes(&root);
    let article = if name.is_empty() {
        "<p class=\"mut\">Pick a note.</p>".to_string()
    } else if !name.ends_with(".md") {
        "<p>Missing.</p>".to_string()
    } else {
        match safe_file(&root, name) {
            Some(p) => {
                let t = std::fs::read_to_string(p).unwrap_or_default();
                let t: String = t.chars().take(80000).collect();
                format!(
                    "<article><p class=\"k\">{}</p><pre>{}</pre></article>",
                    escape(name),
                    escape(&t)
                )
            }
            None => "<p>Missing.</p>".to_string(),
        }
    };
    let links: String = list
        .iter()
        .map(|n| format!("<a href=\"/desk?n={0}\">{0}</a>", escape(n)))
        .collect();
    format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>Obsidian</title><style>body{{margin:0;background:#0c0d0f;color:#e8eaee;font:16px/1.5 system-ui,sans-serif}}\
header{{display:flex;justify-content:space-between;align-items:baseline;padding:1rem 1.25rem;border-bottom:1px solid #23262d}}\
main{{display:grid;grid-template-columns:16rem 1fr;min-height:calc(100vh - 3.5rem)}}nav{{border-right:1px solid #23262d;padding:1rem;overflow:auto}}\
nav a{{display:block;color:#8b919c;text-decoration:none;padding:.35rem 0;font-size:.85rem}}nav a:hover{{color:#e8eaee}}\
section{{padding:1.5rem 1.5rem 3rem;max-width:48rem}}pre{{white-space:pre-wrap;font:14px/1.55 ui-monospace,monospace;color:#c8ccd4}}\
.k{{font:11px/1 ui-monospace,monospace;letter-spacing:.18em;text-transform:uppercase;color:#6b7280}}.mut{{color:#8b919c}}\
@media(max-width:800px){{main{{grid-template-columns:1fr}}nav{{border-right:0;border-bottom:1px solid #23262d;max-height:40vh}}}}\
</style><body><header><strong>Obsidian</strong><span class=\"k\">Galion desk</span></header><main><nav>{links}</nav><section>{article}</section></main></body></html>"
    )
}

const EMPTY_LINK: &str = "{\"at\":\"\",\"laptop_port\":false,\"model\":\"\",\"model_ms\":0,\"hermes\":false,\"hermes_ms\":0,\"france\":false,\"laptop\":{}}";

/// Route one request. `started` gives uptime.
pub fn route(cfg: &Config, method: &str, target: &str, host: &str, started: Instant) -> Reply {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    if method == "OPTIONS" {
        let mut r = Reply::new(204, "text/plain", "");
        r.headers
            .push(("access-control-allow-origin".into(), "*".into()));
        r.headers
            .push(("access-control-allow-methods".into(), "GET,OPTIONS".into()));
        return r;
    }
    if method != "GET" && method != "HEAD" {
        return Reply::json(405, "{\"error\":\"method not allowed\"}".into());
    }
    if path == "/health" {
        return Reply::json(
            200,
            format!(
            "{{\"status\":\"healthy\",\"service\":\"galion-origin\",\"version\":{},\"uptime\":{}}}",
            json_str(VERSION),
            started.elapsed().as_secs()
        ),
        );
    }
    if path == "/grinder" || path.starts_with("/grinder/") {
        return grinder(cfg, path);
    }
    if path == "/desk/live" {
        let body = std::fs::read_to_string(cfg.root.join("state/link.json"))
            .ok()
            .filter(|t| t.trim_start().starts_with('{'))
            .unwrap_or_else(|| EMPTY_LINK.into());
        return Reply::json(200, body);
    }
    if path == "/desk" || path.starts_with("/desk/") {
        if query.contains("notes=1") || query_param(query, "n").is_some() {
            return Reply::html(desk_page(cfg, &query_param(query, "n").unwrap_or_default()));
        }
        return match std::fs::read_to_string(cfg.root.join("desk-admin.html")) {
            Ok(t) => Reply::html(t),
            Err(_) => Reply::html(desk_page(cfg, "")),
        };
    }
    if path == "/soon" {
        return Reply::html(splash(&host));
    }
    if let Some(url) = &cfg.console_url {
        let mut r = Reply::new(302, "text/plain", "");
        r.headers.push((
            "location".into(),
            format!("{}{}", url.trim_end_matches('/'), target),
        ));
        return r;
    }
    Reply::html(splash(&host))
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    }
}

/// Connection limits for the origin, on the shared galion-common server:
/// 8 KiB head, 10 s for the whole head (was 10 s per read), 30 s writes,
/// 32 live connections, 64 KiB thread stacks. Bodies are never used (GET,
/// HEAD and OPTIONS only), so they are capped at 64 KiB.
pub fn limits() -> http::Limits {
    http::Limits {
        max_head_bytes: 8192,
        max_body_bytes: 64 * 1024,
        head_timeout: Duration::from_secs(10),
        body_timeout: Duration::from_secs(10),
        write_timeout: Duration::from_secs(30),
        max_connections: 32,
        thread_stack: 64 * 1024,
        ..http::Limits::default()
    }
}

/// The reply bytes for one parsed request (HTTP/1.0 style, closes after the
/// reply). Byte layout unchanged from the hand-rolled server.
pub fn render(cfg: &Config, req: &http::Request, started: Instant) -> Vec<u8> {
    let method = req.method();
    let host = req.header("host").unwrap_or("");
    let r = route(cfg, method, req.target(), host, started);
    let mut out = format!(
        "HTTP/1.0 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\n",
        r.code,
        reason(r.code),
        r.ctype,
        r.body.len()
    );
    for (k, v) in &r.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("connection: close\r\n\r\n");
    let mut out = out.into_bytes();
    if method != "HEAD" {
        out.extend_from_slice(&r.body);
    }
    out
}

/// Serve one connection with the shared hardened parser.
pub fn handle(cfg: &Config, s: TcpStream, started: Instant) {
    http::handle_connection(s, &limits(), &|req: &http::Request| {
        http::Response::raw(render(cfg, req, started))
    });
}
