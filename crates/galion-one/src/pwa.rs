// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Home-screen install for one.galion.app (2026-11-01 CET).
//!
//! Until the Android and iOS apps ship, the room installs from the browser:
//! Chrome on Android ("Install app") and Safari on iOS ("Add to Home
//! Screen"). Everything is compiled into the binary:
//!
//! * `/manifest.webmanifest`: name, standalone display, colours, icons.
//! * `/sw.js`: a small service worker that caches only the app shell so the
//!   installed app opens offline. It never caches API, pairing, auth, files,
//!   voice or approvals responses (they are not in its list and are never
//!   intercepted) and only ever looks at GET.
//! * `/pwa/*` icons (original, drawn from the Galion mark by
//!   `examples/pwa_icons.rs`) and `/apple-touch-icon.png`.
//!
//! The room page itself is not edited on disk: the head tags and the
//! worker registration are added at serve time, like the boot screen and
//! the Approvals link. Skipped when the page already links a manifest.
//!
//! `ONE_PWA=off` removes the tags and makes `/sw.js` a worker that clears
//! its cache and unregisters itself, so phones that installed it go back to
//! plain network pages without a new binary.

pub const MANIFEST: &[u8] = include_bytes!("../pwa/manifest.webmanifest");
pub const SW: &[u8] = include_bytes!("../pwa/sw.js");
pub const SW_OFF: &[u8] = include_bytes!("../pwa/sw-off.js");
pub const ICON_SVG: &[u8] = include_bytes!("../pwa/icon.svg");
pub const MASKABLE_SVG: &[u8] = include_bytes!("../pwa/maskable.svg");
pub const ICON_192: &[u8] = include_bytes!("../pwa/icon-192.png");
pub const ICON_512: &[u8] = include_bytes!("../pwa/icon-512.png");
pub const MASKABLE_192: &[u8] = include_bytes!("../pwa/maskable-192.png");
pub const MASKABLE_512: &[u8] = include_bytes!("../pwa/maskable-512.png");
pub const APPLE: &[u8] = include_bytes!("../pwa/apple-touch-icon.png");

/// Room sky colour (index-v2.html `theme-color`), also in the manifest.
pub const THEME: &str = "#05060f";

const HEAD: &str = concat!(
    "<link rel=\"manifest\" href=\"/manifest.webmanifest\">",
    "<link rel=\"icon\" href=\"/pwa/icon.svg?v=1\" type=\"image/svg+xml\">",
    "<link rel=\"icon\" href=\"/pwa/icon-192.png?v=1\" sizes=\"192x192\" type=\"image/png\">",
    "<link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png?v=1\" sizes=\"180x180\">",
    "<meta name=\"mobile-web-app-capable\" content=\"yes\">",
    "<meta name=\"apple-mobile-web-app-capable\" content=\"yes\">",
    "<meta name=\"apple-mobile-web-app-title\" content=\"Galion One\">",
    "<meta name=\"apple-mobile-web-app-status-bar-style\" content=\"black\">",
);

const REGISTER: &str = concat!(
    "<script id=\"g1-pwa\">if(\"serviceWorker\"in navigator){",
    "addEventListener(\"load\",function(){",
    "navigator.serviceWorker.register(\"/sw.js\",{scope:\"/\"}).catch(function(){})})}",
    "</script>"
);

/// True unless `ONE_PWA` is `off`, `0`, `no` or `false`.
pub fn enabled_by(v: Option<&str>) -> bool {
    !matches!(
        v.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("off") | Some("0") | Some("no") | Some("false")
    )
}

pub fn enabled() -> bool {
    enabled_by(std::env::var("ONE_PWA").ok().as_deref())
}

/// Add the install tags to the room page (head) and the worker registration
/// (before `</body>`). Unchanged when the page already has a manifest link.
pub fn with_install(html: &str) -> String {
    if html.contains("rel=\"manifest\"") || html.contains("id=\"g1-pwa\"") {
        return html.to_string();
    }
    let Some(at) = html.find("</head>") else {
        return html.to_string();
    };
    let mut head = String::from(HEAD);
    if !html[..at].contains("name=\"theme-color\"") {
        head.push_str(&format!("<meta name=\"theme-color\" content=\"{THEME}\">"));
    }
    let mut out = String::with_capacity(html.len() + head.len() + REGISTER.len());
    out.push_str(&html[..at]);
    out.push_str(&head);
    let rest = &html[at..];
    match rest.rfind("</body>") {
        Some(b) => {
            out.push_str(&rest[..b]);
            out.push_str(REGISTER);
            out.push_str(&rest[b..]);
        }
        None => {
            out.push_str(rest);
            out.push_str(REGISTER);
        }
    }
    out
}

