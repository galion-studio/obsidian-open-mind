// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! "Ask anything" first screen for one.galion.app (2026-11-01 CET).
//!
//! One window, one gradient, one prompt field. The page (`ask/index.html`)
//! is compiled in and served at `/` on the V2 hosts. It talks to the agent
//! through the room's existing routes only (`/me`, `/pair`, `/admin/say`).
//! There is no new backend.
//!
//! 2026-11-01 CET, calm: the only control is a chat-visibility switch (off
//! by default, saved in localStorage). Off shows just the latest answer in
//! the centre and fades it after about a minute; on shows the full stream.
//! No menu: /room, /approvals, /pair and /account stay reachable by URL.
//!
//! The full room (history, files, voice, model, devices) moves to `/room`,
//! still read from `one/index-v2.html` on disk with the same serve-time
//! additions as before, plus [`plain_room`], which swaps the agent's
//! internal name out of the room's visible text.
//!
//! `ONE_ASK=off` (or `0`, `no`, `false`) puts the room back at `/`.

pub const PAGE: &str = include_str!("../ask/index.html");

/// True unless `ONE_ASK` is `off`, `0`, `no` or `false`.
pub fn enabled_by(v: Option<&str>) -> bool {
    !matches!(
        v.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("off") | Some("0") | Some("no") | Some("false")
    )
}

pub fn enabled() -> bool {
    enabled_by(std::env::var("ONE_ASK").ok().as_deref())
}

/// Visible room strings and what they read now. Code identifiers
/// (`"hermes"` service ids, header names) and `//` comments are untouched.
const ROOM_SWAPS: [(&str, &str); 10] = [
    (
        "<p class=\"k\">Galion One · Hermes</p>",
        "<p class=\"k\"><a href=\"/\" style=\"color:inherit;text-decoration:none\">&larr; Galion One</a></p>",
    ),
    ("aria-label=\"Hermes\"", "aria-label=\"Galion One\""),
    ("to talk to Hermes.", "to use Galion One."),
    ("placeholder=\"Ask Hermes anything\"", "placeholder=\"Ask anything…\""),
    ("? \"You\" : \"Hermes\"", "? \"You\" : \"Galion\""),
    ("\"Hermes is thinking\"", "\"Thinking\""),
    ("\"Hermes is still answering", "\"Still answering"),
    ("\"Hermes did not answer.", "\"No answer came back."),
    (">Hermes self-changes<", ">Self-changes<"),
    ("Hermes self-changes", "Self-changes"),
];

