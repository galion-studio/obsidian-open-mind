// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/md/$name` (console-ide `src/routes/_console/md.$name.tsx` plus the
//! `/md/user` / `/md/persona` branch of the phone middleware in
//! `src/start.ts`, live tree 2026-10-20 21:25 CEST).
//!
//! The TSX route itself was a stub (`Hello "/_console/md/$name"!`); the real
//! pages were rendered by the middleware for a paired admin: `USER.md`
//! (`/opt/galion/hermes/memories/USER.md`) and `persona.md`
//! (`/var/lib/memory-tencentdb/persona.md`), escaped in a `<pre>` with two
//! tab links and "name · size bytes · mtime UTC".
//!
//! Here: only the two fixed names map to files (no path from the URL ever
//! reaches the file system); the mtime is on the Zagreb clock with the zone
//! named; the gate (pairing / loopback) runs first like every room. Fix over
//! the TSX: on a loopback host the middleware skipped the md branch and
//! showed the stub; here loopback gets the file too. Any other name is a 404
//! page listing the two files.

use super::p4::zagreb;
use crate::html::esc;

/// The fixed `$name` values and their titles.
pub const NAMES: &[(&str, &str)] = &[("user", "USER.md"), ("persona", "persona.md")];

/// Title for a `$name`, `None` for anything else.
pub fn title_of(name: &str) -> Option<&'static str> {
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// One file as read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdFile {
    pub bytes: u64,
    pub mtime: Option<i64>,
    pub text: String,
    /// The text was cut at the read cap.
    pub cut: bool,
}

fn tabs(active: &str) -> String {
    let mut out = String::from("<nav class=\"p4-tabs\" aria-label=\"Files\">");
    for (n, t) in NAMES {
        out.push_str(&format!(
            "<a href=\"/md/{n}\"{}>{t}</a>",
            if *n == active {
                " aria-current=\"page\""
            } else {
                ""
            }
        ));
    }
    out.push_str("</nav>");
    out
}

/// `file` is the read result; `Err` carries why it is not shown.
pub fn body(name: &str, file: Result<&MdFile, &str>) -> String {
    let Some(title) = title_of(name) else {
        return format!(
            "<div class=\"page w68\">{}<p class=\"note p4-bad\">No such file. Only USER.md and persona.md are shown here.</p></div>",
            tabs("")
        );
    };
    let mut out = format!("<div class=\"page w68\">{}", tabs(name));
    match file {
        Ok(f) => {
            out.push_str(&format!(
                "<p class=\"note\">{} · {} bytes · {}</p>",
                esc(title),
                f.bytes,
                f.mtime.map(zagreb).unwrap_or_else(|| "time unknown".into())
            ));
            if f.cut {
                out.push_str("<p class=\"note\">Long file: only the start is shown.</p>");
            }
            out.push_str(&format!(
                "<pre class=\"p4-pre p4-md\">{}</pre>",
                esc(&f.text)
            ));
        }
        Err(why) => out.push_str(&format!(
            "<p class=\"note p4-bad\">{} is not shown: {}.</p>",
            esc(title),
            esc(why)
        )),
    }
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fixed() {
        assert_eq!(title_of("user"), Some("USER.md"));
        assert_eq!(title_of("persona"), Some("persona.md"));
        for bad in ["", "USER", "../etc/passwd", "user/", "persona.md", "x"] {
            assert_eq!(title_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn renders_escaped_with_zagreb_time() {
        let f = MdFile {
            bytes: 34_907,
            mtime: Some(1_792_352_400),
            text: "# P <script>alert(1)</script> & 'q'".into(),
            cut: false,
        };
        let h = body("persona", Ok(&f));
        assert!(h.contains("persona.md · 34907 bytes · 2026-10-18 21:40 CEST (+02:00)"));
        assert!(h.contains("CEST (+02:00)"));
        assert!(h.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &#39;q&#39;"));
        assert!(!h.contains("<script"));
        assert!(h.contains("<a href=\"/md/persona\" aria-current=\"page\">persona.md</a>"));
        assert!(h.contains("<a href=\"/md/user\">USER.md</a>"));
        assert!(!h.contains("only the start"));
        let cut = MdFile { cut: true, ..f };
        assert!(body("persona", Ok(&cut)).contains("only the start"));
    }

    #[test]
    fn errors_and_unknown() {
        let h = body("user", Err("permission denied"));
        assert!(h.contains("USER.md is not shown: permission denied."));
        let h = body("<x>", Err("ignored"));
        assert!(h.contains("No such file"));
        assert!(!h.contains("&lt;x&gt;") && !h.contains("<x>"));
    }
}
