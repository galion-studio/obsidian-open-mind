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
    /// Hermes research adapter (`--hermes-api`).
    pub hermes: Option<crate::hermes::Hermes>,
    /// Live dictation tunnel (`--stt`).
    pub stt: Option<crate::stt::Stt>,
    /// File explorer (`--files`).
    pub files: Option<crate::files::Files>,
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
        let hermes = match (cfg.hermes_api, cfg.hermes) {
            (true, Some(addr)) => Some(
                crate::hermes::Hermes::new(
                    addr,
                    crate::hermes::load_key(cfg.hermes_key_file.as_deref()),
                    cfg.hermes_providers.clone(),
                    cfg.hermes_laptop,
                )
                .with_models(
                    cfg.hermes_sources.clone(),
                    crate::hermes::load_river_key(cfg.hermes_key_file.as_deref()),
                    cfg.hermes_prefs.clone(),
                ),
            ),
            _ => None,
        };
        let stt = cfg
            .stt
            .map(|a| crate::stt::Stt::new(a, cfg.stt_max_sessions));
        let files = cfg.files_explorer.clone().map(crate::files::Files::new);
        State {
            files,
            site,
            routes,
            cfg,
            ui,
            board: crate::board::Board::new(),
            admin,
            hermes,
            stt,
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

    // `--stt` (t362u): live dictation. Reached only after the local API gate
    // passed (unpaired got 401 above). The stream route also checks Origin.
    if state.cfg.local_api {
        if let Some(stt) = state.stt.as_ref().filter(|_| crate::stt::owns(&clean)) {
            let get = method == "GET" || head_only;
            let (code, out) = match clean.as_str() {
                crate::stt::HEALTH if get => (200, Some(stt.health())),
                crate::stt::STREAM => match stt.tunnel(stream, head, std::mem::take(&mut carry)) {
                    Ok(code) => (code, None),
                    Err(o) => (o.status, Some(o)),
                },
                crate::stt::HEALTH => (405, Some(crate::stt::error(405, "use GET"))),
                _ => (404, Some(crate::stt::error(404, "unknown STT route"))),
            };
            if let Some(o) = out {
                let _ = write_json(stream, o.status, &o.body, head_only, false);
                log(state, method, &clean, o.status);
            } else {
                log(state, method, &clean, code);
            }
            return false;
        }
    }

    // `--hermes-api` (hermes-ui-rebuild): Hermes research adapter. Reached
    // only after the local API gate passed (it answered 401 otherwise), so
    // every proxied route needs a loopback Host+peer or a paired cookie.
    if state.cfg.local_api {
        if let Some(h) = state
            .hermes
            .as_ref()
            .filter(|_| crate::hermes::owns(&clean))
        {
            let mut body = Vec::new();
            if crate::hermes::wants_body(method, &clean) {
                match read_small_body(stream, head, std::mem::take(&mut carry)) {
                    Ok(b) => body = b,
                    Err(code) => {
                        let out = crate::hermes::error(code, "bad request body", &clean);
                        let _ = write_json(stream, out.status, &out.body, head_only, false);
                        log(state, method, &clean, code);
                        return false;
                    }
                }
            }
            match h.handle(method, &clean, query, &body) {
                crate::hermes::Action::Reply(out) => {
                    let _ = write_json(stream, out.status, &out.body, head_only, false);
                    log(state, method, &clean, out.status);
                }
                crate::hermes::Action::Stream { target } => {
                    let mut hdrs = vec![
                        ("Host".to_string(), h.addr.to_string()),
                        ("Accept".to_string(), "text/event-stream".to_string()),
                    ];
                    if let Some(a) = h.auth_header() {
                        hdrs.push(("Authorization".to_string(), a));
                    }
                    let up = RequestHead {
                        method: "GET".into(),
                        target,
                        http11: true,
                        headers: hdrs,
                    };
                    let code = proxy::forward(stream, &up, Vec::new(), h.addr, 0);
                    log(state, method, &clean, code);
                }
            }
            return false;
        }
    }

    // `--files` (console-files): the file explorer API. Reached only after
    // the local API gate passed (401 above otherwise); always closes.
    if let Some(fx) = state
        .files
        .as_ref()
        .filter(|_| state.cfg.local_api && crate::files::owns_api(&clean))
    {
        let code = fx.handle(
            stream,
            head,
            method,
            &clean,
            query,
            std::mem::take(&mut carry),
        );
        log(state, method, &clean, code);
        return false;
    }
    if state.files.is_some()
        && (clean == crate::files::PAGE_PATH || clean == crate::files::EDITOR_PATH)
    {
        let host = head.header("host").unwrap_or("");
        if console_ui::gate::is_console_host(host, &state.cfg.extra_hosts) {
            // The page itself needs a paired device too (same rule as the
            // local API gate: loopback Host from a loopback peer, or a valid
            // galion_admin cookie). Unpaired visitors get a 401 pointer.
            let peer_lb = stream
                .peer_addr()
                .ok()
                .is_some_and(|p| p.ip().is_loopback());
            let lb_host =
                console_ui::gate::is_loopback_host(&console_ui::gate::normalize_host(host));
            let paired = (lb_host && peer_lb)
                || state
                    .admin
                    .as_ref()
                    .is_some_and(|a| a.is_admin(head.header("cookie")));
            // Inside the Rust console UI: the page is drawn in the console
            // shell (header, menu, dock) and an unpaired visitor gets the
            // console's own pairing form with a 401, like /connectors.
            if let (Some(ui), true) = (&state.ui, clean == crate::files::PAGE_PATH) {
                let peer = stream.peer_addr().ok();
                let peer_loopback = peer.is_some_and(|p| p.ip().is_loopback());
                let req = console_ui::Request {
                    method,
                    path: &clean,
                    query,
                    host,
                    cookie: head.header("cookie"),
                    origin: head.header("origin"),
                    sec_fetch_site: head.header("sec-fetch-site"),
                    forwarded_proto: head.header("x-forwarded-proto").filter(|_| peer_loopback),
                    peer_loopback,
                    client_ip: peer.map(|p| p.ip().to_string()).unwrap_or_default(),
                    content_type: head.header("content-type"),
                    body: &[],
                };
                let (fhead, fmain, ftail) = crate::files::shell_parts();
                let reply = ui.shell_page(
                    &req,
                    paired,
                    &console_ui::ShellPage {
                        title: "Files",
                        head: fhead,
                        main: fmain,
                        tail: ftail,
                        csp: crate::files::SHELL_CSP,
                    },
                );
                // Never the explorer without both checks (defence in depth).
                debug_assert!(paired || reply.status != 200);
                let ok = write_reply(stream, &reply, head_only, keep);
                log(state, method, &clean, reply.status);
                if keep && ok {
                    *next_carry = carry;
                    return true;
                }
                return false;
            }
            let code = if !paired {
                401
            } else if method == "GET" || head_only {
                200
            } else {
                405
            };
            let (body, extra): (&[u8], String) = if code == 401 {
                (
                    crate::files::UNPAIRED_HTML.as_bytes(),
                    format!(
                        "Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: {}\r\n",
                        crate::files::CSP
                    ),
                )
            } else if code != 200 {
                (
                    b"Method Not Allowed\n",
                    "Content-Type: text/plain\r\n".into(),
                )
            } else if clean == crate::files::EDITOR_PATH {
                (
                    crate::files::EDITOR_JS,
                    "Content-Type: application/javascript; charset=utf-8\r\nCache-Control: public, max-age=86400\r\n".into(),
                )
            } else {
                (
                    crate::files::page().as_bytes(),
                    format!(
                        "Content-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: {}\r\n",
                        crate::files::CSP
                    ),
                )
            };
            let ok = http::write_head(stream, code, &extra, body.len() as u64, keep)
                .and_then(|_| {
                    if head_only {
                        Ok(())
                    } else {
                        stream.write_all(body)
                    }
                })
                .and_then(|_| stream.flush());
            log(state, method, &clean, code);
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
    let ok = write_reply(stream, &reply, head_only, keep);
    (reply.status, keep && ok)
}

/// Write one Rust UI reply. True when it was written completely.
fn write_reply(
    stream: &mut TcpStream,
    reply: &console_ui::Reply,
    head_only: bool,
    keep: bool,
) -> bool {
    let extra: String = reply
        .headers
        .iter()
        .map(|(k, v)| {
            let v: String = v.chars().filter(|c| *c != '\r' && *c != '\n').collect();
            format!("{k}: {v}\r\n")
        })
        .collect();
    http::write_head(stream, reply.status, &extra, reply.body.len() as u64, keep)
        .and_then(|_| {
            if head_only {
                Ok(())
            } else {
                stream.write_all(&reply.body)
            }
        })
        .and_then(|_| stream.flush())
        .is_ok()
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

/// One JSON answer (`no-store`, `nosniff`).
fn write_json(
    stream: &mut TcpStream,
    status: u16,
    body: &str,
    head_only: bool,
    keep: bool,
) -> io::Result<()> {
    http::write_head(
        stream,
        status,
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
    })
    .and_then(|_| stream.flush())
}
