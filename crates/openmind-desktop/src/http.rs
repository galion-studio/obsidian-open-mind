// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Optional loopback server for the control-console page.
//!
//! Only [`crate::Mode::Listen`] calls [`listen`]. Any other address than
//! loopback is refused before a socket is created.

use crate::error::DesktopError;
use crate::page::{self, CSP};
use galion_common::http::{Limits, Request, Response};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

/// Serves [`page::document`] until the process is stopped.
///
/// # Errors
/// [`DesktopError::Refused`] when `addr` is not loopback.
/// [`DesktopError::Usage`] when `addr` is not `host:port`.
/// [`DesktopError::Bind`] when the OS rejects the bind, or the accept loop ends.
pub fn listen(addr: &str) -> Result<(), DesktopError> {
    let listener = bind_loopback(addr)?;
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| addr.to_string());
    eprintln!("openmind-desktop: listening on {bound}");
    let limits = Limits {
        max_body_bytes: 4096,
        max_connections: 8,
        ..Limits::default()
    };
    let handler: Arc<galion_common::http::Handler> =
        Arc::new(|req: &Request| reply(req.method(), req.path()));
    galion_common::http::serve(listener, limits, handler).map_err(|source| DesktopError::Bind {
        addr: addr.to_string(),
        source,
    })
}

/// Binds `addr` after checking it is loopback. `127.0.0.1:0` picks a free port.
///
/// # Errors
/// See [`listen`].
pub fn bind_loopback(addr: &str) -> Result<TcpListener, DesktopError> {
    let parsed = parse_loopback(addr)?;
    TcpListener::bind(parsed).map_err(|source| DesktopError::Bind {
        addr: addr.to_string(),
        source,
    })
}

/// Parses `addr` and refuses anything that is not a loopback socket address.
///
/// # Errors
/// [`DesktopError::Usage`] or [`DesktopError::Refused`].
pub fn parse_loopback(addr: &str) -> Result<SocketAddr, DesktopError> {
    let parsed: SocketAddr = addr
        .parse()
        .map_err(|_| DesktopError::Usage(format!("openmind-desktop: bad address {addr:?}")))?;
    if parsed.ip().is_loopback() {
        Ok(parsed)
    } else {
        Err(DesktopError::Refused(format!(
            "openmind-desktop: {addr} is not a loopback address"
        )))
    }
}

/// One response. `GET` and `HEAD` of `/` return the page. Nothing is uploaded.
#[must_use]
pub fn reply(method: &str, path: &str) -> Response {
    match (method, path) {
        ("GET" | "HEAD", "/" | "/index.html") => Response::new(
            200,
            "text/html; charset=utf-8",
            page::document().as_bytes().to_vec(),
        )
        .with_header("content-security-policy", CSP)
        .with_header("referrer-policy", "no-referrer")
        .with_header("x-content-type-options", "nosniff")
        .with_header("cache-control", "no-store"),
        ("GET" | "HEAD", _) => Response::text(404, "Not found\n"),
        _ => Response::text(405, "Method not allowed\n").with_header("allow", "GET, HEAD"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn refuses_non_loopback_before_bind() {
        for addr in [
            "0.0.0.0:18081",
            "10.1.2.3:80",
            "[::]:18081",
            "not-an-address",
        ] {
            let err = parse_loopback(addr).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{addr}");
        }
        assert!(parse_loopback("127.0.0.1:18081").is_ok());
        assert!(parse_loopback("[::1]:18081").is_ok());
    }

    #[test]
    fn get_root_is_the_console() {
        let home = reply("GET", "/");
        let bytes = home.to_bytes(false);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 "));
        assert!(text.contains("content-security-policy:"));
        assert!(text.contains("data-desktop=\"control-console\""));
        assert!(reply("GET", "/nope")
            .to_bytes(false)
            .starts_with(b"HTTP/1.1 404"));
        assert!(reply("POST", "/")
            .to_bytes(false)
            .starts_with(b"HTTP/1.1 405"));
        let head = String::from_utf8(home.to_bytes(true)).unwrap();
        assert!(head.contains("content-length:"));
        assert!(!head.contains("data-desktop"));
    }

    #[test]
    fn serves_on_loopback() {
        let listener = bind_loopback("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        thread::spawn(move || {
            let limits = Limits {
                max_body_bytes: 1024,
                max_connections: 4,
                ..Limits::default()
            };
            let handler: Arc<galion_common::http::Handler> =
                Arc::new(|req: &Request| reply(req.method(), req.path()));
            let _ = galion_common::http::serve(listener, limits, handler);
        });
        let mut body = String::new();
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(20));
            let mut stream = match TcpStream::connect(addr) {
                Ok(s) => s,
                Err(_) => continue,
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .write_all(b"GET / HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n")
                .unwrap();
            stream.read_to_string(&mut body).unwrap();
            break;
        }
        assert!(body.contains("data-desktop=\"control-console\""), "{body}");
    }
}
