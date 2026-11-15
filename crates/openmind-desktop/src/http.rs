// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Optional loopback server for the control-console page.
//!
//! Only [`crate::Mode::Listen`] calls [`listen`] or [`listen_with`]. Any
//! address that is not loopback is refused before a socket is created.
//! Uploads are forwarded only to a loopback intake. A peer that is not
//! loopback is refused before a file is touched. `GET /` does not dial the
//! stack. `GET /probes` runs the read-only health probes.

use crate::error::DesktopError;
use crate::files::{self, error_json, error_status, Files};
use crate::page::{self, CSP};
use crate::probe::{self, ProbeConfig};
use galion_common::http::{Limits, Request, Response};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

/// Serves [`page::document`] until the process is stopped.
///
/// Uploads dial [`files::DEFAULT_STACK`]. There is no inbox and no grant,
/// so a path read is refused. The browser picker can still upload bytes.
///
/// # Errors
/// [`DesktopError::Refused`] when `addr` is not loopback.
/// [`DesktopError::Usage`] when `addr` is not `host:port`.
/// [`DesktopError::Files`] when the default stack address is not loopback.
/// [`DesktopError::Bind`] when the OS rejects the bind, or the accept loop ends.
pub fn listen(addr: &str, probes: &ProbeConfig) -> Result<(), DesktopError> {
    let files = Files::open(&[], None, Some(files::DEFAULT_STACK))?;
    listen_with(addr, probes, &files)
}

/// [`listen`] with the operator's grants, inbox, stack address, and probes.
///
/// # Errors
/// See [`listen`].
pub fn listen_with(addr: &str, probes: &ProbeConfig, files: &Files) -> Result<(), DesktopError> {
    let listener = bind_loopback(addr)?;
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| addr.to_string());
    eprintln!("openmind-desktop: listening on {bound}");
    let limits = Limits {
        max_body_bytes: files::MAX_FILE_BYTES,
        max_connections: 8,
        body_timeout: Duration::from_secs(30),
        write_timeout: Duration::from_secs(30),
        accept_chunked: true,
        ..Limits::default()
    };
    let files = files.clone();
    let probes = probes.clone();
    let handler: Arc<galion_common::http::Handler> =
        Arc::new(move |req: &Request| reply(req, &probes, &files));
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

/// One response.
///
/// `GET /` is the page. `GET /files` lists chosen paths. `POST /files/read`
/// reads one of them. `POST /files` uploads picker bytes, or a chosen path
/// when the body is `{"path":"..."}`. Nothing else is a file.
#[must_use]
pub fn reply(req: &Request, probes: &ProbeConfig, files: &Files) -> Response {
    if let Some(peer) = req.peer {
        if !peer.ip().is_loopback() {
            return guarded(Response::json(403, "{\"error\":\"peer\"}"));
        }
    }
    match (req.method(), req.path()) {
        ("GET" | "HEAD", "/" | "/index.html") => page_response(),
        ("GET" | "HEAD", "/one" | "/research") => entry_response(req.path()),
        ("GET" | "HEAD", "/probes") => {
            let body = probe::probe_all(probes).to_json();
            guarded(Response::json(200, body))
        }
        ("GET" | "HEAD", "/files") => guarded(Response::json(200, files.pick_json())),
        ("GET" | "HEAD", "/files/sent") => guarded(Response::json(200, files.sent_json())),
        ("POST", "/files/read") => file_result(read_chosen(req, files)),
        ("POST", "/files") => upload(req, files),
        ("GET" | "HEAD", _) => guarded(Response::text(404, "Not found\n")),
        _ => guarded(
            Response::text(405, "Method not allowed\n").with_header("allow", "GET, HEAD, POST"),
        ),
    }
}

fn entry_response(path: &str) -> Response {
    let body = page::entry(path).unwrap_or("");
    guarded(
        Response::new(200, "text/html; charset=utf-8", body.as_bytes().to_vec())
            .with_header("content-security-policy", CSP),
    )
}

