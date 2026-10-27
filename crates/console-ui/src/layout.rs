// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The page frame: `<head>`, the console shell (header, menu, dock) from
//! console-ide `ide-shell.tsx`, and the bare frame for gate pages.
//!
//! No JavaScript: the menu is a `<details>` element, the theme toggle is a
//! link to `/_ui/theme` that sets a cookie and redirects back.

use crate::html::{esc, url_enc};
use crate::icons::{icon, mark};
use crate::rooms::{self, Room, Zone, DOCK, ROOMS};

/// Per-response frame settings.
#[derive(Debug, Clone)]
pub struct Frame<'a> {
    pub title: &'a str,
    pub css_href: &'a str,
    /// Second stylesheet (overview `ov-*`), linked only on `/console`.
    pub extra_css_href: Option<&'a str>,
    /// `Some(true)` light, `Some(false)` dark (the `galion_theme` cookie);
    /// `None` follows the system (`prefers-color-scheme`).
    pub light: Option<bool>,
    /// Path of the page being rendered (dock/menu highlighting, theme back link).
    pub path: &'a str,
}

/// Origin indicator in the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Up,
    Down,
}

/// `body_attrs` is empty or starts with a space (e.g. ` class="x"`).
pub fn document(f: &Frame<'_>, body_attrs: &str, body: &str) -> String {
    let (attr, scheme, tc) = match f.light {
        Some(true) => (
            " data-theme=\"light\"",
            "light",
            format!(
                "<meta name=\"theme-color\" content=\"{}\">",
                crate::css::theme_color(true)
            ),
        ),
        Some(false) => (
            " data-theme=\"dark\"",
            "dark",
            format!(
                "<meta name=\"theme-color\" content=\"{}\">",
                crate::css::theme_color(false)
            ),
        ),
        None => (
            "",
            "dark light",
            format!(
                "<meta name=\"theme-color\" media=\"(prefers-color-scheme: light)\" content=\"{}\">\
<meta name=\"theme-color\" media=\"(prefers-color-scheme: dark)\" content=\"{}\">",
                crate::css::theme_color(true),
                crate::css::theme_color(false)
            ),
        ),
    };
    format!(
        "<!doctype html>\n<html lang=\"en\"{attr}><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\
<title>{title}</title>{tc}\
<meta name=\"color-scheme\" content=\"{scheme}\">\
<meta name=\"description\" content=\"Lightweight operator console for the Galion Studio fleet. API-first. CPU-safe.\">\
<link rel=\"icon\" type=\"image/svg+xml\" href=\"/_ui/favicon.svg\">\
<link rel=\"stylesheet\" href=\"{css}\">{extra}</head>\
<body{body_attrs}>{body}</body></html>\n",
        title = esc(f.title),
        css = esc(f.css_href),
        extra = match f.extra_css_href {
            Some(h) => format!(
                "<link rel=\"stylesheet\" href=\"{}\">",
                esc(h)
            ),
            None => String::new(),
        },
    )
}

fn theme_link(f: &Frame<'_>) -> String {
    let one = |to: &str, label: &str, ic: &str, class: &str| {
        format!(
            "<a class=\"iconbtn{class}\" href=\"/_ui/theme?to={to}&amp;back={back}\" aria-label=\"{label}\" rel=\"nofollow\">{i}</a>",
            back = esc(&url_enc(f.path)),
            i = icon(ic, "i")
        )
    };
    match f.light {
        Some(true) => one("dark", "Use dark", "moon", ""),
        Some(false) => one("light", "Use light", "sun", ""),
        // Following the system: offer the opposite of whatever it is; CSS
        // (`prefers-color-scheme`) shows exactly one of the two.
        None => format!(
            "{}{}",
            one("light", "Use light", "sun", " th-d"),
            one("dark", "Use dark", "moon", " th-l")
        ),
    }
}

