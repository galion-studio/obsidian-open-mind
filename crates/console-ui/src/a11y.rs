// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Accessibility checks and the small contrast fixes they require.
//!
//! The sky gradient is `linear-gradient(135deg, #B8E8FF 0%, #58C4FF 55%, #0B5FA5 100%)`.
//! Ink (`#0B0F14`) clears the ice and sky stops and fails the ocean stop (about 2.92:1).
//! A paper plate behind the glyph keeps the icon readable on every stop without
//! recolouring the gradient. Kickers that used `#6a6a6a` / `#8a8a86` were under
//! 4.5:1; this sheet points `--c-subtle` at mist on dark and paper-muted on light.

use crate::css;
use std::sync::OnceLock;

pub const INK: &str = "#0B0F14";
pub const PAPER: &str = "#F7F9FB";
pub const ICE: &str = "#B8E8FF";
pub const SKY: &str = "#58C4FF";
pub const OCEAN: &str = "#0B5FA5";
pub const MIST: &str = "#A7B6C4";
pub const PAPER_MUTED: &str = "#3F4C59";
/// Lightest mix of the research wash that still fails for mist text.
pub const WASH_FAILS: f64 = 0.38;
/// Mix used by the fix. Mist on this wash clears 4.5:1.
pub const WASH_OK: f64 = 0.22;

fn channel(hex: &str, i: usize) -> Option<f64> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    u8::from_str_radix(&h[i..i + 2], 16)
        .ok()
        .map(|c| f64::from(c) / 255.0)
}

fn linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of an `#RRGGBB` colour. `None` when the hex is not 6 digits.
pub fn luminance(hex: &str) -> Option<f64> {
    let r = linear(channel(hex, 0)?);
    let g = linear(channel(hex, 2)?);
    let b = linear(channel(hex, 4)?);
    Some(0.2126 * r + 0.7152 * g + 0.0722 * b)
}

/// WCAG 2.1 contrast ratio.
pub fn contrast(fg: &str, bg: &str) -> Option<f64> {
    let a = luminance(fg)?;
    let b = luminance(bg)?;
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    Some((hi + 0.05) / (lo + 0.05))
}

pub fn passes_text(ratio: f64) -> bool {
    ratio >= 4.5
}

pub fn passes_ui(ratio: f64) -> bool {
    ratio >= 3.0
}

/// sRGB mix: `(1 - t) * base + t * toward`.
pub fn mix(base: &str, toward: &str, t: f64) -> Option<String> {
    let mut out = String::from("#");
    for i in [0, 2, 4] {
        let a = channel(base, i)?;
        let b = channel(toward, i)?;
        let v = (a * (1.0 - t) + b * t).mul_add(255.0, 0.5).floor() as u8;
        out.push_str(&format!("{v:02X}"));
    }
    Some(out)
}

/// Contrast sheet. Linked after the page sheets so these declarations win.
/// It does not move, resize or recolour the gradient.
pub fn sheet() -> String {
    let mut out =
        String::from("/* Galion console UI (accessibility). Contrast and focus only. */\n");
    out.push_str(
        ":root{--c-subtle:#A7B6C4}\n\
html[data-theme=light]{--c-subtle:#3F4C59}\n\
@media (prefers-color-scheme:light){html:not([data-theme]){--c-subtle:#3F4C59}}\n\
.sr-only:focus,.sr-only:focus-visible{position:static;width:auto;height:auto;margin:.5rem;clip:auto;white-space:normal;background:#0B0F14;color:#E6EDF3;outline:2px solid #58C4FF;outline-offset:2px;padding:.5rem .75rem;z-index:80}\n\
.ask-voice .i{background:#F7F9FB;color:#0B0F14;border-radius:999px;box-shadow:0 0 0 3px #F7F9FB}\n\
.ask-voice:focus-visible,.ask-plus:focus-visible,.ask-input:focus-visible{outline:2px solid #0B0F14;outline-offset:2px;box-shadow:0 0 0 4px #F7F9FB}\n\
body:has(.ask-page){background:radial-gradient(90% 48% at 50% -14%,color-mix(in srgb,var(--c-ice) 22%,transparent),transparent 62%),var(--c-bg)}\n",
    );
    out
}

pub fn href() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        let h = css::fnv64(sheet().as_bytes());
        format!("/_ui/a11y.{h:016x}.css")
    })
}

pub fn cached() -> &'static str {
    static B: OnceLock<String> = OnceLock::new();
    B.get_or_init(sheet)
}

pub fn etag() -> &'static str {
    static E: OnceLock<String> = OnceLock::new();
    E.get_or_init(|| {
        let h = css::fnv64(cached().as_bytes());
        format!("\"{h:016x}\"")
    })
}

fn step(html: &str, i: usize) -> usize {
    i + html[i..].chars().next().map(char::len_utf8).unwrap_or(1)
}

/// Opening tags of controls, in document order, with a short accessible name.
pub fn focusables(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != b'<' {
            i = step(html, i);
            continue;
        }
        let rest = &html[i..];
        let kind = [
            "<a ",
            "<a>",
            "<button",
            "<input",
            "<textarea",
            "<select",
            "<summary",
        ]
        .iter()
        .find(|k| rest.starts_with(*k));
        if kind.is_none() {
            i = step(html, i);
            continue;
        }
        let end = rest.find('>').unwrap_or(0);
        let tag = &rest[..=end];
        if tag.starts_with("<input") && tag.contains("type=\"hidden\"") {
            i += end + 1;
            continue;
        }
        let name = attr(tag, "aria-label")
            .or_else(|| attr(tag, "href"))
            .unwrap_or_else(|| tag_name(tag).to_string());
        out.push(format!("{} {name}", tag_name(tag)));
        i += end + 1;
    }
    out
}

