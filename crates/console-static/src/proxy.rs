// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Streaming reverse proxy for the allowlisted API prefixes.
//!
//! One upstream connection per request (`Connection: close` both ways), so
//! the response is streamed byte for byte until the upstream closes. That
//! keeps Server-Sent Events (`/api/v1/markers/stream`, hermes run events)
//! working without parsing chunked framing. The `Host` header is forwarded
//! unchanged: the console's phone gate decides by Host.

use crate::http::{self, RequestHead};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Idle limit while waiting for upstream bytes (SSE heartbeats are far shorter).
pub const UPSTREAM_IDLE: Duration = Duration::from_secs(300);
/// Largest upstream response head accepted.
pub const MAX_RESPONSE_HEAD: usize = 64 * 1024;

/// Methods the console API uses. Anything else (CONNECT, TRACE, ...) is 405.
pub const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];

/// Exact path of the build-session zip (query string ignored).
pub fn session_pack_path(target: &str) -> bool {
    let path = target.split('?').next().unwrap_or(target);
    path == "/api/v1/session-pack"
}

/// Defence in depth: the zip needs a paired phone cookie before we even
/// open the upstream. Matches the API's `phone required` body.
pub fn has_admin_cookie(cookie: Option<&str>) -> bool {
    cookie
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .any(|p| p.starts_with("galion_admin=") && p.len() > "galion_admin=".len())
}