fn room_tile(room: &Room, path: &str) -> String {
    let active = room.to.is_some_and(|to| rooms::on_room(path, to));
    let tile = format!(
        "<span class=\"tile\">{}</span><span class=\"ell\">{}</span>",
        icon(room.icon, "i"),
        esc(room.label)
    );
    let cur = if active { " aria-current=\"page\"" } else { "" };
    match (room.to, room.href) {
        (_, Some(href)) => format!(
            "<li><a class=\"room\" href=\"{}\">{tile}</a></li>",
            esc(href)
        ),
        (Some(to), None) => format!(
            "<li><a class=\"room\" href=\"{}\"{cur}>{tile}</a></li>",
            esc(to)
        ),
        (None, None) => format!("<li><span class=\"room\">{tile}</span></li>"),
    }
}

fn room_grid(title: &str, zone: Zone, path: &str, extra_class: &str) -> String {
    let tiles: String = ROOMS
        .iter()
        .filter(|r| r.zone == zone)
        .map(|r| room_tile(r, path))
        .collect();
    format!(
        "<section{extra_class}><h2 class=\"k\">{}</h2><ul class=\"rooms\">{tiles}</ul></section>",
        esc(title)
    )
}

/// The console shell around a page body.
pub fn console(f: &Frame<'_>, origin: Origin, main: &str) -> String {
    let room = rooms::current(f.path);
    let (dot, word) = match origin {
        Origin::Up => ("online", "up"),
        Origin::Down => ("offline", "down"),
    };
    let dock: String = DOCK
        .iter()
        .map(|(to, label)| {
            let cur = if rooms::on_room(f.path, to) {
                " aria-current=\"page\""
            } else {
                ""
            };
            format!(
                "<li><a href=\"{to}\"{cur}>{}{}</a></li>",
                icon(rooms::icon_of(to).unwrap_or("desk"), "i"),
                esc(label)
            )
        })
        .collect();
    let body = format!(
        "<div class=\"app\">\
<a href=\"#main\" class=\"sr-only\">Skip to console</a>\
<header class=\"top\"><div class=\"top-row\">{mark}\
<div class=\"top-title\"><p>{ricon}<span class=\"ell\">{label}</span></p><p>{line}</p></div>\
<span class=\"origin\"><span class=\"dot {dot}\"></span>{word}</span>{theme}\
<details class=\"menu\"><summary class=\"iconbtn\" aria-label=\"Menu\">{iopen}{iclose}</summary>\
<div class=\"sheet\">{up}{down}</div></details>\
</div></header>\
<div class=\"body\"><main id=\"main\" class=\"main\" tabindex=\"-1\">{main}</main></div>\
<nav class=\"dock\" aria-label=\"Primary\"><ul>{dock}</ul></nav></div>",
        mark = mark("i i-md"),
        ricon = icon(room.icon, "i i-sm"),
        label = esc(room.label),
        line = esc(room.line),
        theme = theme_link(f),
        iopen = icon("menu", "i i-open"),
        iclose = icon("close", "i i-close"),
        up = room_grid("Open", Zone::Up, f.path, ""),
        down = room_grid("Cold", Zone::Down, f.path, " class=\"cold\""),
    );
    document(f, "", &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_marks_current_page_and_escapes() {
        let f = Frame {
            title: "Galion <Console>",
            css_href: "/_ui/console.x.css",
            extra_css_href: None,
            light: Some(false),
            path: "/services",
        };
        let html = console(&f, Origin::Up, "<p>body</p>");
        assert!(html.contains("<title>Galion &lt;Console&gt;</title>"));
        assert!(html.contains("<a class=\"room\" href=\"/services\" aria-current=\"page\">"));
        assert!(html.contains("href=\"/_ui/theme?to=light&amp;back=/services\""));
        assert!(html.contains("data-theme=\"dark\""));
        assert!(!html.contains("<script"));
        assert_eq!(html.matches("<li><a href=").count(), DOCK.len());
    }
}
