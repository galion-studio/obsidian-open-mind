// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Connection handling: static files, SPA shell, redirects, proxy, 404.
//!
//! Decision order for a request target:
//! 1. sanitize the path ([`crate::path`]); refused -> 404;
//! 2. an allowlisted proxy prefix with an upstream configured -> proxy;
//! 3. a redirect -> 302; a built file -> the file; a page route -> the shell;
//! 4. everything else -> 404 (also for other methods, so probing with POST or
//!    OPTIONS learns nothing). Known resources answer 405 to non GET/HEAD.

use crate::config::Config;
use crate::http::{self, HeadError, RequestHead};
use crate::path;
use crate::proxy;
use crate::routes::Routes;
use crate::site::{Entry, Site};
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Idle time allowed between requests on a kept-alive connection, and for a
/// slow client to finish sending a head.
pub const IDLE: Duration = Duration::from_secs(15);
/// Requests served on one kept-alive connection before it is closed.
pub const MAX_REQUESTS_PER_CONN: usize = 100;

pub struct State {
    pub site: Site,
    pub routes: Routes,
    pub cfg: Config,
    /// Rust-rendered pages (`--ui rust`).
    pub ui: Option<console_ui::Ui>,
    /// Board catalog + marker ring (`--local-api`).
    pub board: crate::board::Board,
    /// Admin-device store (`--admin-store`).
    pub admin: Option<crate::admin::AdminStore>,
    active: AtomicUsize,
}

impl State {
    pub fn new(site: Site, routes: Routes, cfg: Config) -> State {
        let ui = match (cfg.rust_ui, cfg.upstream) {
            (true, Some(upstream)) => Some(console_ui::Ui::new(console_ui::UiConfig {
                upstream,
                pulse: cfg.pulse,
                extra_hosts: cfg.extra_hosts.clone(),
                desk: cfg.desk,
                files: cfg.files.clone(),
            })),
            _ => None,
        };
        let admin = cfg
            .admin_store
            .clone()
            .map(|p| crate::admin::AdminStore::new(p, cfg.admin_mirror.clone()));
        State {
            site,
            routes,
            cfg,
            ui,
            board: crate::board::Board::new(),
            admin,
            active: AtomicUsize::new(0),
        }
    }
}

