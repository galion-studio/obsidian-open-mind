// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/session` (Build session): port of console-ide
//! `src/routes/_console/session.tsx` (live tree, 2026-10-20 21:25 CEST).
//! console-port-5, wave 4 slot 139.
//!
//! Data: `GET /api/v1/session` (`{ok, items:[{name, bytes}]}`) and
//! `GET /api/v1/session?name=<file>` (the markdown text); both need the
//! paired phone (admin cookie, forwarded as for every room). The zip link
//! is `/api/v1/session-pack`. The front refuses the download without the
//! admin cookie (Android-safe headers are forced on the way out). The API
//! `/api/` proxy as before.
//!
//! Differences from the TSX, on purpose:
//! - a file is opened with a link (`/session?name=…`) instead of a click
//!   handler; the name must match the server's own rule
//!   (`^[A-Za-z0-9][A-Za-z0-9._-]*\.md$`, `session-docs.ts` `safeName`) or
//!   the page answers 400 and nothing is asked upstream;
//! - when the list or the file does not load the page says why (live,
//!   unpaired: `HTTP 401 (phone required)`) instead of "0 files";
//! - a very long file is cut at [`MAX_DOC_CHARS`] and the page says so.

use crate::html::{esc, url_enc};
use crate::icons::icon;
use crate::json::Value;

/// Longest file text rendered (chars).
pub const MAX_DOC_CHARS: usize = 200_000;

/// `session-docs.ts` `safeName`, without the `basename` step: a name with a
/// slash is refused outright.
pub fn name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 3
        && b.len() <= 255
        && b[0].is_ascii_alphanumeric()
        && name.ends_with(".md")
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Upstream path for one file.
pub fn doc_path(name: &str) -> String {
    format!("/api/v1/session?name={}", url_enc(name))
}

/// Why something did not load: `HTTP <code> (<error>)` or "did not answer".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    Down,
    Status(u16, String),
}

impl Miss {
    pub fn say(&self) -> String {
        match self {
            Miss::Down => "The console API did not answer.".into(),
            Miss::Status(c, e) if e.is_empty() => format!("HTTP {c}."),
            Miss::Status(c, e) => format!("HTTP {c} ({e})."),
        }
    }
}

/// One file of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub name: String,
    pub bytes: String,
}

/// TSX `text(value, fallback)`.
fn text(v: &Value, fallback: &str) -> String {
    match v {
        Value::Num(n) if n.is_finite() => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Value::Str(s) if s != "undefined" && s != "null" => s.clone(),
        _ => fallback.to_string(),
    }
}

/// The list items (TSX: `Array.isArray(j.items) ? j.items : []`).
pub fn docs_of(v: &Value) -> Vec<Doc> {
    v.get("items")
        .as_array()
        .iter()
        .map(|d| Doc {
            name: text(d.get("name"), ""),
            bytes: text(d.get("bytes"), "0"),
        })
        .collect()
}

/// What the page shows of an opened file.
pub enum Open<'a> {
    None,
    /// The name was refused before any upstream call.
    Refused,
    Doc(&'a str, Result<&'a str, Miss>),
}

