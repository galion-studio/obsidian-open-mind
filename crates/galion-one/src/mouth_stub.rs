// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Honest mouth (:8002) stub shared by galion-one's in-process /health and the
// standalone `galion-mouth-stub` binary (slot 291 / publish blocker B7 follow-up
// to slot 259 mouth-honest-health).
//
// Contract (aligned with docs/ops/mouth-honest-health.md):
//   * GET  /health     -> 503 JSON, status "unavailable", reason mentions
//     "voice not deployed" (never "loading" / "chatterbox").
//   * POST /synthesize -> 503 JSON with the same reason (no audio, no model).
//   * No keep-warm loop, no model load, no remote dial.
//
// The full galion-one binary still owns the live :8002 listener today; this
// module is the truthful body, and `galion-mouth-stub` is the deploy-ready
// tiny binary that can sit on :8002 alone when One is not the mouth host.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Explicit reason string for probes / nerve (must not contain "loading").
pub const REASON: &str = "voice not deployed; set GALION_MOUTH to a galion-voice-serve address";

/// Default bind for the standalone stub (same port as ONE_MOUTH_BIND).
pub const DEFAULT_BIND: &str = "127.0.0.1:8002";

/// JSON body for the honest unavailable /health answer.
pub fn unavailable_body() -> String {
    format!(
        "{{\"ok\":false,\"status\":\"unavailable\",\"provider\":\"stub\",\"model\":null,\"loaded\":false,\"reason\":\"{REASON}\"}}"
    )
}

/// (status, body) for GET /health when no speech backend is configured.
pub fn unavailable_health() -> (u16, Vec<u8>) {
    (503, unavailable_body().into_bytes())
}

/// (status, body) for POST /synthesize on the stub (no audio ever).
pub fn unavailable_synthesize() -> (u16, Vec<u8>) {
    let body = format!(
        "{{\"ok\":false,\"status\":\"unavailable\",\"error\":\"voice\",\"reason\":\"{REASON}\"}}"
    );
    (503, body.into_bytes())
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            header_end = pos;
            break;
        }
        if buf.len() > 64 * 1024 {
            return Ok(());
        }
    }
    let head = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = head.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");
    let mut length = 0usize;
    for line in lines {
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = rest.trim().parse().unwrap_or(0);
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < length && length <= 64 * 1024 {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let _ = body;

    if method == "GET" && path == "/health" {
        let (status, bytes) = unavailable_health();
        return write_response(&mut stream, status, "application/json", &bytes);
    }
    if method == "POST" && path == "/synthesize" {
        let (status, bytes) = unavailable_synthesize();
        return write_response(&mut stream, status, "application/json", &bytes);
    }
    write_response(&mut stream, 404, "text/plain", b"not found\n")
}

/// Bind `addr` and serve the honest stub forever (no keep-warm, no models).
pub fn serve(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    eprintln!("mouth-stub: listening on {addr} (voice not deployed)");
    for stream in listener.incoming().flatten() {
        let _ = stream.set_nodelay(true);
        std::thread::spawn(move || {
            if let Err(err) = handle(stream) {
                eprintln!("mouth-stub: {err}");
            }
        });
    }
    Ok(())
}

/// Resolve bind: `--bind ADDR` flag, else `GALION_MOUTH_STUB_BIND`, else
/// `ONE_MOUTH_BIND`, else [`DEFAULT_BIND`].
pub fn resolve_bind(args: &[String]) -> String {
    if let Some(i) = args.iter().position(|a| a == "--bind") {
        if let Some(v) = args.get(i + 1) {
            if !v.trim().is_empty() {
                return v.clone();
            }
        }
    }
    for key in ["GALION_MOUTH_STUB_BIND", "ONE_MOUTH_BIND"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return v;
            }
        }
    }
    DEFAULT_BIND.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_503_unavailable_not_loading() {
        let (code, body) = unavailable_health();
        let s = String::from_utf8_lossy(&body);
        assert_eq!(code, 503);
        assert!(s.contains(r#""status":"unavailable""#), "{s}");
        assert!(s.contains("voice not deployed"), "{s}");
        assert!(s.contains("set GALION_MOUTH"), "{s}");
        assert!(s.contains(r#""loaded":false"#), "{s}");
        assert!(!s.contains("loading"), "{s}");
        assert!(!s.contains("chatterbox"), "{s}");
        assert!(!s.contains("cuda"), "{s}");
    }

    #[test]
    fn synthesize_is_503_same_reason() {
        let (code, body) = unavailable_synthesize();
        let s = String::from_utf8_lossy(&body);
        assert_eq!(code, 503);
        assert!(s.contains("voice not deployed"), "{s}");
        assert!(!s.contains("loading"), "{s}");
    }

    #[test]
    fn resolve_bind_flag_beats_env() {
        let args = vec!["--bind".into(), "127.0.0.1:21600".into()];
        assert_eq!(resolve_bind(&args), "127.0.0.1:21600");
    }

    #[test]
    fn reason_constant_has_no_loading_lie() {
        assert!(REASON.contains("voice not deployed"));
        assert!(!REASON.contains("loading"));
        assert!(!REASON.contains("chatterbox"));
    }
}