/// Accept loop. Returns only on a listener error.
pub fn serve(listener: TcpListener, state: Arc<State>) -> io::Result<()> {
    for conn in listener.incoming() {
        let mut stream = match conn {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => continue,
            Err(e) => {
                // EMFILE and friends: back off instead of spinning.
                eprintln!("console-static: accept: {e}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if state.active.fetch_add(1, Ordering::SeqCst) >= state.cfg.max_conns {
            state.active.fetch_sub(1, Ordering::SeqCst);
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = http::write_simple(
                &mut stream,
                503,
                "Retry-After: 1\r\n",
                "Service Unavailable\n",
                false,
                false,
            );
            continue;
        }
        let st = Arc::clone(&state);
        let spawned = thread::Builder::new()
            .name("conn".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                handle_connection(stream, &st);
                st.active.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            state.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
    Ok(())
}

fn log(state: &State, method: &str, shown: &str, code: u16) {
    if state.cfg.access_log {
        // Path only, never the query string (it can carry tokens).
        eprintln!("{method} {shown} {code}");
    }
}

/// Serve every request on one connection.
pub fn handle_connection(mut stream: TcpStream, state: &State) {
    let _ = stream.set_read_timeout(Some(IDLE));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_nodelay(true);
    let mut carry = Vec::new();
    for _ in 0..MAX_REQUESTS_PER_CONN {
        let head = match http::read_head(&mut stream, &mut carry) {
            Ok(h) => h,
            Err(HeadError::Closed) | Err(HeadError::Io) => return,
            Err(HeadError::TooLarge) => {
                let _ = http::write_simple(
                    &mut stream,
                    431,
                    "",
                    "Request Header Fields Too Large\n",
                    false,
                    false,
                );
                return;
            }
            Err(HeadError::Malformed) => {
                let _ = http::write_simple(&mut stream, 400, "", "Bad Request\n", false, false);
                return;
            }
        };
        let keep = handle_request(
            &mut stream,
            &head,
            std::mem::take(&mut carry),
            state,
            &mut carry,
        );
        if !keep {
            let _ = stream.shutdown(std::net::Shutdown::Write);
            return;
        }
    }
}

/// Handle one request; returns whether the connection may be reused. When it
/// may, `next_carry` receives any pipelined bytes.
fn handle_request(
    stream: &mut TcpStream,
    head: &RequestHead,
    carry: Vec<u8>,
    state: &State,
    next_carry: &mut Vec<u8>,
) -> bool {
    let (raw_path, query) = path::split_target(&head.target);
    let method = head.method.as_str();
    let head_only = method == "HEAD";
    let has_body = head.header("transfer-encoding").is_some()
        || head
            .header("content-length")
            .map(|v| v.trim() != "0")
            .unwrap_or(false);
    // A body we will not read means the connection cannot be reused.
    let mut keep = head.wants_keep_alive() && !has_body;

    let clean = match path::sanitize(raw_path) {
        Ok(p) => p,
        Err(why) => {
            log(state, method, &format!("<refused:{why:?}>"), 404);
            let _ = http::write_simple(stream, 404, "", "Not Found\n", head_only, keep);
            if keep {
                *next_carry = carry;
            }
            return keep;
        }
    };

    let mut carry = carry;
    if state.cfg.local_api {
        let peer = stream.peer_addr().ok();
        let peer_loopback = peer.is_some_and(|p| p.ip().is_loopback());
        let mut api_body = Vec::new();
        if crate::api::wants_body(method, &clean) {
            match read_small_body(stream, head, std::mem::take(&mut carry)) {
                Ok(b) => api_body = b,
                Err(code) => {
                    let text = match code {
                        411 => "Length Required\n",
                        413 => "Content Too Large\n",
                        _ => "Bad Request\n",
                    };
                    let _ = http::write_simple(stream, code, "", text, head_only, false);
                    log(state, method, &clean, code);
                    return false;
                }
            }
            // Body consumed here; do not reuse the connection.
            keep = false;
        }
        let req = crate::api::ApiRequest {
            method,
            path: &clean,
            head,
            peer_loopback,
            body: &api_body,
        };
        let ctx = crate::api::ApiCtx {
            board: &state.board,
            admin: state.admin.as_ref(),
            extra_hosts: &state.cfg.extra_hosts,
            arena: state.cfg.arena_root.as_deref(),
        };
        if let Some(reply) = crate::api::try_handle(&req, &ctx) {
            let mut extra = format!(
                "Content-Type: {}\r\nCache-Control: no-store\r\n",
                reply.content_type
            );
            if let Some(c) = &reply.set_cookie {
                let c: String = c.chars().filter(|ch| *ch != '\r' && *ch != '\n').collect();
                extra.push_str(&format!("Set-Cookie: {c}\r\n"));
            }
            let ok = http::write_head(stream, reply.status, &extra, reply.body.len() as u64, keep)
                .and_then(|_| {
                    if head_only {
                        Ok(())
                    } else {
                        stream.write_all(reply.body.as_bytes())
                    }
                })
                .and_then(|_| stream.flush());
            log(state, method, &clean, reply.status);
            if keep && ok.is_ok() {
                *next_carry = carry;
                return true;
            }
            return false;
        }
    }

    // Data routes the local API did not answer (it already ran the gate:
    // anything that reaches here passed host + loopback/admin checks).
    if let Some(up) = state.cfg.data_upstream {
        let p = clean.split('?').next().unwrap_or(&clean);
        if state.cfg.local_api && state.cfg.data_paths.iter().any(|d| d == p) {
            if method != "GET" && method != "HEAD" {
                let body = crate::arena::HELD;
                let _ = http::write_head(
                    stream,
                    405,
                    "Content-Type: application/json\r\nAllow: GET, HEAD\r\nCache-Control: no-store\r\n",
                    body.len() as u64,
                    false,
                )
                .and_then(|_| {
                    if head_only {
                        Ok(())
                    } else {
                        stream.write_all(body.as_bytes())
                    }
                });
                log(state, method, &clean, 405);
                return false;
            }
            let code = proxy::forward(stream, head, carry, up, 0);
            log(state, method, &clean, code);
            return false;
        }
    }

    // `--owner-routes` (slot 395): Node-era rows owned by another Rust
    // service, and explicit 410 for retired rows. Reached only after the
    // local API gate passed (it answers 401 itself otherwise).
    if state.cfg.local_api && state.cfg.owner_routes {
        if let Some(r) = crate::owners::owner_route(&clean) {
            if let Some(up) = crate::owners::addr(r, state.cfg.pulse, state.cfg.hermes) {
                if method != "GET" && method != "HEAD" {
                    let _ = http::write_simple(
                        stream,
                        405,
                        "Allow: GET, HEAD\r\n",
                        "Method Not Allowed\n",
                        head_only,
                        false,
                    );
                    log(state, method, &clean, 405);
                    return false;
                }
                let mut h = head.clone();
                h.target = crate::owners::target(r, query);
                h.headers.retain(|(k, _)| {
                    !crate::owners::DROP_HEADERS
                        .iter()
                        .any(|d| k.eq_ignore_ascii_case(d))
                });
                let code = proxy::forward(stream, &h, Vec::new(), up, 0);
                log(state, method, &clean, code);
                return false;
            }
        } else if crate::owners::is_retired(&clean) {
            let body = crate::owners::retired_body(&clean);
            let ok = http::write_head(
                stream,
                410,
                "Content-Type: application/json\r\nCache-Control: no-store\r\n",
                body.len() as u64,
                keep,
            )
            .and_then(|_| {
                if head_only {
                    Ok(())
                } else {
                    stream.write_all(body.as_bytes())
                }
            });
            log(state, method, &clean, 410);
            if keep && ok.is_ok() {
                *next_carry = carry;
                return true;
            }
            return false;
        }
    }

    if let Some(up) = state.cfg.upstream {
        if state
            .cfg
            .proxy_prefixes
            .iter()
            .any(|p| clean.starts_with(p.as_str()))
        {
            let code = proxy::forward(stream, head, carry, up, state.cfg.max_body);
            log(state, method, &clean, code);
            return false;
        }
    }

    if let Some(ui) = &state.ui {
        if ui.owns(&clean, state.site.shell().is_some()) {
            let (code, keep) = serve_ui(stream, head, carry, ui, &clean, query, keep, next_carry);
            log(state, method, &clean, code);
            return keep;
        }
    }

    enum Target<'a> {
        Redirect(&'a str),
        File(&'a Entry),
        None,
    }
    let target = if let Some(to) = state.routes.redirect(&clean) {
        Target::Redirect(to)
    } else if let Some(e) = state.site.file(&clean) {
        Target::File(e)
    } else if state.routes.is_page(&clean) {
        state.site.shell().map(Target::File).unwrap_or(Target::None)
    } else {
        Target::None
    };

    let code = match target {
        Target::None => {
            let _ = http::write_simple(stream, 404, "", "Not Found\n", head_only, keep);
            404
        }
        _ if method != "GET" && method != "HEAD" => {
            let _ = http::write_simple(
                stream,
                405,
                "Allow: GET, HEAD\r\n",
                "Method Not Allowed\n",
                head_only,
                keep,
            );
            405
        }
        Target::Redirect(to) => {
            let loc = if query.is_empty() {
                to.to_string()
            } else {
                format!("{to}?{query}")
            };
            let _ = http::write_simple(
                stream,
                302,
                &format!("Location: {loc}\r\n"),
                "",
                head_only,
                keep,
            );
            302
        }
        Target::File(e) => match serve_file(stream, head, e, head_only, keep) {
            Ok(code) => code,
            Err(_) => {
                keep = false;
                500
            }
        },
    };
    log(state, method, &clean, code);
    if keep {
        *next_carry = carry;
    }
    keep
}

fn serve_file(
    stream: &mut TcpStream,
    head: &RequestHead,
    e: &Entry,
    head_only: bool,
    keep: bool,
) -> io::Result<u16> {
    let common = format!(
        "Content-Type: {}\r\nCache-Control: {}\r\nETag: {}\r\n",
        e.content_type, e.cache_control, e.etag
    );
    if let Some(inm) = head.header("if-none-match") {
        if inm
            .split(',')
            .any(|t| t.trim() == e.etag || t.trim() == "*")
        {
            http::write_head(stream, 304, &common, 0, keep)?;
            stream.flush()?;
            return Ok(304);
        }
    }
    let mut f = match File::open(&e.file) {
        Ok(f) => f,
        Err(_) => {
            http::write_simple(stream, 404, "", "Not Found\n", head_only, keep)?;
            return Ok(404);
        }
    };
    let len = f.metadata()?.len();
    http::write_head(stream, 200, &common, len, keep)?;
    if !head_only {
        let sent = io::copy(&mut (&mut f).take(len), stream)?;
        if sent != len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "file shrank while serving",
            ));
        }
    }
    stream.flush()?;
    Ok(200)
}

/// Answer a request owned by the Rust UI. Returns (status, keep-alive).
#[allow(clippy::too_many_arguments)]
fn serve_ui(
    stream: &mut TcpStream,
    head: &RequestHead,
    carry: Vec<u8>,
    ui: &console_ui::Ui,
    clean: &str,
    query: &str,
    keep: bool,
    next_carry: &mut Vec<u8>,
) -> (u16, bool) {
    let method = head.method.as_str();
    let head_only = method == "HEAD";
    // Form bodies only: small, Content-Length framed, read fully here.
    let mut body = Vec::new();
    let mut keep = keep;
    if head.header("transfer-encoding").is_some() {
        let _ = http::write_simple(stream, 411, "", "Length Required\n", head_only, false);
        return (411, false);
    }
    if let Some(len) = head.header("content-length") {
        let n: usize = match len.trim().parse() {
            Ok(n) => n,
            Err(_) => {
                let _ = http::write_simple(stream, 400, "", "Bad Request\n", head_only, false);
                return (400, false);
            }
        };
        if n > console_ui::MAX_FORM {
            let _ = http::write_simple(stream, 413, "", "Content Too Large\n", head_only, false);
            return (413, false);
        }
        body = carry;
        while body.len() < n {
            let mut buf = [0u8; 1024];
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => return (400, false),
                Ok(k) => body.extend_from_slice(&buf[..k]),
            }
        }
        body.truncate(n);
        keep = false;
    } else if keep {
        *next_carry = carry;
    }
    let peer = stream.peer_addr().ok();
    let peer_loopback = peer.is_some_and(|p| p.ip().is_loopback());
    let req = console_ui::Request {
        method,
        path: clean,
        query,
        host: head.header("host").unwrap_or(""),
        cookie: head.header("cookie"),
        origin: head.header("origin"),
        sec_fetch_site: head.header("sec-fetch-site"),
        // Only a loopback peer (cloudflared) may tell us the scheme.
        forwarded_proto: head.header("x-forwarded-proto").filter(|_| peer_loopback),
        peer_loopback,
        client_ip: peer.map(|p| p.ip().to_string()).unwrap_or_default(),
        content_type: head.header("content-type"),
        body: &body,
    };
    let reply = ui.handle(&req);
    let extra: String = reply
        .headers
        .iter()
        .map(|(k, v)| {
            let v: String = v.chars().filter(|c| *c != '\r' && *c != '\n').collect();
            format!("{k}: {v}\r\n")
        })
        .collect();
    let ok = http::write_head(stream, reply.status, &extra, reply.body.len() as u64, keep)
        .and_then(|_| {
            if head_only {
                Ok(())
            } else {
                stream.write_all(&reply.body)
            }
        })
        .and_then(|_| stream.flush());
    (reply.status, keep && ok.is_ok())
}

/// Read a small Content-Length body for a local API POST. Errors are the
/// HTTP status to answer (411 chunked, 413 too large, 400 bad/short).
fn read_small_body(
    stream: &mut TcpStream,
    head: &RequestHead,
    carry: Vec<u8>,
) -> Result<Vec<u8>, u16> {
    if head.header("transfer-encoding").is_some() {
        return Err(411);
    }
    let n: usize = match head.header("content-length") {
        None => return Ok(Vec::new()),
        Some(v) => v.trim().parse().map_err(|_| 400u16)?,
    };
    if n > crate::api::MAX_API_BODY {
        return Err(413);
    }
    let mut body = carry;
    while body.len() < n {
        let mut buf = [0u8; 1024];
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return Err(400),
            Ok(k) => body.extend_from_slice(&buf[..k]),
        }
    }
    body.truncate(n);
    Ok(body)
}
