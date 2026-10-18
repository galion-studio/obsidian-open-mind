// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// tuner: minimal static file server (std only) for the ukulele tuner page,
// imported from /opt/galion/tuner/src/main.rs (B53). The responses are byte
// for byte the live ones: a fixed whitelist of files from the www dir, the
// same headers (CSP, nosniff, microphone=(self)), /health, 405 and 404.
//
// Config (env, loopback defaults, same names as the live tuner.service):
//   TUNER_ADDR  listen address, default 127.0.0.1:8650
//   TUNER_ROOT  www directory, default /opt/galion/tuner/www

use std::fs;

pub const DEFAULT_ADDR: &str = "127.0.0.1:8650";
pub const DEFAULT_ROOT: &str = "/opt/galion/tuner/www";

pub const FILES: &[(&str, &str, &str)] = &[
    ("/", "index.html", "text/html; charset=utf-8"),
    ("/index.html", "index.html", "text/html; charset=utf-8"),
    ("/app.js", "app.js", "text/javascript; charset=utf-8"),
    ("/pitch.js", "pitch.js", "text/javascript; charset=utf-8"),
    ("/style.css", "style.css", "text/css; charset=utf-8"),
];

pub fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

pub fn addr() -> String {
    env_or("TUNER_ADDR", DEFAULT_ADDR)
}

pub fn root() -> String {
    env_or("TUNER_ROOT", DEFAULT_ROOT)
}

pub fn reply(code: &str, ctype: &str, body: &[u8], head: bool, cache: &str) -> Vec<u8> {
    let hdr = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\n\
X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
Permissions-Policy: microphone=(self)\r\n\
Content-Security-Policy: default-src 'self'; style-src 'self'; script-src 'self'; media-src 'self' blob:\r\n\
Connection: close\r\n\r\n",
        code,
        ctype,
        body.len(),
        cache
    );
    let mut out = hdr.into_bytes();
    if !head {
        out.extend_from_slice(body);
    }
    out
}

/// The full response for one request read (first read, up to 4 KiB), or None
/// when the live server would close without answering (empty read).
pub fn respond(req: &[u8], root: &str) -> Option<Vec<u8>> {
    if req.is_empty() {
        return None;
    }
    let req = String::from_utf8_lossy(req);
    let mut parts = req.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");
    let head = method == "HEAD";
    if method != "GET" && !head {
        return Some(reply(
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed\n",
            false,
            "no-store",
        ));
    }
    if path == "/health" {
        return Some(reply(
            "200 OK",
            "application/json",
            b"{\"ok\":true}\n",
            head,
            "no-store",
        ));
    }
    for (p, f, ct) in FILES {
        if *p == path {
            return Some(match fs::read(format!("{}/{}", root, f)) {
                Ok(b) => reply("200 OK", ct, &b, head, "public, max-age=300"),
                Err(_) => reply(
                    "500 Internal Server Error",
                    "text/plain",
                    b"missing file\n",
                    head,
                    "no-store",
                ),
            });
        }
    }
    Some(reply(
        "404 Not Found",
        "text/plain",
        b"not found\n",
        head,
        "no-store",
    ))
}

/// The https://<name>.trycloudflare.com URL in a cloudflared log line.
pub fn quick_url(line: &str) -> Option<String> {
    let mut rest = line;
    while let Some(i) = rest.find("https://") {
        let tail = &rest[i + 8..];
        let host: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
            .collect();
        if let Some(name) = host.strip_suffix(".trycloudflare.com") {
            if !name.is_empty() && !name.contains('.') && name != "api" {
                return Some(format!("https://{host}"));
            }
        }
        rest = tail;
    }
    None
}

/// Next restart delay: 2 s doubling to 60 s; a run that lasted 60 s or more
/// counts as healthy and resets the delay.
pub fn next_delay(prev: u64, ran_secs: u64) -> u64 {
    if ran_secs >= 60 {
        2
    } else {
        (prev.max(1) * 2).min(60)
    }
}

/// Minimal JSON string quoting for the URL file.
pub fn quote(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn www() -> String {
        format!("{}/www", env!("CARGO_MANIFEST_DIR"))
    }

    fn text(v: Option<Vec<u8>>) -> String {
        String::from_utf8(v.unwrap()).unwrap()
    }

    #[test]
    fn health_and_errors_byte_exact() {
        let h = text(respond(b"GET /health HTTP/1.1\r\n\r\n", &www()));
        assert_eq!(h, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nPermissions-Policy: microphone=(self)\r\nContent-Security-Policy: default-src 'self'; style-src 'self'; script-src 'self'; media-src 'self' blob:\r\nConnection: close\r\n\r\n{\"ok\":true}\n");
        assert!(text(respond(b"POST / HTTP/1.1\r\n\r\n", &www()))
            .starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
        assert!(
            text(respond(b"GET /../etc/passwd HTTP/1.1\r\n\r\n", &www()))
                .starts_with("HTTP/1.1 404 Not Found\r\n")
        );
        assert!(respond(b"", &www()).is_none());
        let missing = text(respond(b"GET /app.js HTTP/1.1\r\n\r\n", "/nonexistent"));
        assert!(missing.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
    }

    #[test]
    fn files_and_head() {
        let page = text(respond(b"GET /?x=1 HTTP/1.1\r\n\r\n", &www()));
        let body = std::fs::read_to_string(format!("{}/index.html", www())).unwrap();
        assert!(page.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(page.contains("Cache-Control: public, max-age=300\r\n"));
        assert!(page.ends_with(&body));
        let head = text(respond(b"HEAD /style.css HTTP/1.1\r\n\r\n", &www()));
        assert!(head.ends_with("\r\n\r\n"));
        assert!(head.contains(&format!(
            "Content-Length: {}\r\n",
            std::fs::metadata(format!("{}/style.css", www()))
                .unwrap()
                .len()
        )));
    }

    #[test]
    fn quick_url_parsing() {
        let line = "2026-10-18T15:17:02Z INF |  https://lucky-words-here-now.trycloudflare.com                      |";
        assert_eq!(
            quick_url(line).as_deref(),
            Some("https://lucky-words-here-now.trycloudflare.com")
        );
        assert_eq!(
            quick_url("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
        assert_eq!(quick_url("see https://api.trycloudflare.com/tunnel"), None);
        assert_eq!(
            quick_url("https://example.com https://a-b.trycloudflare.com"),
            Some("https://a-b.trycloudflare.com".into())
        );
    }

    #[test]
    fn restart_backoff() {
        let mut d = 2;
        let mut seen = vec![];
        for _ in 0..7 {
            d = next_delay(d, 1);
            seen.push(d);
        }
        assert_eq!(seen, [4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(next_delay(60, 3600), 2);
    }
}
