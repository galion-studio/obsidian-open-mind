// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Content types for the console build and cache policy per file.

/// `Content-Type` for a file name, or `None` when the extension is not one the
/// console build produces. Unknown types are not served at all (404), so a
/// stray `.php`, `.map`, `.ts` or `.env.local` can never be handed out.
pub fn content_type(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.').map(|(_, e)| e)?;
    let ext = ext.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "webm" => "video/webm",
        "mp4" => "video/mp4",
        _ => return None,
    })
}

/// True for Vite's content-hashed output (`/assets/name-HASH.ext`): the name
/// changes whenever the bytes change, so it may be cached forever.
pub fn is_hashed_asset(url_path: &str) -> bool {
    let Some(name) = url_path.strip_prefix("/assets/") else {
        return false;
    };
    if name.contains('/') {
        return false;
    }
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    match stem.rsplit_once('-') {
        Some((_, hash)) => {
            hash.len() >= 8 && hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
        None => false,
    }
}

/// `Cache-Control` for a served file.
pub fn cache_control(url_path: &str, content_type: &str) -> &'static str {
    if is_hashed_asset(url_path) {
        "public, max-age=31536000, immutable"
    } else if content_type.starts_with("text/html") || content_type == "application/manifest+json" {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types() {
        assert_eq!(content_type("a.js"), Some("text/javascript; charset=utf-8"));
        assert_eq!(content_type("A.CSS"), Some("text/css; charset=utf-8"));
        assert_eq!(
            content_type("manifest.webmanifest"),
            Some("application/manifest+json")
        );
        assert_eq!(content_type("og.jpg"), Some("image/jpeg"));
        assert_eq!(content_type("favicon.svg"), Some("image/svg+xml"));
    }

    #[test]
    fn unknown_types_refused() {
        for n in [
            "phpinfo.php",
            "x.map",
            "vite.config.ts",
            "noext",
            "a.env",
            "a.sh",
            "a.py",
        ] {
            assert_eq!(content_type(n), None, "{n}");
        }
    }

    #[test]
    fn hashed_assets() {
        assert!(is_hashed_asset("/assets/index-8epzlwHQ.js"));
        assert!(is_hashed_asset("/assets/styles-CL6A0KsY.css"));
        assert!(is_hashed_asset("/assets/x-WYtXMPOY.js"));
        assert!(!is_hashed_asset("/assets/plain.js"));
        assert!(!is_hashed_asset("/favicon.svg"));
        assert!(!is_hashed_asset("/assets/sub/a-12345678.js"));
    }

    #[test]
    fn cache_policy() {
        assert_eq!(
            cache_control("/assets/index-8epzlwHQ.js", "text/javascript"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            cache_control("/agents", "text/html; charset=utf-8"),
            "no-cache"
        );
        assert_eq!(
            cache_control("/manifest.webmanifest", "application/manifest+json"),
            "no-cache"
        );
        assert_eq!(
            cache_control("/og.jpg", "image/jpeg"),
            "public, max-age=3600"
        );
    }
}