fn tag_name(tag: &str) -> &str {
    tag.trim_start_matches('<')
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let rest = &tag[start..];
    Some(rest[..rest.find('"')?].to_string())
}

pub fn positive_tabindex(html: &str) -> bool {
    html.split("tabindex=\"").skip(1).any(|s| {
        s.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
            > 0
    })
}

/// Inputs, textareas and selects with no accessible name.
pub fn unlabeled_fields(html: &str) -> Vec<String> {
    let mut missing = Vec::new();
    let mut labelled_ids = Vec::new();
    for part in html.split("<label").skip(1) {
        let tag = part.split('>').next().unwrap_or("");
        if let Some(id) = attr(&format!("<label{tag}>"), "for") {
            labelled_ids.push(id);
        }
    }
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut in_label = 0i32;
    while i + 1 < bytes.len() {
        let rest = &html[i..];
        if rest.starts_with("<label") {
            in_label += 1;
        } else if rest.starts_with("</label") {
            in_label -= 1;
        }
        if rest.starts_with("<input")
            || rest.starts_with("<textarea")
            || rest.starts_with("<select")
        {
            let end = rest.find('>').unwrap_or(0);
            let tag = &rest[..=end];
            let hidden = tag.contains("type=\"hidden\"");
            let named = attr(tag, "aria-label").is_some()
                || attr(tag, "aria-labelledby").is_some()
                || attr(tag, "id").is_some_and(|id| labelled_ids.iter().any(|l| l == &id))
                || in_label > 0;
            if !hidden && !named {
                missing.push(tag_name(tag).to_string());
            }
            i += end + 1;
            continue;
        }
        i = step(html, i);
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(fg: &str, bg: &str) -> f64 {
        contrast(fg, bg).unwrap()
    }

    #[test]
    fn restyle_pairs_clear_body_text() {
        // Pairs from the Galion Research restyle (draft PR 32).
        let sky_ink = ratio(INK, SKY);
        let ocean_paper = ratio(OCEAN, PAPER);
        let mist_surface = ratio(MIST, "#131A22");
        assert!(
            passes_text(sky_ink),
            "sky {SKY} with ink {INK} text is {sky_ink:.2}:1"
        );
        assert!(
            passes_text(ocean_paper),
            "ocean {OCEAN} on paper {PAPER} is {ocean_paper:.2}:1"
        );
        assert!(
            passes_text(mist_surface),
            "mist {MIST} on surface #131A22 is {mist_surface:.2}:1"
        );
        assert!(sky_ink > 9.0 && sky_ink < 10.5);
        assert!(ocean_paper > 6.0 && ocean_paper < 6.5);
        assert!(mist_surface > 8.0 && mist_surface < 9.0);
    }

    #[test]
    fn sky_gradient_stops_and_the_plate() {
        // The finding: ink on the ocean stop of the sky gradient fails WCAG.
        assert!(ratio(INK, ICE) > 14.0);
        assert!(ratio(INK, SKY) > 9.0);
        assert!(
            ratio(INK, OCEAN) < 3.0,
            "ink on ocean was expected to fail, got {}",
            ratio(INK, OCEAN)
        );
        assert!(!passes_ui(ratio(INK, OCEAN)));
        // The fix paints the glyph on a paper plate. The plate itself clears the ocean stop.
        assert!(passes_text(ratio(INK, PAPER)));
        assert!(passes_text(ratio(PAPER, OCEAN)));
        let s = sheet();
        // A SPDX tag inside this comment is read by comply as the file licence.
        assert!(!s.contains("SPDX-License-Identifier"));
        assert!(s.contains(".ask-voice .i{background:#F7F9FB;color:#0B0F14"));
        assert!(s.contains("body:has(.ask-page)"));
        assert!(s.contains("22%"));
        assert!(!s.contains("38%"));
    }

    #[test]
    fn wash_and_kicker_contrast() {
        let hot = mix(INK, ICE, WASH_FAILS).unwrap();
        let cool = mix(INK, ICE, WASH_OK).unwrap();
        assert!(
            !passes_text(ratio(MIST, &hot)),
            "mist on the 38% wash should fail"
        );
        assert!(passes_text(ratio(MIST, &cool)));
        assert!(!passes_text(ratio("#6a6a6a", "#1e1e1e")));
        assert!(passes_text(ratio(MIST, "#1e1e1e")));
        assert!(passes_text(ratio(MIST, INK)));
        assert!(!passes_text(ratio("#8a8a86", "#f6f6f4")));
        assert!(passes_text(ratio(PAPER_MUTED, "#f6f6f4")));
        assert!(passes_text(ratio(PAPER_MUTED, PAPER)));
        let s = sheet();
        assert!(s.contains("--c-subtle:#A7B6C4"));
        assert!(s.contains("--c-subtle:#3F4C59"));
        assert_eq!(s.matches('{').count(), s.matches('}').count());
    }

    #[test]
    fn focus_order_helper_skips_hidden_inputs() {
        let html = "<a href=\"#main\">Skip</a><input type=\"hidden\" name=\"x\"><input aria-label=\"Code\"><button type=\"submit\">Pair</button>";
        let f = focusables(html);
        assert_eq!(f[0], "a #main");
        assert!(f.iter().any(|s| s.contains("Code")));
        assert!(!f.iter().any(|s| s.contains("hidden")));
        assert!(!positive_tabindex(html));
        assert!(positive_tabindex("<a tabindex=\"2\" href=\"/\">x</a>"));
        assert!(unlabeled_fields("<input name=\"q\">").contains(&"input".to_string()));
        assert!(unlabeled_fields("<label>Name<input name=\"n\"></label>").is_empty());
        assert!(unlabeled_fields("<input aria-label=\"Filter\">").is_empty());
    }
}