pub fn body(list: Result<&[Doc], &Miss>, open: Open<'_>) -> String {
    let n = list.map(<[Doc]>::len).unwrap_or(0);
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Session</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">Build session</span></h1>",
        icon("session", "i i-md")
    ));
    match list {
        Ok(_) => out.push_str(&format!(
            "<p class=\"lede\">Notes from this pod. {n} files. No secrets in the pack.</p>"
        )),
        Err(m) => out.push_str(&format!(
            "<p class=\"lede\">Notes from this pod. No secrets in the pack.</p><p class=\"err\">The file list did not load: {}</p>",
            esc(&m.say())
        )),
    }
    out.push_str("<p class=\"lede mono\">/session</p><a href=\"/api/v1/session-pack\">Download zip</a></header>");
    if let Ok(items) = list {
        out.push_str("<ul class=\"rows\">");
        for d in items {
            let name = if name_ok(&d.name) {
                format!(
                    "<a class=\"mono ell\" href=\"/session?name={}\">{}</a>",
                    esc(&url_enc(&d.name)),
                    esc(&d.name)
                )
            } else {
                // The TSX shows it as a button that would fetch nothing useful.
                format!("<span class=\"mono ell\">{}</span>", esc(&d.name))
            };
            out.push_str(&format!(
                "<li>{name}<span class=\"mono tiny subtle\">{}</span></li>",
                esc(&d.bytes)
            ));
        }
        out.push_str("</ul>");
    }
    match open {
        Open::None => {}
        Open::Refused => out.push_str(
            "<p class=\"err\">That file name is not valid (letters, digits, . _ - and .md only). Nothing was asked.</p>",
        ),
        Open::Doc(name, got) => {
            out.push_str(&format!(
                "<article><p class=\"note mono\">{}</p>",
                esc(name)
            ));
            match got {
                Ok(t) => {
                    let t = if t == "undefined" { "" } else { t };
                    let cut: String = t.chars().take(MAX_DOC_CHARS).collect();
                    out.push_str(&format!("<pre class=\"code\">{}</pre>", esc(&cut)));
                    if cut.len() < t.len() {
                        out.push_str(&format!(
                            "<p class=\"note\">Cut at {MAX_DOC_CHARS} characters. The zip has the whole file.</p>"
                        ));
                    }
                }
                Err(m) => out.push_str(&format!(
                    "<p class=\"err\">The file did not load: {}</p>",
                    esc(&m.say())
                )),
            }
            out.push_str("</article>");
        }
    }
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn names_follow_safe_name() {
        for ok in ["00-summary.md", "a.md", "A_b-c.d.md", "9.md"] {
            assert!(name_ok(ok), "{ok}");
        }
        for bad in [
            "",
            ".md",
            "-a.md",
            ".hidden.md",
            "a.txt",
            "a.md/",
            "../a.md",
            "a/b.md",
            "a b.md",
            "a%2f.md",
            "a\n.md",
            "ü.md",
            "a.MD",
        ] {
            assert!(!name_ok(bad), "{bad:?}");
        }
        assert_eq!(
            doc_path("00-summary.md"),
            "/api/v1/session?name=00-summary.md"
        );
    }

    #[test]
    fn list_and_doc_render() {
        let v = parse(br#"{"ok":true,"items":[{"name":"00-summary.md","bytes":1234},{"name":"<x>.md","bytes":"undefined"},{"bytes":5}]}"#).unwrap();
        let d = docs_of(&v);
        assert_eq!(d.len(), 3);
        assert_eq!(d[1].bytes, "0");
        assert_eq!(d[2].name, "");
        let h = body(Ok(&d), Open::Doc("00-summary.md", Ok("# Hi <b>\n")));
        assert!(h.contains("3 files. No secrets in the pack."));
        assert!(h.contains("href=\"/session?name=00-summary.md\""));
        assert!(h.contains("<span class=\"mono ell\">&lt;x&gt;.md</span>"));
        assert!(h.contains("<pre class=\"code\"># Hi &lt;b&gt;\n</pre>"));
        assert!(h.contains("href=\"/api/v1/session-pack\""));
        assert!(!h.contains("<script") && !h.contains("<b>"));
        let u = body(Ok(&d), Open::Doc("a.md", Ok("undefined")));
        assert!(u.contains("<pre class=\"code\"></pre>"));
    }

    #[test]
    fn failures_say_why() {
        let m = Miss::Status(401, "phone required".into());
        let h = body(Err(&m), Open::None);
        assert!(h.contains("did not load: HTTP 401 (phone required)."));
        assert!(!h.contains("0 files"));
        let h = body(Ok(&[]), Open::Doc("a.md", Err(Miss::Down)));
        assert!(
            h.contains("0 files")
                && h.contains("The file did not load: The console API did not answer.")
        );
        assert!(body(Ok(&[]), Open::Refused).contains("Nothing was asked"));
        let long = "x".repeat(MAX_DOC_CHARS + 5);
        assert!(body(Ok(&[]), Open::Doc("a.md", Ok(&long))).contains("Cut at 200000"));
    }
}