/// Force Android-friendly download headers on a successful pack response.
/// Keeps upstream status/body; replaces or adds Content-Type,
/// Content-Disposition, Cache-Control, nosniff.
pub fn harden_session_pack_head(raw: &[u8]) -> Option<(Vec<u8>, u16)> {
    let (head, code) = rewrite_response_head(raw)?;
    if code != 200 {
        return Some((head, code));
    }
    let text = std::str::from_utf8(&head).ok()?;
    let text = text.strip_suffix("\r\n\r\n")?;
    let mut lines = text.split("\r\n");
    let status = lines.next()?;
    let mut out = String::from(status);
    out.push_str("\r\n");
    for l in lines {
        let (k, _) = match l.split_once(':') {
            Some(kv) => kv,
            None => continue,
        };
        if k.eq_ignore_ascii_case("content-type")
            || k.eq_ignore_ascii_case("content-disposition")
            || k.eq_ignore_ascii_case("cache-control")
            || k.eq_ignore_ascii_case("x-content-type-options")
        {
            continue;
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    out.push_str("Content-Type: application/zip\r\n");
    out.push_str("Content-Disposition: attachment; filename=\"build-session.zip\"\r\n");
    out.push_str("Cache-Control: no-store\r\n");
    out.push_str("X-Content-Type-Options: nosniff\r\n");
    out.push_str("Connection: close\r\n\r\n");
    Some((out.into_bytes(), code))
}

fn refuse_phone(client: &mut TcpStream, head_only: bool) -> u16 {
    let body = "{\"error\":\"phone required\"}";
    let head = format!(
        "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = client.write_all(head.as_bytes());
    if !head_only {
        let _ = client.write_all(body.as_bytes());
    }
    let _ = client.flush();
    401
}

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn is_hop(name: &str, listed: &[String]) -> bool {
    HOP_BY_HOP.iter().any(|h| name.eq_ignore_ascii_case(h))
        || listed.iter().any(|l| name.eq_ignore_ascii_case(l))
}

/// Request-body length, or the status to answer instead.
pub fn body_len(head: &RequestHead, max_body: u64) -> Result<u64, u16> {
    if head.header("transfer-encoding").is_some() {
        // Chunked uploads are not proxied; browsers send Content-Length for
        // fetch() bodies and FormData.
        return Err(411);
    }
    match head.header("content-length") {
        None => Ok(0),
        Some(v) if !v.is_empty() && v.len() <= 19 && v.bytes().all(|b| b.is_ascii_digit()) => {
            let n: u64 = v.parse().map_err(|_| 400u16)?;
            if n > max_body {
                Err(413)
            } else {
                Ok(n)
            }
        }
        Some(_) => Err(400),
    }
}

/// Which upstream a request goes to; decides the request-head policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `--upstream` (the console API): every end-to-end header is forwarded,
    /// the API needs the session cookie and phone gate headers.
    Api,
    /// `--data-upstream` (a read-only data reader such as the bank reader,
    /// F-351-1): only [`DATA_FORWARD`] headers are forwarded. Cookies,
    /// `Authorization` and anything unknown never leave the console, and a
    /// `Set-Cookie` from the reader is dropped.
    Data,
}

/// Request headers a data reader may see (allowlist, case-insensitive).
/// Credentials (`Cookie`, `Authorization`, `Proxy-Authorization`, API key
/// headers, ...) are not in it and are therefore never forwarded.
/// `X-Forwarded-*` are rebuilt by the console after this list.
pub const DATA_FORWARD: &[&str] = &[
    "host",
    "accept",
    "accept-language",
    "user-agent",
    "if-none-match",
    "if-modified-since",
    "cache-control",
];

fn data_allowed(name: &str) -> bool {
    DATA_FORWARD.iter().any(|h| name.eq_ignore_ascii_case(h))
}

/// The request head sent upstream.
pub fn upstream_head(head: &RequestHead, client_ip: IpAddr) -> String {
    upstream_head_mode(head, client_ip, Mode::Api)
}

/// The request head sent upstream under `mode`'s header policy.
pub fn upstream_head_mode(head: &RequestHead, client_ip: IpAddr, mode: Mode) -> String {
    let listed: Vec<String> = head
        .header("connection")
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut out = format!("{} {} HTTP/1.1\r\n", head.method, head.target);
    let mut xff = None;
    let mut proto = None;
    for (k, v) in &head.headers {
        if is_hop(k, &listed) {
            continue;
        }
        if k.eq_ignore_ascii_case("x-forwarded-for") {
            xff = Some(v.clone());
            continue;
        }
        if k.eq_ignore_ascii_case("x-forwarded-proto") {
            proto = Some(v.clone());
            continue;
        }
        if k.eq_ignore_ascii_case("x-forwarded-host") {
            continue;
        }
        if mode == Mode::Data && !data_allowed(k) {
            continue;
        }
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    let xff = match xff {
        Some(prev) => format!("{prev}, {client_ip}"),
        None => client_ip.to_string(),
    };
    out.push_str(&format!("X-Forwarded-For: {xff}\r\n"));
    let proto = proto
        .filter(|p| p == "http" || p == "https")
        .unwrap_or_else(|| "http".to_string());
    out.push_str(&format!("X-Forwarded-Proto: {proto}\r\n"));
    if let Some(h) = head.header("host") {
        out.push_str(&format!("X-Forwarded-Host: {h}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");
    out
}

/// Rewrite the upstream response head: drop hop-by-hop connection headers,
/// force `Connection: close`, add nosniff when missing. Returns the new head
/// and the status code.
pub fn rewrite_response_head(raw: &[u8]) -> Option<(Vec<u8>, u16)> {
    rewrite_response_head_mode(raw, Mode::Api)
}

/// [`rewrite_response_head`]; in [`Mode::Data`] also drops `Set-Cookie`
/// (a data reader must never set cookies on the console origin).
pub fn rewrite_response_head_mode(raw: &[u8], mode: Mode) -> Option<(Vec<u8>, u16)> {
    let text = std::str::from_utf8(raw).ok()?;
    let text = text.strip_suffix("\r\n\r\n")?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next()?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next()?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return None;
    }
    let code: u16 = parts.next()?.parse().ok()?;
    if !(100..=599).contains(&code) {
        return None;
    }
    let mut out = format!(
        "HTTP/1.1 {code} {}\r\n",
        parts
            .next()
            .filter(|r| !r.is_empty())
            .unwrap_or(http::reason(code))
    );
    let mut nosniff = false;
    let mut chunked = false;
    for l in lines {
        let (k, v) = l.split_once(':')?;
        if k.eq_ignore_ascii_case("connection") || k.eq_ignore_ascii_case("keep-alive") {
            continue;
        }
        if mode == Mode::Data && k.trim().eq_ignore_ascii_case("set-cookie") {
            continue;
        }
        if k.eq_ignore_ascii_case("transfer-encoding") {
            chunked = v.to_ascii_lowercase().contains("chunked");
        }
        if k.eq_ignore_ascii_case("x-content-type-options") {
            nosniff = true;
        }
        out.push_str(l);
        out.push_str("\r\n");
    }
    let _ = chunked; // chunked framing passes through unchanged to the client.
    if !nosniff {
        out.push_str("X-Content-Type-Options: nosniff\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    Some((out.into_bytes(), code))
}

/// Forward one request. `carry` holds body bytes already read after the head.
/// Returns the status for the access log. The caller closes the client
/// connection afterwards.
pub fn forward(
    client: &mut TcpStream,
    head: &RequestHead,
    carry: Vec<u8>,
    upstream: SocketAddr,
    max_body: u64,
) -> u16 {
    forward_mode(client, head, carry, upstream, max_body, Mode::Api)
}

/// [`forward`] with an explicit header policy ([`Mode::Data`] for
/// `--data-upstream`: credential headers stripped, `Set-Cookie` dropped).
pub fn forward_mode(
    client: &mut TcpStream,
    head: &RequestHead,
    mut carry: Vec<u8>,
    upstream: SocketAddr,
    max_body: u64,
    mode: Mode,
) -> u16 {
    let head_only = head.method == "HEAD";
    if session_pack_path(&head.target) {
        if head.method != "GET" && head.method != "HEAD" {
            let _ = http::write_simple(
                client,
                405,
                "Allow: GET, HEAD\r\n",
                "Method Not Allowed\n",
                head_only,
                false,
            );
            return 405;
        }
        if !has_admin_cookie(head.header("cookie")) {
            return refuse_phone(client, head_only);
        }
    }
    let fail = |c: &mut TcpStream, code: u16| {
        let _ = http::write_simple(
            c,
            code,
            "",
            &format!("{}\n", http::reason(code)),
            head_only,
            false,
        );
        code
    };
    if !METHODS.contains(&head.method.as_str()) {
        let _ = http::write_simple(
            client,
            405,
            "Allow: GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS\r\n",
            "Method Not Allowed\n",
            head_only,
            false,
        );
        return 405;
    }
    let len = match body_len(head, max_body) {
        Ok(n) => n,
        Err(code) => return fail(client, code),
    };
    let client_ip = client
        .peer_addr()
        .map(|a| a.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let mut up = match TcpStream::connect_timeout(&upstream, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(_) => return fail(client, 502),
    };
    let _ = up.set_read_timeout(Some(UPSTREAM_IDLE));
    let _ = up.set_write_timeout(Some(Duration::from_secs(30)));
    let _ = up.set_nodelay(true);
    if up
        .write_all(upstream_head_mode(head, client_ip, mode).as_bytes())
        .is_err()
    {
        return fail(client, 502);
    }
    // Body: bytes already buffered first, then exactly the rest.
    carry.truncate(len.min(carry.len() as u64) as usize);
    if up.write_all(&carry).is_err() {
        return fail(client, 502);
    }
    let rest = len - carry.len() as u64;
    if rest > 0 {
        let mut limited = (&mut *client).take(rest);
        match io::copy(&mut limited, &mut up) {
            Ok(n) if n == rest => {}
            _ => return fail(client, 400),
        }
    }
    let _ = up.flush();
    // Response head.
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let end = loop {
        if let Some(end) = http::head_end(&buf) {
            break end;
        }
        if buf.len() > MAX_RESPONSE_HEAD {
            return fail(client, 502);
        }
        match up.read(&mut chunk) {
            Ok(0) => return fail(client, 502),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                return fail(client, 504)
            }
            Err(_) => return fail(client, 502),
        }
    };
    let rewritten = if session_pack_path(&head.target) {
        harden_session_pack_head(&buf[..end])
    } else {
        rewrite_response_head_mode(&buf[..end], mode)
    };
    let Some((new_head, code)) = rewritten else {
        return fail(client, 502);
    };
    if client.write_all(&new_head).is_err() {
        return code;
    }
    if !head_only {
        if client.write_all(&buf[end..]).is_err() {
            return code;
        }
        let _ = client.flush();
        // Stream until the upstream closes; flush per read so SSE frames go
        // out immediately.
        loop {
            match up.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if client.write_all(&chunk[..n]).is_err() || client.flush().is_err() {
                        break;
                    }
                }
            }
        }
    }
    let _ = client.flush();
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(raw: &str) -> RequestHead {
        http::parse_head(raw.as_bytes()).unwrap()
    }

    #[test]
    fn strips_hop_by_hop_and_adds_forwarded() {
        let h = head("POST /api/v1/markers HTTP/1.1\r\nHost: console.galion.app\r\nConnection: keep-alive, X-Secret-Hop\r\nX-Secret-Hop: 1\r\nUpgrade: websocket\r\nX-Forwarded-For: 10.0.0.4\r\nX-Forwarded-Proto: https\r\nX-Forwarded-Host: evil\r\nContent-Length: 2\r\n\r\n");
        let up = upstream_head(&h, "127.0.0.1".parse().unwrap());
        assert!(up.starts_with("POST /api/v1/markers HTTP/1.1\r\n"));
        assert!(up.contains("Host: console.galion.app\r\n"));
        assert!(up.contains("Content-Length: 2\r\n"));
        assert!(up.contains("X-Forwarded-For: 10.0.0.4, 127.0.0.1\r\n"));
        assert!(up.contains("X-Forwarded-Proto: https\r\n"));
        assert!(up.contains("X-Forwarded-Host: console.galion.app\r\n"));
        assert!(!up.contains("evil"));
        assert!(!up.to_ascii_lowercase().contains("upgrade"));
        assert!(!up.contains("X-Secret-Hop"));
        assert!(up.ends_with("Connection: close\r\n\r\n"));
    }

    #[test]
    fn body_rules() {
        assert_eq!(
            body_len(&head("GET /a HTTP/1.1\r\nHost: x\r\n\r\n"), 10),
            Ok(0)
        );
        assert_eq!(
            body_len(
                &head("POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\n"),
                10
            ),
            Ok(5)
        );
        assert_eq!(
            body_len(
                &head("POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 50\r\n\r\n"),
                10
            ),
            Err(413)
        );
        assert_eq!(
            body_len(
                &head("POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: -1\r\n\r\n"),
                10
            ),
            Err(400)
        );
        assert_eq!(
            body_len(
                &head("POST /a HTTP/1.1\r\nHost: x\r\nContent-Length: 1, 1\r\n\r\n"),
                10
            ),
            Err(400)
        );
        assert_eq!(
            body_len(
                &head("POST /a HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n"),
                10
            ),
            Err(411)
        );
    }

    #[test]
    fn response_head_rewrite() {
        let (h, code) = rewrite_response_head(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\nKeep-Alive: timeout=5\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
        let h = String::from_utf8(h).unwrap();
        assert_eq!(code, 200);
        assert!(h.contains("Content-Type: text/event-stream\r\n"));
        assert!(h.contains("Transfer-Encoding: chunked\r\n"));
        assert!(!h.contains("keep-alive"));
        assert!(h.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(h.ends_with("Connection: close\r\n\r\n"));
        let (h, code) = rewrite_response_head(b"HTTP/1.1 401 \r\n\r\n").unwrap();
        assert_eq!(code, 401);
        assert!(String::from_utf8(h)
            .unwrap()
            .starts_with("HTTP/1.1 401 Unauthorized\r\n"));
        assert!(rewrite_response_head(b"SSH-2.0\r\n\r\n").is_none());
        assert!(rewrite_response_head(b"HTTP/1.1 999 X\r\n\r\n").is_none());
    }

    #[test]
    fn session_pack_path_and_cookie() {
        assert!(session_pack_path("/api/v1/session-pack"));
        assert!(session_pack_path("/api/v1/session-pack?x=1"));
        assert!(!session_pack_path("/api/v1/session"));
        assert!(!session_pack_path("/api/v1/session-pack/extra"));
        assert!(has_admin_cookie(Some("a=1; galion_admin=tok; b=2")));
        assert!(!has_admin_cookie(Some("galion_admin=")));
        assert!(!has_admin_cookie(None));
    }

    #[test]
    fn session_pack_head_hardened() {
        let (h, code) = harden_session_pack_head(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: inline\r\nCache-Control: public\r\nContent-Length: 3\r\n\r\n",
        )
        .unwrap();
        assert_eq!(code, 200);
        let h = String::from_utf8(h).unwrap();
        assert!(h.contains("Content-Type: application/zip\r\n"));
        assert!(h.contains("Content-Disposition: attachment; filename=\"build-session.zip\"\r\n"));
        assert!(h.contains("Cache-Control: no-store\r\n"));
        assert!(h.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(!h.contains("application/octet-stream"));
        assert!(!h.contains("inline"));
        let (h, code) = harden_session_pack_head(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n",
        )
        .unwrap();
        assert_eq!(code, 401);
        assert!(String::from_utf8(h).unwrap().contains("application/json"));
    }
    #[test]
    fn data_mode_strips_credentials() {
        let raw = "GET /api/v1/bank?x=1 HTTP/1.1\r\nHost: console.galion.app\r\nCookie: galion_admin=SECRETTOK; galion_theme=dark\r\nAuthorization: Bearer SECRETBEARER\r\nProxy-Authorization: Basic SECRETPROXY\r\nX-Api-Key: SECRETKEY\r\nX-Hermes-Key: SECRETH\r\ncookie: second=SECRET2\r\nAccept: application/json\r\nUser-Agent: t/1\r\nIf-None-Match: \"e1\"\r\nX-Forwarded-For: 10.0.0.4\r\nX-Forwarded-Host: evil\r\n\r\n";
        let h = head(raw);
        let up = upstream_head_mode(&h, "127.0.0.1".parse().unwrap(), Mode::Data);
        assert!(up.starts_with("GET /api/v1/bank?x=1 HTTP/1.1\r\n"));
        assert!(!up.contains("SECRET"), "{up}");
        let low = up.to_ascii_lowercase();
        for k in ["cookie", "authorization", "x-api-key", "x-hermes-key"] {
            assert!(!low.contains(&format!("{k}:")), "{k} in {up}");
        }
        for k in [
            "Host: console.galion.app\r\n",
            "Accept: application/json\r\n",
            "User-Agent: t/1\r\n",
            "If-None-Match: \"e1\"\r\n",
            "X-Forwarded-For: 10.0.0.4, 127.0.0.1\r\n",
            "X-Forwarded-Host: console.galion.app\r\n",
        ] {
            assert!(up.contains(k), "{k} missing in {up}");
        }
        assert!(!up.contains("evil"));
        assert!(up.ends_with("Connection: close\r\n\r\n"));
        // Api mode is unchanged: the console API still gets the session cookie.
        let api = upstream_head(&h, "127.0.0.1".parse().unwrap());
        assert!(api.contains("Cookie: galion_admin=SECRETTOK; galion_theme=dark\r\n"));
        assert!(api.contains("Authorization: Bearer SECRETBEARER\r\n"));
        assert!(!api.contains("SECRETPROXY"), "proxy-auth is hop-by-hop");
    }

    #[test]
    fn data_mode_drops_set_cookie() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: galion_admin=forged; Path=/\r\nset-cookie: b=2\r\nContent-Length: 2\r\n\r\n";
        let (h, code) = rewrite_response_head_mode(raw, Mode::Data).unwrap();
        let h = String::from_utf8(h).unwrap();
        assert_eq!(code, 200);
        assert!(!h.to_ascii_lowercase().contains("set-cookie"), "{h}");
        assert!(h.contains("Content-Type: application/json\r\nContent-Length: 2\r\n"));
        let (h, _) = rewrite_response_head(raw).unwrap();
        assert!(String::from_utf8(h)
            .unwrap()
            .contains("Set-Cookie: galion_admin=forged"));
    }
}
