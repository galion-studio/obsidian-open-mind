// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Whitelist static responses. Byte layout matches the live `/opt/galion/tuner` server.

use std::fs;

/// One route the server is willing to answer. Anything else is 404.
pub struct ListedFile {
    /// Request path, no query string.
    pub route: &'static str,
    /// File name under the configured root. Not taken from the request.
    pub name: &'static str,
    /// `Content-Type` header value.
    pub content_type: &'static str,
}

/// The live whitelist. Paths are exact, so `..` never selects a file.
pub const FILES: &[ListedFile] = &[
    ListedFile {
        route: "/",
        name: "index.html",
        content_type: "text/html; charset=utf-8",
    },
    ListedFile {
        route: "/index.html",
        name: "index.html",
        content_type: "text/html; charset=utf-8",
    },
    ListedFile {
        route: "/app.js",
        name: "app.js",
        content_type: "text/javascript; charset=utf-8",
    },
    ListedFile {
        route: "/pitch.js",
        name: "pitch.js",
        content_type: "text/javascript; charset=utf-8",
    },
    ListedFile {
        route: "/style.css",
        name: "style.css",
        content_type: "text/css; charset=utf-8",
    },
];

fn reply(code: &str, content_type: &str, body: &[u8], head: bool, cache: &str) -> Vec<u8> {
    let len = body.len();
    let header = format!(
        "HTTP/1.1 {code}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {len}\r\n\
         Cache-Control: {cache}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Permissions-Policy: microphone=(self)\r\n\
         Content-Security-Policy: default-src 'self'; style-src 'self'; script-src 'self'; media-src 'self' blob:\r\n\
         Connection: close\r\n\
         \r\n"
    );
    let mut out = header.into_bytes();
    if !head {
        out.extend_from_slice(body);
    }
    out
}

/// Full response for one request buffer, or `None` when the live server would
/// close without answering (empty read).
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
    for file in FILES {
        if file.route == path {
            return Some(match fs::read(format!("{root}/{}", file.name)) {
                Ok(body) => reply(
                    "200 OK",
                    file.content_type,
                    &body,
                    head,
                    "public, max-age=300",
                ),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn www() -> String {
        format!("{}/www", env!("CARGO_MANIFEST_DIR"))
    }

    fn text(response: Option<Vec<u8>>) -> String {
        String::from_utf8(response.expect("response")).expect("utf8")
    }

    #[test]
    fn health_head_and_errors_are_byte_exact() {
        let health = text(respond(b"GET /health HTTP/1.1\r\n\r\n", &www()));
        assert_eq!(
            health,
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: 12\r\n\
             Cache-Control: no-store\r\n\
             X-Content-Type-Options: nosniff\r\n\
             Referrer-Policy: no-referrer\r\n\
             Permissions-Policy: microphone=(self)\r\n\
             Content-Security-Policy: default-src 'self'; style-src 'self'; script-src 'self'; media-src 'self' blob:\r\n\
             Connection: close\r\n\
             \r\n\
             {\"ok\":true}\n"
        );
        let head = text(respond(b"HEAD /health HTTP/1.1\r\n\r\n", &www()));
        assert!(head.ends_with("\r\n\r\n"));
        assert!(head.contains("Content-Length: 12\r\n"));
        assert!(!head.contains("{\"ok\":true}"));

        let denied = text(respond(b"POST /health HTTP/1.1\r\n\r\n", &www()));
        assert!(denied.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
        assert!(denied.ends_with("method not allowed\n"));

        let missing = text(respond(b"GET /../etc/passwd HTTP/1.1\r\n\r\n", &www()));
        assert!(missing.starts_with("HTTP/1.1 404 Not Found\r\n"));
        assert!(missing.ends_with("not found\n"));
        assert!(respond(b"", &www()).is_none());
    }

    #[test]
    fn serves_copied_page_and_strips_query() {
        let page = text(respond(b"GET /?x=1 HTTP/1.1\r\n\r\n", &www()));
        let body = fs::read(format!("{}/index.html", www())).unwrap();
        assert!(page.contains("Content-Type: text/html; charset=utf-8\r\n"));
        assert!(page.contains("Cache-Control: public, max-age=300\r\n"));
        assert!(page.as_bytes().ends_with(&body));

        let same = text(respond(b"GET /index.html HTTP/1.1\r\n\r\n", &www()));
        assert_eq!(page, same);

        let css = fs::metadata(format!("{}/style.css", www())).unwrap().len();
        let head = text(respond(b"HEAD /style.css HTTP/1.1\r\n\r\n", &www()));
        assert!(head.contains(&format!("Content-Length: {css}\r\n")));
        assert!(head.ends_with("\r\n\r\n"));

        let gone = text(respond(b"GET /app.js HTTP/1.1\r\n\r\n", "/nonexistent"));
        assert!(gone.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
        assert!(gone.ends_with("missing file\n"));
    }
}
