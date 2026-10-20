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
    active: AtomicUsize,
}

impl State {
    pub fn new(site: Site, routes: Routes, cfg: Config) -> State {
        State {
            site,
            routes,
            cfg,
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
