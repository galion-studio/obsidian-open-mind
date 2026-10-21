// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/guardian` (console-ide `src/routes/_console/guardian.tsx`, live tree
//! 2026-10-20 21:25 CEST): Guardian status from
//! `GET /api/v3/guardian/status`, plus Scan and Heal secrets.
//!
//! Without JavaScript:
//! - Scan is a same-origin-checked `<form method=post>`; the server sends
//!   `POST /api/v3/guardian/scan {}` upstream;
//! - Heal asks for the Defender PIN in a password field of the same kind of
//!   form (the TSX used `window.prompt`); the server sends it upstream as
//!   `X-Galion-Pin` on `POST /api/v3/guardian/heal {}`. The PIN is never
//!   rendered, logged or kept; an empty PIN sends nothing (TSX: `if (!pin)
//!   return`).
//!
//! Fix over the TSX: when the status call fails (live today: 401 `phone
//! required` for an unpaired browser) the page says so instead of showing
//! "… · score — · incidents 0".

use super::p2::{js_num, text};
use crate::html::esc;
use crate::json::Value;

/// Largest PIN accepted from the form.
pub const MAX_PIN: usize = 64;

/// Actions the page may send.
pub const ACTIONS: &[&str] = &["scan", "heal"];

/// A PIN is sent only if it is 1..=64 visible ASCII characters.
pub fn pin_ok(pin: &str) -> bool {
    !pin.is_empty() && pin.len() <= MAX_PIN && pin.bytes().all(|b| b.is_ascii_graphic())
}

/// Why the status did not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    /// Upstream answered with this status and (if JSON) its `error`.
    Status(u16, String),
    /// No answer, or not JSON.
    Down,
}

pub fn body(status: Result<&Value, &Miss>, note: &str) -> String {
    let null = Value::Null;
    let d = status.unwrap_or(&null);
    let st = text(d.get("status"), "…");
    let score = d
        .get("score")
        .as_f64()
        .map(js_num)
        .unwrap_or_else(|| "—".into());
    let inc = d
        .get("activeIncidents")
        .as_f64()
        .map(js_num)
        .unwrap_or_else(|| "0".into());
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Guardian</p>");
    out.push_str(&format!(
        "<p class=\"lede\">Watch · 24/7 · {} · score {} · incidents {}</p></header>",
        esc(&st),
        esc(&score),
        esc(&inc)
    ));
    match status {
        Err(Miss::Status(code, err)) => out.push_str(&format!(
            "<p class=\"note t-bad\">Guardian status did not load: HTTP {code}{}.</p>",
            if err.is_empty() {
                String::new()
            } else {
                format!(" ({})", esc(err))
            }
        )),
        Err(Miss::Down) => out.push_str(
            "<p class=\"note t-bad\">Guardian status did not load: the console API did not answer.</p>",
        ),
        Ok(_) => {}
    }
    out.push_str(
        "<form class=\"actions\" method=\"post\" action=\"/guardian\">\
<button class=\"btn ghost\" type=\"submit\" name=\"act\" value=\"scan\">Scan</button></form>\
<form class=\"actions pin\" method=\"post\" action=\"/guardian\">\
<label for=\"g-pin\">Defender PIN</label>\
<input id=\"g-pin\" type=\"password\" name=\"pin\" autocomplete=\"off\" maxlength=\"64\" required>\
<button class=\"btn ghost\" type=\"submit\" name=\"act\" value=\"heal\">Heal secrets</button></form>",
    );
    if !note.is_empty() {
        out.push_str(&format!("<p class=\"note\">{}</p>", esc(note)));
    }
    out.push_str("<section class=\"sec\"><h2>Secret mode 400</h2><ul class=\"rows\">");
    for row in d.get("secrets").as_array() {
        let ok = row.get("ok").as_bool() == Some(true);
        out.push_str(&format!(
            "<li><span>{}</span><span class=\"{}\">{}</span></li>",
            esc(&text(row.get("name"), "")),
            if ok { "muted" } else { "t-bad" },
            esc(&text(row.get("mode"), ""))
        ));
    }
    let lib = d.get("library");
    out.push_str(&format!(
        "</ul></section><section class=\"sec\"><h2>Session library · {}</h2><ul class=\"rows scroll\">",
        lib.get("count").as_f64().map(js_num).unwrap_or_else(|| "0".into())
    ));
    for f in lib.get("files").as_array() {
        out.push_str(&format!("<li><span>{}</span></li>", esc(&text(f, ""))));
    }
    out.push_str("</ul></section>");
    let errors = d.get("errors").as_array();
    if !errors.is_empty() {
        out.push_str("<section class=\"sec\"><ul class=\"rows\">");
        for e in errors {
            out.push_str(&format!(
                "<li><span>{}</span><span class=\"muted\">{}</span></li>",
                esc(&text(e.get("service"), "")),
                esc(&text(e.get("error"), "down"))
            ));
        }
        out.push_str("</ul></section>");
    }
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn renders_status() {
        let v = parse(br#"{"status":"watching","score":97,"activeIncidents":1,
"secrets":[{"name":"a.env","mode":"400","ok":true},{"name":"b<.env","mode":"644","ok":false}],
"library":{"count":2,"files":["s1.md","s2.md"]},"errors":[{"service":"tuner"},{"service":"x","error":"timeout"}]}"#).unwrap();
        let b = body(Ok(&v), "");
        for n in [
            "Watch · 24/7 · watching · score 97 · incidents 1",
            "<span>a.env</span><span class=\"muted\">400</span>",
            "<span>b&lt;.env</span><span class=\"t-bad\">644</span>",
            "Session library · 2",
            "<li><span>s2.md</span></li>",
            "<span>tuner</span><span class=\"muted\">down</span>",
            "<span class=\"muted\">timeout</span>",
            "type=\"password\" name=\"pin\" autocomplete=\"off\"",
            "value=\"scan\"",
            "value=\"heal\"",
        ] {
            assert!(b.contains(n), "missing {n}");
        }
        assert!(!b.contains("did not load"));
    }

    #[test]
    fn says_why_it_is_empty() {
        let b = body(Err(&Miss::Status(401, "phone required".into())), "");
        assert!(b.contains("Watch · 24/7 · … · score — · incidents 0"));
        assert!(b.contains("HTTP 401 (phone required)."));
        assert!(body(Err(&Miss::Down), "scan: HTTP 200").contains("scan: HTTP 200"));
        assert!(!b.contains("Session library · 1"));
    }

    #[test]
    fn pins() {
        assert!(pin_ok("1234"));
        assert!(!pin_ok(""));
        assert!(!pin_ok("12 34"));
        assert!(!pin_ok("12\r\nX-Evil: 1"));
        assert!(!pin_ok(&"9".repeat(65)));
    }
}