/// The room page with the agent's internal name taken out of visible text.
pub fn plain_room(html: &str) -> String {
    let mut out = html.to_string();
    for (from, to) in ROOM_SWAPS {
        out = out.replace(from, to);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text a person can see: tags, `<!-- -->` and `//` comment lines dropped.
    fn visible(html: &str) -> String {
        let mut out = String::new();
        for l in html.lines() {
            if l.trim_start().starts_with("//") {
                continue;
            }
            out.push_str(l);
            out.push('\n');
        }
        out
    }

    #[test]
    fn page_is_one_field_and_uses_the_room_routes() {
        assert!(PAGE.contains("placeholder=\"Ask anything&hellip;\""));
        assert!(!PAGE.to_ascii_lowercase().contains("hermes"));
        for route in ["\"/admin/say\"", "\"/me\"", "\"/pair\""] {
            assert!(PAGE.contains(route), "{route}");
        }
        assert!(PAGE.contains("prefers-reduced-motion:reduce"));
        // Private dictation only: the room's worklet and /stt/stream, no browser speech service.
        assert!(PAGE.contains("\"/stt/stream\"") && PAGE.contains("\"/stt/health\""));
        assert!(
            PAGE.contains("id=\"g1-worklet\"")
                && PAGE.contains("registerProcessor(\"galion-pcm16\"")
        );
        assert!(!PAGE.contains("SpeechRecognition") && !PAGE.contains("speechSynthesis"));
        assert!(
            !PAGE.contains("<link") && !PAGE.contains("@import") && !PAGE.contains("src=\"http")
        );
        assert!(PAGE.contains("</head>") && PAGE.contains("</body>"));
        assert!(PAGE.len() < 32 * 1024);
        let s = crate::pwa::with_install(PAGE);
        assert_eq!(s.matches("rel=\"manifest\"").count(), 1);
        assert_eq!(s.matches("id=\"g1-pwa\"").count(), 1);
        assert_eq!(s.matches("name=\"theme-color\"").count(), 1);
    }

    /// One control only: chat visibility, off by default, kept per device.
    /// No menu and no links; /room, /approvals, /pair, /account stay URL-only.
    #[test]
    fn one_switch_no_menu() {
        assert_eq!(PAGE.matches("role=\"switch\"").count(), 1);
        assert!(PAGE.contains("aria-label=\"Show chat\"") && PAGE.contains("title=\"Show chat\""));
        assert!(PAGE.contains("aria-checked=\"false\""));
        assert!(PAGE.contains("<body class=\"off\">"));
        assert!(PAGE.contains("localStorage.getItem(KEY) === \"on\""));
        assert_eq!(
            PAGE.matches("<button").count(),
            4,
            "switch, mic, send, pair"
        );
        for gone in [
            "href=\"/room\"",
            "href=\"/approvals\"",
            "/account",
            "aria-haspopup",
            "class=\"menu\"",
            "New conversation",
            "Install app",
            "/pair/invite",
            "/api/approvals/count",
        ] {
            assert!(!PAGE.contains(gone), "{gone}");
        }
        // The pairing field still comes up by itself.
        assert!(PAGE.contains("id=\"pairf\"") && PAGE.contains("pairf.hidden = false"));
    }

    /// The field is one pill: no scrollbar, resize handle, inner box or seam.
    #[test]
    fn field_is_one_clean_pill() {
        for want in [
            "resize:none;overflow-y:hidden;scrollbar-width:none;-webkit-appearance:none;appearance:none",
            "textarea::-webkit-scrollbar{display:none",
            "border:0;border-radius:0;outline:0;box-shadow:none;background:transparent",
            ".dock{flex:none;padding:.5rem 0 max(1rem,calc(env(safe-area-inset-bottom) + .5rem));background:none}",
            "caret-color:var(--am)",
        ] {
            assert!(PAGE.contains(want), "{want}");
        }
        assert!(!PAGE.contains("backdrop-filter"));
        assert!(!PAGE.contains("linear-gradient(to top,var(--ob)"));
    }

    /// 2026-11-01 ask fix: every question has an id (the server answers it
    /// once), waits are bounded, drops resume, and no line says "try again".
    #[test]
    fn asks_resume_and_never_say_try_again() {
        assert!(!PAGE.to_ascii_lowercase().contains("try again"));
        assert!(!PAGE.contains("=== \"Again\"") && !PAGE.contains("=== \"Wait\""));
        for want in [
            "id: j.id",
            "r.json.pending",
            "AbortController",
            "Reconnecting",
            "\"galion-one-pending\"",
            "resume()",
            "ok: j !== null",
            "addEventListener(\"online\", wake)",
        ] {
            assert!(PAGE.contains(want), "{want}");
        }
    }

    #[test]
    fn room_text_loses_the_name() {
        let real = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/galion-one-web/v2/index-v2.html"
        ))
        .unwrap();
        let s = plain_room(&real);
        assert!(!visible(&s).contains("Hermes"), "{}", {
            visible(&s)
                .lines()
                .filter(|l| l.contains("Hermes"))
                .collect::<Vec<_>>()
                .join("\n")
        });
        assert!(s.contains("placeholder=\"Ask anything…\""));
        assert!(s.contains("href=\"/\""));
        // Service ids stay as they are.
        assert!(s.contains("rows[i].id === \"hermes\""));
        assert_eq!(plain_room(&s), s);
    }

    #[test]
    fn switch() {
        assert!(enabled_by(None) && enabled_by(Some("on")));
        assert!(!enabled_by(Some("off")) && !enabled_by(Some(" 0 ")) && !enabled_by(Some("No")));
    }
}
