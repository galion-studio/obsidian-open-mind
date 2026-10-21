// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Strict request-path handling.
//!
//! [`sanitize`] turns a raw request target into a canonical decoded path or
//! refuses it. Refused paths are answered 404 by the server (never 400, so a
//! scanner learns nothing). Rules:
//!
//! - origin-form only: the target starts with `/` (no `*`, no absolute URL);
//! - the query (`?...`) is split off and never decoded here;
//! - `%XX` is decoded exactly once; a bad escape, an encoded `/` or `\`, a
//!   decoded `%` (double encoding), NUL or any control byte is refused;
//! - no empty segment (`//`), no `.`/`..`, and no segment that starts with `.`
//!   (dotfiles such as `/.env`, `/.git/config`, `/public/.env`);
//! - every byte is in a small safe set: ASCII letters, digits and `-_.~@+,=!$`.
//!   Non-ASCII is refused (the console has no such files or routes).

/// Longest accepted path in bytes (after decoding).
pub const MAX_PATH: usize = 1024;

/// Why a path was refused. The server maps every variant to 404.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    NotOriginForm,
    TooLong,
    BadEscape,
    EncodedSeparator,
    DoubleEncoding,
    BadByte,
    EmptySegment,
    DotSegment,
}

/// Split a request target into `(path, query)`. The query excludes `?`.
/// A fragment (`#...`) is dropped; browsers never send one.
pub fn split_target(target: &str) -> (&str, &str) {
    let target = target.split('#').next().unwrap_or("");
    match target.find('?') {
        Some(i) => (&target[..i], &target[i + 1..]),
        None => (target, ""),
    }
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn safe_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_' | b'.' | b'~' | b'@' | b'+' | b',' | b'=' | b'!' | b'$'
        )
}

/// Canonical decoded path for a raw path (no query), or why it is refused.
pub fn sanitize(raw: &str) -> Result<String, Reject> {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'/') {
        return Err(Reject::NotOriginForm);
    }
    if bytes.len() > MAX_PATH * 3 {
        return Err(Reject::TooLong);
    }
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let (Some(h), Some(l)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            ) else {
                return Err(Reject::BadEscape);
            };
            let d = (h << 4) | l;
            match d {
                b'/' | b'\\' => return Err(Reject::EncodedSeparator),
                b'%' => return Err(Reject::DoubleEncoding),
                _ if !safe_byte(d) => return Err(Reject::BadByte),
                _ => out.push(d),
            }
            i += 3;
        } else if b == b'/' || safe_byte(b) {
            out.push(b);
            i += 1;
        } else {
            return Err(Reject::BadByte);
        }
    }
    if out.len() > MAX_PATH {
        return Err(Reject::TooLong);
    }
    // Only ASCII safe bytes and '/' remain, so this cannot fail.
    let path = String::from_utf8(out).map_err(|_| Reject::BadByte)?;
    if path == "/" {
        return Ok(path);
    }
    // A single trailing slash is allowed ("/agents/" is not a different page,
    // but it is not served either: the caller looks it up literally).
    let body = &path[1..];
    for seg in body.split('/') {
        if seg.is_empty() {
            // Trailing slash yields one empty last segment; allow only that.
            continue;
        }
        if seg.starts_with('.') {
            return Err(Reject::DotSegment);
        }
    }
    if body.contains("//") || body.starts_with('/') {
        return Err(Reject::EmptySegment);
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_pass() {
        assert_eq!(sanitize("/").unwrap(), "/");
        assert_eq!(sanitize("/agents").unwrap(), "/agents");
        assert_eq!(
            sanitize("/assets/index-8epzlwHQ.js").unwrap(),
            "/assets/index-8epzlwHQ.js"
        );
        assert_eq!(
            sanitize("/__grok/icon-180.png").unwrap(),
            "/__grok/icon-180.png"
        );
        assert_eq!(
            sanitize("/api/hermes/runs/$id").unwrap(),
            "/api/hermes/runs/$id"
        );
        assert_eq!(sanitize("/a%2Db").unwrap(), "/a-b");
        assert_eq!(sanitize("/agents/").unwrap(), "/agents/");
    }

    #[test]
    fn scanner_probes_refused() {
        for p in [
            "/.env",
            "/public/.env",
            "/.git/config",
            "/.git/HEAD",
            "/opt/galion/console-ide/.git/config",
            "/@fs/opt/galion/console-ide/.git/config",
            "/%2eenv",
            "/%2e%2e/%2e%2e/etc/passwd",
            "/..",
            "/a/../b",
            "/a/./b",
            "/.well-known/security.txt",
            "/.vercel/project.json",
            "/node_modules/.vite/",
        ] {
            assert!(sanitize(p).is_err(), "{p} must be refused");
        }
    }

    #[test]
    fn encodings_and_bytes_refused() {
        assert_eq!(sanitize("/a%2fb"), Err(Reject::EncodedSeparator));
        assert_eq!(sanitize("/a%5cb"), Err(Reject::EncodedSeparator));
        assert_eq!(sanitize("/a%252e"), Err(Reject::DoubleEncoding));
        assert_eq!(sanitize("/a%00"), Err(Reject::BadByte));
        assert_eq!(sanitize("/a%zz"), Err(Reject::BadEscape));
        assert_eq!(sanitize("/a%2"), Err(Reject::BadEscape));
        assert_eq!(sanitize("/a\\b"), Err(Reject::BadByte));
        assert_eq!(sanitize("/a b"), Err(Reject::BadByte));
        assert_eq!(sanitize("/caf\u{e9}"), Err(Reject::BadByte));
        assert_eq!(sanitize("/a:b"), Err(Reject::BadByte));
        assert_eq!(sanitize("//etc/passwd"), Err(Reject::EmptySegment));
        assert_eq!(sanitize("/a//b"), Err(Reject::EmptySegment));
        assert_eq!(sanitize("*"), Err(Reject::NotOriginForm));
        assert_eq!(sanitize("http://x/"), Err(Reject::NotOriginForm));
        assert_eq!(sanitize(""), Err(Reject::NotOriginForm));
        let long = format!("/{}", "a".repeat(MAX_PATH + 1));
        assert_eq!(sanitize(&long), Err(Reject::TooLong));
    }

    #[test]
    fn split_target_works() {
        assert_eq!(split_target("/a?b=1"), ("/a", "b=1"));
        assert_eq!(split_target("/a"), ("/a", ""));
        assert_eq!(split_target("/a#x?y"), ("/a", ""));
        assert_eq!(split_target("/a?b#c"), ("/a", "b"));
    }
}