fn page_response() -> Response {
    guarded(
        Response::new(
            200,
            "text/html; charset=utf-8",
            page::document().as_bytes().to_vec(),
        )
        .with_header("content-security-policy", CSP),
    )
}

fn guarded(response: Response) -> Response {
    response
        .with_header("x-content-type-options", "nosniff")
        .with_header("referrer-policy", "no-referrer")
        .with_header("cache-control", "no-store")
}

fn upload(req: &Request, files: &Files) -> Response {
    let result = if let Some(name) = req.header("x-file-name") {
        files.upload_bytes(name, &req.body)
    } else {
        match files::json_path(&req.body) {
            Ok(path) => files.upload_path(&path),
            Err(err) => Err(err),
        }
    };
    file_result(result.map(|receipt| receipt.to_json()))
}

fn read_chosen(req: &Request, files: &Files) -> Result<String, files::FileError> {
    let path = files::json_path(&req.body)?;
    Ok(files.read_path(&path)?.to_json())
}

fn file_result(result: Result<String, files::FileError>) -> Response {
    match result {
        Ok(body) => guarded(Response::json(200, body)),
        Err(err) => guarded(Response::json(error_status(&err), error_json(&err))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galion_common::http::Head;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::thread;
    use std::time::Duration;

    fn request(
        method: &str,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        loopback: bool,
    ) -> Request {
        Request {
            head: Head {
                method: method.into(),
                target: target.into(),
                minor: 1,
                headers: headers
                    .iter()
                    .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                    .collect(),
                content_length: Some(body.len() as u64),
                chunked: false,
                len: 0,
            },
            raw_head: Vec::new(),
            body: body.to_vec(),
            peer: Some(if loopback {
                SocketAddr::from(([127, 0, 0, 1], 9))
            } else {
                SocketAddr::from(([10, 1, 2, 3], 9))
            }),
        }
    }

    fn dispatch(req: &Request, files: &Files) -> Response {
        reply(req, &ProbeConfig::defaults(), files)
    }

    fn call(method: &str, path: &str) -> Response {
        dispatch(&request(method, path, &[], b"", true), &Files::closed())
    }

    #[test]
    fn refuses_non_loopback_before_bind() {
        for addr in [
            "0.0.0.0:18181",
            "10.1.2.3:80",
            "[::]:18181",
            "not-an-address",
        ] {
            let err = parse_loopback(addr).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{addr}");
        }
        assert!(parse_loopback(crate::DEFAULT_BIND).is_ok());
        assert!(parse_loopback("[::1]:18181").is_ok());
    }

    #[test]
    fn get_root_is_the_console() {
        let home = call("GET", "/");
        let bytes = home.to_bytes(false);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 "));
        assert!(text.contains("content-security-policy:"));
        assert!(text.contains("connect-src 'self'"));
        assert!(text.contains("data-desktop=\"control-console\""));
        assert!(call("GET", "/nope")
            .to_bytes(false)
            .starts_with(b"HTTP/1.1 404"));
        assert!(call("POST", "/")
            .to_bytes(false)
            .starts_with(b"HTTP/1.1 405"));
        let head = String::from_utf8(home.to_bytes(true)).unwrap();
        assert!(head.contains("content-length:"));
        assert!(!head.contains("data-desktop"));
    }

    #[test]
    fn one_and_research_are_pages() {
        for path in ["/one", "/research"] {
            let bytes = call("GET", path).to_bytes(false);
            let text = String::from_utf8(bytes).unwrap();
            assert!(text.starts_with("HTTP/1.1 200 "), "{path}");
            assert!(text.contains("data-entry="), "{path}");
            assert!(text.contains("content-security-policy:"), "{path}");
        }
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
            let files = Files::closed();
            let handler: Arc<galion_common::http::Handler> =
                Arc::new(move |req: &Request| dispatch(req, &files));
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

    #[test]
    fn files_stay_on_chosen_paths_and_loopback_peers() {
        let tmp = std::env::temp_dir().join(format!(
            "om-desk-http-{}",
            galion_common::hex::encode(&temp_id())
        ));
        std::fs::create_dir(&tmp).unwrap();
        let grant = tmp.join("chosen");
        std::fs::create_dir(&grant).unwrap();
        std::fs::write(grant.join("notes.txt"), b"inside").unwrap();
        std::fs::write(tmp.join("secret.txt"), b"SENTINEL-OUTSIDE").unwrap();
        let files = Files::open(std::slice::from_ref(&grant), None, Some("127.0.0.1:1")).unwrap();

        let traversal = dispatch(
            &request("POST", "/files", &[], br#"{"path":"../secret.txt"}"#, true),
            &files,
        );
        let text = String::from_utf8(traversal.to_bytes(false)).unwrap();
        assert!(text.starts_with("HTTP/1.1 403 "), "{text}");
        assert!(text.contains("\"error\":\"traversal\""));
        assert!(!text.contains("SENTINEL"));

        let outside = dispatch(
            &request(
                "POST",
                "/files/read",
                &[],
                br#"{"path":"/etc/passwd"}"#,
                true,
            ),
            &files,
        );
        let outside_text = String::from_utf8(outside.to_bytes(false)).unwrap();
        assert!(outside_text.starts_with("HTTP/1.1 403 "), "{outside_text}");
        assert!(!outside_text.contains("root:"));

        let peer = dispatch(
            &request(
                "POST",
                "/files",
                &[("x-file-name", "notes.txt")],
                b"inside",
                false,
            ),
            &files,
        );
        let peer_text = String::from_utf8(peer.to_bytes(false)).unwrap();
        assert!(peer_text.contains("\"error\":\"peer\""));
        assert!(!peer_text.contains("inside"));

        let listed = String::from_utf8(
            dispatch(&request("GET", "/files", &[], b"", true), &files).to_bytes(false),
        )
        .unwrap();
        assert!(listed.contains("notes.txt"));
        assert!(!listed.contains("secret.txt"));
        assert!(call_on("GET", "/files/notes.txt", &[], b"", &files)
            .to_bytes(false)
            .starts_with(b"HTTP/1.1 404"));
        assert!(!listed.contains("SENTINEL"));

        let read = dispatch(
            &request("POST", "/files/read", &[], br#"{"path":"notes.txt"}"#, true),
            &files,
        );
        let read_text = String::from_utf8(read.to_bytes(false)).unwrap();
        assert!(read_text.contains("\"text\":\"inside\""));

        let spoof = dispatch(
            &request(
                "POST",
                "/files?path=/etc/passwd",
                &[("x-file-name", "/etc/passwd")],
                b"hello",
                true,
            ),
            &files,
        );
        let spoof_text = String::from_utf8(spoof.to_bytes(false)).unwrap();
        assert!(
            spoof_text.contains("\"error\":\"type\""),
            "a name is not a path: {spoof_text}"
        );
        assert!(!spoof_text.contains("root:"));

        let closed = call_on(
            "POST",
            "/files",
            &[("x-file-name", "a.txt")],
            b"hi",
            &Files::closed(),
        );
        let closed_text = String::from_utf8(closed.to_bytes(false)).unwrap();
        assert!(closed_text.starts_with("HTTP/1.1 503 "));
        assert!(closed_text.contains("\"error\":\"no-destination\""));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn call_on(
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        files: &Files,
    ) -> Response {
        dispatch(&request(method, path, headers, body, true), files)
    }

    /// Eight bytes that differ per call. Unix reads `/dev/urandom`. Windows
    /// has no such file, so the id is a counter mixed with the process id.
    fn temp_id() -> [u8; 8] {
        #[cfg(unix)]
        {
            let mut buf = [0u8; 8];
            std::fs::File::open("/dev/urandom")
                .unwrap()
                .read_exact(&mut buf)
                .unwrap();
            buf
        }
        #[cfg(not(unix))]
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let n = NEXT
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ u64::from(std::process::id());
            n.to_le_bytes()
        }
    }
}
