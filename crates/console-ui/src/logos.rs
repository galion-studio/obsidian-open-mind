// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Connector logos (owner t455u, 2026-10-30): each brand's own icon,
//! downloaded once from its official site (sources in assets/logos/SOURCES.md),
//! stored as wrapped base64 text and served from `/_ui/cn-logo/`. Never hotlinked.
//! Logos are trademarks of their owners.

/// `(name, mime, base64)`.
pub const LOGOS: &[(&str, &str, &str)] = &[
    (
        "asana",
        "image/png",
        include_str!("assets/logos/asana.png.b64"),
    ),
    (
        "bigdata",
        "image/png",
        include_str!("assets/logos/bigdata.png.b64"),
    ),
    (
        "bigquery",
        "image/png",
        include_str!("assets/logos/bigquery.png.b64"),
    ),
    ("box", "image/png", include_str!("assets/logos/box.png.b64")),
    (
        "buffer",
        "image/png",
        include_str!("assets/logos/buffer.png.b64"),
    ),
    (
        "calendly",
        "image/png",
        include_str!("assets/logos/calendly.png.b64"),
    ),
    (
        "canva",
        "image/png",
        include_str!("assets/logos/canva.png.b64"),
    ),
    (
        "clickup",
        "image/png",
        include_str!("assets/logos/clickup.png.b64"),
    ),
    (
        "coinbase",
        "image/png",
        include_str!("assets/logos/coinbase.png.b64"),
    ),
    (
        "daloopa",
        "image/png",
        include_str!("assets/logos/daloopa.png.b64"),
    ),
    (
        "datadog",
        "image/png",
        include_str!("assets/logos/datadog.png.b64"),
    ),
    (
        "etoro",
        "image/png",
        include_str!("assets/logos/etoro.png.b64"),
    ),
    (
        "excalidraw",
        "image/png",
        include_str!("assets/logos/excalidraw.png.b64"),
    ),
    (
        "figma",
        "image/png",
        include_str!("assets/logos/figma.png.b64"),
    ),
    (
        "finary",
        "image/png",
        include_str!("assets/logos/finary.png.b64"),
    ),
    (
        "gamma",
        "image/svg+xml",
        include_str!("assets/logos/gamma.svg.b64"),
    ),
    (
        "github",
        "image/png",
        include_str!("assets/logos/github.png.b64"),
    ),
    (
        "gmail",
        "image/png",
        include_str!("assets/logos/gmail.png.b64"),
    ),
    (
        "google-calendar",
        "image/png",
        include_str!("assets/logos/google-calendar.png.b64"),
    ),
    (
        "google-drive",
        "image/png",
        include_str!("assets/logos/google-drive.png.b64"),
    ),
    (
        "hubspot",
        "image/png",
        include_str!("assets/logos/hubspot.png.b64"),
    ),
    (
        "hyperframes",
        "image/png",
        include_str!("assets/logos/hyperframes.png.b64"),
    ),
    (
        "ibkr",
        "image/png",
        include_str!("assets/logos/ibkr.png.b64"),
    ),
    (
        "linear",
        "image/png",
        include_str!("assets/logos/linear.png.b64"),
    ),
    (
        "meltwater",
        "image/png",
        include_str!("assets/logos/meltwater.png.b64"),
    ),
    (
        "mixpanel",
        "image/png",
        include_str!("assets/logos/mixpanel.png.b64"),
    ),
    (
        "netlify",
        "image/png",
        include_str!("assets/logos/netlify.png.b64"),
    ),
    (
        "notion",
        "image/png",
        include_str!("assets/logos/notion.png.b64"),
    ),
    (
        "outlook",
        "image/png",
        include_str!("assets/logos/outlook.png.b64"),
    ),
    (
        "ringcentral-chat",
        "image/png",
        include_str!("assets/logos/ringcentral-chat.png.b64"),
    ),
    (
        "robinhood",
        "image/png",
        include_str!("assets/logos/robinhood.png.b64"),
    ),
    (
        "scalable-capital",
        "image/png",
        include_str!("assets/logos/scalable-capital.png.b64"),
    ),
    (
        "shopify",
        "image/png",
        include_str!("assets/logos/shopify.png.b64"),
    ),
    (
        "sp-global",
        "image/png",
        include_str!("assets/logos/sp-global.png.b64"),
    ),
    (
        "stripe",
        "image/png",
        include_str!("assets/logos/stripe.png.b64"),
    ),
    (
        "teams",
        "image/png",
        include_str!("assets/logos/teams.png.b64"),
    ),
    (
        "vercel",
        "image/png",
        include_str!("assets/logos/vercel.png.b64"),
    ),
    (
        "webull",
        "image/png",
        include_str!("assets/logos/webull.png.b64"),
    ),
    (
        "whop",
        "image/png",
        include_str!("assets/logos/whop.png.b64"),
    ),
    ("wix", "image/png", include_str!("assets/logos/wix.png.b64")),
    ("x", "image/png", include_str!("assets/logos/x.png.b64")),
];

use std::sync::OnceLock;

/// URL prefix of the logo files.
pub const PREFIX: &str = "/_ui/cn-logo/";

/// One decoded logo.
pub struct Logo {
    pub name: &'static str,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    /// `/_ui/cn-logo/<name>.<hash>.<ext>` (immutable).
    pub href: String,
    pub etag: String,
}

/// Standard base64; skips `#` comment lines and whitespace. `None` on bad input.
pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        for &c in line.trim().as_bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => continue,
                _ => return None,
            };
            acc = (acc << 6) | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
    }
    Some(out)
}

/// Every logo, decoded once.
pub fn all() -> &'static [Logo] {
    static L: OnceLock<Vec<Logo>> = OnceLock::new();
    L.get_or_init(|| {
        LOGOS
            .iter()
            .filter_map(|&(name, mime, text)| {
                let bytes = b64_decode(text)?;
                let h = crate::css::fnv64(&bytes);
                let ext = if mime == "image/svg+xml" {
                    "svg"
                } else {
                    "png"
                };
                Some(Logo {
                    name,
                    mime,
                    href: format!("{PREFIX}{name}.{:08x}.{ext}", h as u32),
                    etag: format!("\"{h:016x}\""),
                    bytes,
                })
            })
            .collect()
    })
}

/// The URL of a logo by name.
pub fn href(name: &str) -> Option<&'static str> {
    all()
        .iter()
        .find(|l| l.name == name)
        .map(|l| l.href.as_str())
}

/// The logo served at `path`, if any.
pub fn by_path(path: &str) -> Option<&'static Logo> {
    all().iter().find(|l| l.href == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_logo_decodes_to_a_real_image() {
        assert_eq!(all().len(), LOGOS.len());
        for l in all() {
            if l.mime == "image/png" {
                assert!(l.bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{}", l.name);
            } else {
                assert!(
                    String::from_utf8_lossy(&l.bytes).contains("<svg"),
                    "{}",
                    l.name
                );
                let s = String::from_utf8_lossy(&l.bytes).to_lowercase();
                assert!(!s.contains("<script") && !s.contains(" on") && !s.contains("href=\"http"));
            }
            assert!(by_path(&l.href).is_some());
        }
        assert_eq!(b64_decode("# c\nTWFu\nTWE=\n").unwrap(), b"ManMa");
        assert!(b64_decode("!!").is_none());
    }
}
