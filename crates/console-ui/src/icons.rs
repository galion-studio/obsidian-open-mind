// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! SVG icons. Path data carried over from the console-ide
//! `src/components/icon.tsx` (Galion's own code, same licence).
//!
//! V2 (slot 276): every path is rendered once as an immutable sprite at
//! `/_ui/i.<fnv64>.svg` (`<symbol id=…>`); pages reference icons with
//! `<svg …><use href="/_ui/i.<h>.svg#name"/></svg>` (~70 B) instead of
//! inlining the path (~220 B) on every occurrence.

use crate::css;
use std::sync::OnceLock;

/// (name, path data).
pub const PATHS: &[(&str, &str)] = &[
    ("sun", "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8M12 2v2M12 20v2M4.2 4.2l1.4 1.4M18.4 18.4l1.4 1.4M2 12h2M20 12h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4"),
    ("moon", "M16 13.2A6.2 6.2 0 0 1 8.8 5 7 7 0 1 0 16 13.2z"),
    ("desk", "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z"),
    ("research", "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6M12 3v3M12 18v3M3 12h3M18 12h3"),
    ("instances", "M6 5h12v9H6zM4 18h16"),
    ("nerve", "M4 14l4-5 3 3 4-6 5 8"),
    ("defence", "M12 3l7 3v6c0 4.5-3 7-7 9-4-2-7-4.5-7-9V6z"),
    ("chats", "M5 6h14v9H8l-3 3z"),
    ("context", "M4 6h16M4 12h10M4 18h13"),
    ("cache", "M5 7h14v10H5zM8 7V5h8v2"),
    ("loop", "M7 8h8a4 4 0 0 1 0 8H9M7 8l2-2M7 8l2 2"),
    ("voice", "M12 4a3 3 0 0 0-3 3v4a3 3 0 0 0 6 0V7a3 3 0 0 0-3-3M8 18a4 4 0 0 0 8 0M12 18v2"),
    ("file", "M7 4h7l4 4v12H7zM14 4v4h4"),
    ("folder", "M3 7V6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1zM3 10h18"),
    ("tools", "M14 6a3 3 0 0 0-4 4L4 16l4 4 6-6a3 3 0 0 0 4-4l-2 2-2-2z"),
    ("settings", "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6M12 3v2M12 19v2M4.5 6.5l1.5 1.5M18 16.5l1.5 1.5M4.5 17.5 6 16M18 8l1.5-1.5"),
    ("database", "M12 5c4 0 7 1 7 2.5S16 10 12 10 5 9 5 7.5 8 5 12 5M5 8v4c0 1.5 3 2.5 7 2.5s7-1 7-2.5V8M5 13v4c0 1.5 3 2.5 7 2.5s7-1 7-2.5v-4"),
    ("activity", "M3 12h4l2-5 4 10 2-5h6"),
    ("pin", "M9 4h6l-1 6 3 2v2H7v-2l3-2zM12 14v6"),
    ("stop", "M7 7h10v10H7z"),
    ("health", "M4 12h4l2-4 4 8 2-4h4"),
    ("model", "M8 8h8v8H8zM12 4v4M12 16v4M4 12h4M16 12h4"),
    ("menu", "M4 7h16M4 12h16M4 17h16"),
    ("plug", "M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0zM12 16v5"),
    ("close", "M6 6l12 12M18 6 6 18"),
    ("search", "M11 6a5 5 0 1 0 0 10 5 5 0 0 0 0-10M15.5 15.5 20 20"),
    ("send", "M4 12h14M13 6l6 6-6 6"),
    ("scope", "M5 7h14M5 12h9M5 17h11"),
    ("arena", "M12 4l7 4v8l-7 4-7-4V8z"),
    ("agents", "M8 10a3 3 0 1 0 0-6 3 3 0 0 0 0 6M16 11a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5M4 19c.5-3 2-4 4-4s3.5 1 4 4M14 15c2 0 3.2 1 3.8 3"),
    ("services", "M6 6h12v4H6zM6 14h12v4H6z"),
    ("edge", "M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16M4 12h16M12 4c2 2.5 2 13 0 16M12 4c-2 2.5-2 13 0 16"),
    ("api", "M6 5h9l3 3v11H6zM9 12h6M9 16h4"),
    ("gateway", "M5 12h8M13 8l5 4-5 4M5 6v12"),
    ("guardian", "M8 11V8a4 4 0 0 1 8 0v3M6 11h12v8H6z"),
    ("keys", "M8 12a3 3 0 1 0 0-6 3 3 0 0 0 0 6M11 10h9l-2 2 2 2h-3"),
    ("pod", "M8 8h8v8H8zM12 4v4M6 12H4M20 12h-2M12 16v4"),
    ("jupyter", "M5 6h14v12H5zM8 10l2 2-2 2M12 14h4"),
    ("markers", "M12 20s6-5 6-10a6 6 0 1 0-12 0c0 5 6 10 6 10zM12 8a2 2 0 1 0 0 4 2 2 0 0 0 0-4"),
    ("session", "M7 4h10v16H7zM10 8h4M10 12h4"),
    ("cloudflare", "M7 16h10a4 4 0 0 0 .4-8 5 5 0 0 0-9.5 1.5A3.5 3.5 0 0 0 7 16z"),
    ("repos", "M8 6a2 2 0 1 0 0-4 2 2 0 0 0 0 4M16 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4M8 22a2 2 0 1 0 0-4 2 2 0 0 0 0 4M8 6v12M8 8c6 0 6 4 8 4"),
    ("soon", "M12 6v6l4 2M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16"),
    ("fridge", "M8 3h8v18H8zM8 10h8M12 6v2M12 13v2"),
    ("pack", "M4 8l8-4 8 4v8l-8 4-8-4zM12 8v12M4 8l8 4 8-4"),
    ("one", "M12 15a3 3 0 0 0 3-3V7a3 3 0 0 0-6 0v5a3 3 0 0 0 3 3M8 19h8"),
    ("data", "M5 6h14v4H5zM5 14h14v4H5z"),
    ("skill", "M12 4l2 5h5l-4 3 2 5-5-3-5 3 2-5-4-3h5z"),
];