/// A full HTTP response for an install route, or None to fall through.
pub fn respond(method: &str, path: &str, if_none_match: &str, on: bool) -> Option<Vec<u8>> {
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        return None;
    }
    const ICON: &str = "public, max-age=604800";
    let (kind, body, cache): (&str, &[u8], &str) = match path {
        "/manifest.webmanifest" => (
            "application/manifest+json; charset=utf-8",
            MANIFEST,
            "public, max-age=3600",
        ),
        // Browsers check the worker at least daily; no-cache + ETag makes
        // every check a cheap 304 and lets a new binary roll out at once.
        "/sw.js" => (
            "text/javascript; charset=utf-8",
            if on { SW } else { SW_OFF },
            "no-cache",
        ),
        "/pwa/icon.svg" => ("image/svg+xml", ICON_SVG, ICON),
        "/pwa/maskable.svg" => ("image/svg+xml", MASKABLE_SVG, ICON),
        "/pwa/icon-192.png" => ("image/png", ICON_192, ICON),
        "/pwa/icon-512.png" => ("image/png", ICON_512, ICON),
        "/pwa/maskable-192.png" => ("image/png", MASKABLE_192, ICON),
        "/pwa/maskable-512.png" => ("image/png", MASKABLE_512, ICON),
        // iOS also asks for these at the root on its own.
        "/apple-touch-icon.png" | "/apple-touch-icon-precomposed.png" => ("image/png", APPLE, ICON),
        _ => return None,
    };
    let tag = crate::home::etag(body);
    let common = format!(
        "Content-Type: {kind}\r\nCache-Control: {cache}\r\nETag: {tag}\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n"
    );
    if if_none_match.split(',').any(|t| t.trim() == tag) {
        return Some(format!("HTTP/1.1 304 Not Modified\r\n{common}\r\n").into_bytes());
    }
    let mut out = format!(
        "HTTP/1.1 200 OK\r\n{common}Content-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    if !head_only {
        out.extend_from_slice(body);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: &[u8]) -> String {
        String::from_utf8_lossy(r).into_owned()
    }

    /// (width, height, RGBA pixels) of one of our PNGs.
    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let mut at = 8;
        let (mut w, mut h, mut idat) = (0u32, 0u32, Vec::new());
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let kind = &png[at + 4..at + 8];
            let data = &png[at + 8..at + 8 + len];
            if kind == b"IHDR" {
                w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                assert_eq!(&data[8..10], &[8, 6], "8-bit RGBA");
            }
            if kind == b"IDAT" {
                idat.extend_from_slice(data);
            }
            at += 12 + len;
        }
        let (raw, res) = crate::inflate::inflate_zlib(&idat, 8 << 20);
        assert!(res.is_ok());
        let row = (w * 4) as usize;
        assert_eq!(raw.len(), (row + 1) * h as usize);
        let mut px = vec![0u8; row * h as usize];
        for y in 0..h as usize {
            let f = raw[y * (row + 1)];
            for x in 0..row {
                let v = raw[y * (row + 1) + 1 + x];
                let left = if x >= 4 { px[y * row + x - 4] } else { 0 };
                let up = if y > 0 { px[(y - 1) * row + x] } else { 0 };
                px[y * row + x] = match f {
                    0 => v,
                    1 => v.wrapping_add(left),
                    2 => v.wrapping_add(up),
                    _ => panic!("filter {f} not used by pwa_icons"),
                };
            }
        }
        (w, h, px)
    }

    #[test]
    fn icons_have_the_right_sizes_and_backgrounds() {
        for (png, size, opaque) in [
            (ICON_192, 192, false),
            (ICON_512, 512, false),
            (MASKABLE_192, 192, true),
            (MASKABLE_512, 512, true),
            (APPLE, 180, true),
        ] {
            let (w, h, px) = decode(png);
            assert_eq!((w, h), (size, size));
            let corner = &px[..4];
            if opaque {
                assert!(px.chunks(4).all(|p| p[3] == 255), "{size}: fully opaque");
                assert_eq!(corner, &[0x05, 0x06, 0x0f, 255], "{size}: room sky");
            } else {
                assert_eq!(corner[3], 0, "{size}: transparent corner");
            }
            // The lit core sits in the middle.
            let mid = ((size / 2) * size + size / 2) as usize * 4;
            assert_eq!(&px[mid..mid + 4], &[0xB8, 0xE8, 0xFF, 255], "{size}: core");
        }
    }

    #[test]
    fn manifest_is_installable() {
        let m: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
        assert_eq!(m["name"], "Galion One");
        assert_eq!(m["start_url"], "/");
        assert_eq!(m["scope"], "/");
        assert_eq!(m["display"], "standalone");
        assert_eq!(m["theme_color"], THEME);
        assert_eq!(m["background_color"], THEME);
        let icons = m["icons"].as_array().unwrap();
        for need in ["192x192", "512x512"] {
            assert!(icons
                .iter()
                .any(|i| i["sizes"] == need && i["type"] == "image/png" && i["purpose"] == "any"));
        }
        assert!(icons.iter().any(|i| i["purpose"] == "maskable"));
        // Every icon is served by respond().
        for i in icons {
            let src = i["src"].as_str().unwrap();
            let path = src.split('?').next().unwrap();
            let r = respond("GET", path, "", true).expect(path);
            assert!(text(&r).contains(&format!("Content-Type: {}", i["type"].as_str().unwrap())));
        }
    }

    #[test]
    fn worker_never_touches_api_or_auth() {
        let sw = std::str::from_utf8(SW).unwrap();
        assert!(sw.contains("if (r.method !== \"GET\") return;"));
        // The precache list holds only the page, manifest and icons.
        let list = &sw[sw.find("var SHELL = [").unwrap()..];
        let list = &list[..list.find("];").unwrap()];
        for bad in [
            "/api",
            "/pair",
            "/me",
            "/session",
            "/threads",
            "/files",
            "/hear",
            "/reply",
            "/voice",
            "/approvals",
            "/link",
            "/admin",
            "/model",
            "/stt",
        ] {
            assert!(
                !list.contains(&format!("\"{bad}")),
                "{bad} must not be cached"
            );
        }
        // Navigations other than the room are never answered by the worker.
        assert!(sw.contains("if (u.pathname !== \"/\" && u.pathname !== \"/index.html\") return;"));
        let off = std::str::from_utf8(SW_OFF).unwrap();
        assert!(off.contains("unregister()") && !off.contains("\"fetch\""));
    }

    #[test]
    fn routes_and_headers() {
        let r = text(&respond("GET", "/manifest.webmanifest", "", true).unwrap());
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(r.contains("Content-Type: application/manifest+json; charset=utf-8\r\n"));
        assert!(r.contains("Cache-Control: public, max-age=3600\r\n"));
        let r = text(&respond("GET", "/sw.js", "", true).unwrap());
        assert!(r.contains("Content-Type: text/javascript; charset=utf-8\r\n"));
        assert!(r.contains("Cache-Control: no-cache\r\n"));
        assert!(r.contains("var SHELL"));
        let r = text(&respond("GET", "/sw.js", "", false).unwrap());
        assert!(r.contains("unregister") && !r.contains("var SHELL"));
        let r = text(&respond("GET", "/apple-touch-icon.png", "", true).unwrap());
        assert!(r.contains("Content-Type: image/png\r\n"));
        assert!(r.contains("Cache-Control: public, max-age=604800\r\n"));
        assert!(respond("GET", "/apple-touch-icon-precomposed.png", "", true).is_some());
        let r = text(&respond("GET", "/pwa/icon.svg", "", true).unwrap());
        assert!(r.contains("Content-Type: image/svg+xml\r\n"));
        let tag = crate::home::etag(MANIFEST);
        let r = text(&respond("GET", "/manifest.webmanifest", &tag, true).unwrap());
        assert!(r.starts_with("HTTP/1.1 304"));
        let r = respond("HEAD", "/pwa/icon-512.png", "", true).unwrap();
        assert!(text(&r).ends_with("\r\n\r\n"));
        assert!(respond("POST", "/sw.js", "", true).is_none());
        assert!(respond("GET", "/api/v1/services", "", true).is_none());
        assert!(respond("GET", "/pwa/../secret", "", true).is_none());
    }

    #[test]
    fn page_tags() {
        let page = "<html><head><meta name=\"theme-color\" content=\"#05060f\"><title>x</title></head><body><p>hi</p></body></html>";
        let s = with_install(page);
        assert!(s.contains("<link rel=\"manifest\" href=\"/manifest.webmanifest\">"));
        assert!(s.contains("rel=\"apple-touch-icon\""));
        assert_eq!(s.matches("name=\"theme-color\"").count(), 1);
        let head_end = s.find("</head>").unwrap();
        assert!(s.find("rel=\"manifest\"").unwrap() < head_end);
        assert!(s.find("id=\"g1-pwa\"").unwrap() > s.find("<p>hi</p>").unwrap());
        assert!(s.ends_with("</body></html>"));
        assert_eq!(with_install(&s), s, "only once");
        let bare = with_install("<head></head><body></body>");
        assert!(bare.contains(&format!("<meta name=\"theme-color\" content=\"{THEME}\">")));
        assert_eq!(with_install("no head"), "no head");
        let real = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/galion-one-web/v2/index-v2.html"
        ))
        .unwrap();
        let s = with_install(&real);
        assert_eq!(s.matches("rel=\"manifest\"").count(), 1);
        assert_eq!(s.matches("id=\"g1-pwa\"").count(), 1);
        assert_eq!(s.matches("name=\"theme-color\"").count(), 1);
        assert!(enabled_by(None) && enabled_by(Some("on")));
        assert!(!enabled_by(Some("off")) && !enabled_by(Some(" OFF ")) && !enabled_by(Some("0")));
    }
}
