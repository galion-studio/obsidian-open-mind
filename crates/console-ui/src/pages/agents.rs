// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/agents` (console-ide `src/routes/_console/agents.tsx`, live tree).
//!
//! The three buttons are a plain `<form method="post">`; the server sends
//! `POST /api/v1/command {"cmd":...}` upstream and renders the answer in the
//! output box, exactly what the TSX `fire()` showed, without JavaScript.
//!
//! Fixes over the TSX: "Pod" now reads `target.pod` (the TSX read a
//! top-level `pod` that `/api/v1/health` does not have, so it always showed
//! "—"); "Persist" is not in the health payload at all, so it says so
//! instead of printing a hard-coded "/workspace" with a green dot.

use super::tidy;
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// Commands the page may send (the TSX buttons).
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("health", "Probe origin", "primary"),
    ("boot", "Boot light", "ghost"),
    ("catalog", "Catalog", "ghost"),
];

pub fn allowed(cmd: &str) -> bool {
    COMMANDS.iter().any(|(c, _, _)| *c == cmd)
}

fn row(label: &str, value: &str, ok: bool) -> String {
    let tone = if ok { "online" } else { "held" };
    format!(
        "<li class=\"fact\"><span class=\"ell\">{}</span><span><span class=\"dot {tone}\"></span><span class=\"badge {tone}\"><span class=\"ell\">{}</span></span></span></li>",
        esc(label),
        esc(value)
    )
}

/// What the output box shows for a command answer (TSX: `json.ok ? json.data ?? json : json`).
pub fn note_of(answer: &Value) -> String {
    let shown = if answer.get("ok").as_bool() == Some(true) && !answer.get("data").is_null() {
        answer.get("data")
    } else {
        answer
    };
    tidy(&shown.to_json())
}

pub fn body(health: Option<&Value>, note: &str) -> String {
    let (origin, ok) = match health {
        Some(h) => (
            h.get("version")
                .as_str()
                .map(tidy)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "…".into()),
            h.get("status").as_str() == Some("healthy"),
        ),
        None => ("no answer".to_string(), false),
    };
    let pod = health
        .and_then(|h| {
            h.get("pod")
                .as_str()
                .or_else(|| h.get("target").get("pod").as_str())
        })
        .map(tidy)
        .filter(|s| !s.is_empty());
    let persist = health
        .and_then(|h| h.get("persist").as_str())
        .map(tidy)
        .filter(|s| !s.is_empty());
    let buttons: String = COMMANDS
        .iter()
        .map(|(cmd, label, variant)| {
            format!("<button class=\"btn {variant}\" type=\"submit\" name=\"cmd\" value=\"{cmd}\">{label}</button>")
        })
        .collect();
    let note = if note.is_empty() { "idle" } else { note };
    format!(
        "<div class=\"page w42\"><p class=\"k ell\">Operator agents · no VRAM</p>\
<h1>{ic}<span class=\"ell\">Govern</span></h1><ul class=\"facts\">{r1}{r2}{r3}</ul>\
<form class=\"actions\" method=\"post\" action=\"/agents\">{buttons}</form>\
<pre class=\"out\" aria-live=\"polite\">{note}</pre></div>",
        ic = icon("agents", "i i-md"),
        r1 = row("Origin", &origin, ok),
        r2 = match &persist {
            Some(p) => row("Persist", p, true),
            None => row("Persist", "not in /api/v1/health", false),
        },
        r3 = match &pod {
            Some(p) => row("Pod", p, true),
            None => row("Pod", "—", false),
        },
        note = esc(note),
    )
}