struct Sprite {
    body: String,
    href: String,
    etag: String,
}

fn sprite() -> &'static Sprite {
    static S: OnceLock<Sprite> = OnceLock::new();
    S.get_or_init(|| {
        let body = build_sprite();
        let h = css::fnv64(body.as_bytes());
        Sprite {
            href: format!("/_ui/i.{h:016x}.svg"),
            etag: format!("\"{h:016x}\""),
            body,
        }
    })
}

/// Immutable sprite document: one `<symbol id=name>` per [`PATHS`] entry.
pub fn build_sprite() -> String {
    let mut out = String::with_capacity(4096);
    out.push_str(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" \
         stroke-linecap=\"round\" stroke-linejoin=\"round\">",
    );
    for (name, d) in PATHS {
        // Names are our compile-time constants (ASCII [a-z]); path data is
        // likewise ours and contains no quotes or angle brackets (see tests).
        out.push_str("<symbol id=\"");
        out.push_str(name);
        out.push_str("\" viewBox=\"0 0 24 24\"><path d=\"");
        out.push_str(d);
        out.push_str("\"/></symbol>");
    }
    out.push_str("</svg>\n");
    out
}

/// `/_ui/i.<fnv64>.svg` for the current [`PATHS`] contents.
pub fn sprite_href() -> &'static str {
    &sprite().href
}

/// Raw sprite bytes (served with an immutable Cache-Control).
pub fn sprite_body() -> &'static str {
    &sprite().body
}

/// ETag for the sprite (`"<hex>"`).
pub fn sprite_etag() -> &'static str {
    &sprite().etag
}

/// `<svg>` for `name` via the shared sprite; unknown names still size the box
/// but point at a missing symbol (renders empty, same as an empty path before).
pub fn icon(name: &str, class: &str) -> String {
    let href = sprite_href();
    // Only emit a fragment for known names so a typo cannot invent an id.
    let frag = if PATHS.iter().any(|(n, _)| *n == name) {
        name
    } else {
        ""
    };
    format!(
        "<svg class=\"{class}\" viewBox=\"0 0 24 24\" fill=\"none\" \
         stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" \
         stroke-linejoin=\"round\" aria-hidden=\"true\">\
         <use href=\"{href}#{frag}\"/></svg>"
    )
}

/// The Galion mark (console-ide `src/components/mark.tsx`). Kept inline: one
/// per page, not in [`PATHS`].
pub fn mark(class: &str) -> String {
    format!(
        "<svg class=\"{class}\" viewBox=\"0 0 32 32\" aria-hidden=\"true\"><rect x=\"1.5\" y=\"1.5\" width=\"29\" height=\"29\" rx=\"3\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\"/><path d=\"M10 10.5h8.5c3.2 0 5.5 2.1 5.5 5.5s-2.3 5.5-5.5 5.5H10V10.5zm4 3.2v4.6h4.4c1.5 0 2.4-1 2.4-2.3s-.9-2.3-2.4-2.3H14z\" fill=\"currentColor\"/></svg>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_path_data() {
        assert!(PATHS.len() >= 40, "{}", PATHS.len());
        for (n, d) in PATHS {
            assert!(d.starts_with('M'), "{n}");
            assert!(!d.contains('"') && !d.contains('<'), "{n}");
            assert!(
                n.bytes().all(|b| b.is_ascii_lowercase()),
                "symbol id must be [a-z]: {n}"
            );
        }
    }

    #[test]
    fn icon_uses_sprite_not_inline_path() {
        let s = icon("services", "i");
        assert!(s.contains("<use href=\"/_ui/i."), "{s}");
        assert!(s.contains("#services\""), "{s}");
        assert!(!s.contains("<path"), "{s}");
        assert!(!s.contains("M6 6h12v4H6z"), "{s}");
        let nope = icon("nope", "i");
        assert!(nope.contains("href=\"/_ui/i."), "{nope}");
        assert!(nope.contains("#\""), "empty fragment for unknown: {nope}");
    }

    #[test]
    fn sprite_lists_every_symbol_once() {
        let body = build_sprite();
        assert!(body.starts_with("<svg xmlns="));
        for (n, d) in PATHS {
            assert!(body.contains(&format!("id=\"{n}\"")), "missing symbol {n}");
            assert!(body.contains(d), "missing path for {n}");
            assert_eq!(
                body.matches(&format!("id=\"{n}\"")).count(),
                1,
                "duplicate symbol {n}"
            );
        }
        let href = sprite_href();
        assert!(
            href.starts_with("/_ui/i.") && href.ends_with(".svg"),
            "{href}"
        );
        assert_eq!(href, sprite_href(), "href is stable");
        assert_eq!(sprite_body(), body.as_str());
    }
}
