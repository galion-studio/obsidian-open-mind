// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/connectors` page body (owner t453u). Sections of rows: initials icon,
//! name, one-line description, state, and a Connect / Manage action; an
//! "Add custom connector" form (name, https URL or local command, auth).
//! Zero JavaScript: `<details>` disclosures and same-origin form posts that
//! carry the CSRF token. Token inputs are `type=password`, never pre-filled;
//! a saved token is only ever shown as "saved".

use crate::connectors::{Auth, Catalog, Connector, State, Store};
use crate::html::esc;

/// Everything the page needs.
pub struct View<'a> {
    pub catalog: &'a Catalog,
    pub customs: &'a [Connector],
    /// `None`: `--connectors-dir` not set (read-only screen).
    pub store: Option<&'a Store>,
    pub csrf: &'a str,
    /// `--connectors-apply`: the Hermes MCP client that reads the file is live.
    pub apply: bool,
    /// One line above the list (already plain text).
    pub notice: Option<(bool, String)>,
    /// "Last saved" on the Zagreb clock.
    pub saved: Option<String>,
}

fn hidden(csrf: &str, op: &str, id: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"op\" value=\"{op}\">\
<input type=\"hidden\" name=\"id\" value=\"{}\">",
        esc(csrf),
        esc(id)
    )
}

fn state_badge(s: State, has_secret: bool) -> String {
    let (cls, word) = match s {
        State::Connected if has_secret => ("cn-on", "Connected · token saved"),
        State::Connected | State::Builtin => ("cn-on", "Connected"),
        State::Off => ("cn-off", "Disconnected"),
        State::Available => ("cn-off", "Not connected"),
        State::NeedsSignIn => ("cn-wait", "Needs sign-in setup"),
    };
    format!("<span class=\"cn-st {cls}\">{word}</span>")
}

fn token_form(v: &View<'_>, c: &Connector, button: &str) -> String {
    let lab = if c.token_label.is_empty() {
        "Token"
    } else {
        c.token_label.as_str()
    };
    format!(
        "<form method=\"post\" action=\"/connectors\" class=\"cn-form\">{h}\
<label>{lab}<input type=\"password\" name=\"token\" autocomplete=\"off\" spellcheck=\"false\" required minlength=\"8\" maxlength=\"4096\"></label>\
<p class=\"tiny muted\">Stored in a private file on the server (0600). Never shown again. Starts read-only: writes off.</p>\
<button class=\"btn primary\" type=\"submit\">{button}</button></form>",
        h = hidden(v.csrf, "connect", &c.id),
        lab = esc(lab),
        button = esc(button),
    )
}

fn action(v: &View<'_>, c: &Connector, s: State) -> String {
    let Some(_) = v.store else {
        return "<span class=\"tiny muted\">Read-only here</span>".into();
    };
    let target = c.url.as_deref().or(c.command.as_deref()).unwrap_or("");
    let facts = format!("<p class=\"tiny muted cn-mono\">{}</p>", esc(target));
    match s {
        State::Builtin => format!(
            "<details class=\"cn-det\"><summary class=\"btn\">Manage</summary><div class=\"cn-pop\">{facts}\
<p class=\"tiny\">{}</p></div></details>",
            esc(&c.note)
        ),
        State::NeedsSignIn => format!(
            "<details class=\"cn-det\"><summary class=\"btn\">Set up</summary><div class=\"cn-pop\">{facts}\
<p class=\"tiny\">{}</p>\
<p class=\"tiny\">Sign-in (OAuth) is not built into Hermes yet, so this connector cannot be connected here. Nothing has been saved.</p>\
<button class=\"btn\" type=\"button\" disabled>Sign in (coming later)</button></div></details>",
            esc(&c.note)
        ),
        State::Connected => {
            let renew = if c.auth == Auth::Token {
                token_form(v, c, "Replace token")
            } else {
                String::new()
            };
            format!(
                "<details class=\"cn-det\"><summary class=\"btn\">Manage</summary><div class=\"cn-pop\">{facts}\
<p class=\"tiny\">Read-only (writes off).</p>{renew}\
<form method=\"post\" action=\"/connectors\" class=\"cn-form\">{h}\
<button class=\"btn\" type=\"submit\">Disconnect</button></form></div></details>",
                h = hidden(v.csrf, "disconnect", &c.id)
            )
        }
        State::Available | State::Off => {
            let inner = if c.auth == Auth::Token {
                token_form(v, c, "Connect")
            } else {
                format!(
                    "<form method=\"post\" action=\"/connectors\" class=\"cn-form\">{}\
<button class=\"btn primary\" type=\"submit\">Connect</button></form>",
                    hidden(v.csrf, "connect", &c.id)
                )
            };
            format!(
                "<details class=\"cn-det\"><summary class=\"btn\">Connect</summary><div class=\"cn-pop\">{facts}{inner}</div></details>"
            )
        }
    }
}

fn row(v: &View<'_>, c: &Connector) -> String {
    let s = match v.store {
        Some(st) => st.state(c),
        None => match c.auth {
            Auth::Builtin => State::Builtin,
            Auth::OAuth => State::NeedsSignIn,
            _ => State::Available,
        },
    };
    let has_secret = v.store.is_some_and(|st| st.has_secret(&c.id));
    let custom = if c.custom {
        " <span class=\"cn-st cn-off\">Custom</span>"
    } else {
        ""
    };
    format!(
        "<li class=\"cn-row\" id=\"c-{id}\"><span class=\"cn-ic\" aria-hidden=\"true\">{ini}</span>\
<div class=\"cn-txt\"><p class=\"cn-name\">{name}{custom}</p><p class=\"cn-blurb\">{blurb}</p>{badge}</div>\
<div class=\"cn-act\">{act}</div></li>",
        id = esc(&c.id),
        ini = esc(&c.initials),
        name = esc(&c.name),
        blurb = esc(&c.blurb),
        badge = state_badge(s, has_secret),
        act = action(v, c, s),
    )
}

fn add_form(v: &View<'_>) -> String {
    if v.store.is_none() {
        return String::new();
    }
    format!(
        "<details class=\"cn-add\"><summary class=\"btn\">+ Add custom connector</summary>\
<form method=\"post\" action=\"/connectors\" class=\"cn-form cn-pop\">{h}\
<label>Name<input name=\"name\" required maxlength=\"48\" placeholder=\"My server\" autocomplete=\"off\"></label>\
<label>MCP server URL or local command<input name=\"target\" required maxlength=\"700\" placeholder=\"https://example.com/mcp\" autocomplete=\"off\" spellcheck=\"false\"></label>\
<p class=\"tiny muted\">URL must be https://. A local command must be in /opt/galion/rust/bin/ with \"mcp\" in its name; arguments after spaces.</p>\
<fieldset class=\"cn-auth\"><legend class=\"tiny\">Authentication</legend>\
<label class=\"cn-radio\"><input type=\"radio\" name=\"auth\" value=\"none\" checked> None</label>\
<label class=\"cn-radio\"><input type=\"radio\" name=\"auth\" value=\"token\"> Bearer token / API key</label></fieldset>\
<label>Token (if any)<input type=\"password\" name=\"token\" autocomplete=\"off\" spellcheck=\"false\" maxlength=\"4096\"></label>\
<label>Token variable (optional)<input name=\"env\" maxlength=\"64\" placeholder=\"MY_SERVER_TOKEN\" autocomplete=\"off\"></label>\
<p class=\"tiny muted\">Starts read-only (writes off). OAuth sign-in is not built yet.</p>\
<button class=\"btn primary\" type=\"submit\">Add connector</button></form></details>",
        h = hidden(v.csrf, "add", "")
    )
}

fn apply_bar(v: &View<'_>) -> String {
    let Some(store) = v.store else {
        return "<p class=\"note\">Connector storage is not configured on this console (no <code>--connectors-dir</code>); the list is read-only.</p>".into();
    };
    let saved = v
        .saved
        .as_deref()
        .map(|s| format!(" Last saved {}.", esc(s)))
        .unwrap_or_default();
    let _ = store;
    if v.apply {
        format!(
            "<div class=\"cn-apply\"><p class=\"tiny\">Hermes loads connectors when it starts.{saved}</p>\
<form method=\"post\" action=\"/connectors\">{}<button class=\"btn\" type=\"submit\">Apply (restarts Hermes)</button></form></div>",
            hidden(v.csrf, "apply", "")
        )
    } else {
        format!(
            "<div class=\"cn-apply\"><p class=\"tiny\">Saved connectors take effect once the Hermes MCP client update is deployed (not live yet); then Apply restarts Hermes to load them.{saved}</p></div>"
        )
    }
}

/// The page body.
pub fn body(v: &View<'_>) -> String {
    let mut out = String::from(
        "<section class=\"cn\"><div class=\"cn-head\"><div><h1 class=\"cn-h\">Connectors</h1>\
<p class=\"lede\">Each connector is an MCP server Hermes can use. New connectors start read-only.</p></div></div>",
    );
    if let Some((ok, msg)) = &v.notice {
        out.push_str(&format!(
            "<p class=\"cn-msg {}\" role=\"status\">{}</p>",
            if *ok { "cn-ok" } else { "err" },
            esc(msg)
        ));
    }
    out.push_str(&apply_bar(v));
    out.push_str(&add_form(v));
    for s in &v.catalog.sections {
        let rows: String = v
            .catalog
            .connectors
            .iter()
            .filter(|c| c.section == s.id)
            .map(|c| row(v, c))
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "<h2 class=\"k cn-sec\">{}</h2><ul class=\"cn-list\">{rows}</ul>",
            esc(&s.title)
        ));
    }
    if !v.customs.is_empty() {
        let rows: String = v.customs.iter().map(|c| row(v, c)).collect();
        out.push_str(&format!(
            "<h2 class=\"k cn-sec\">Custom</h2><ul class=\"cn-list\">{rows}</ul>"
        ));
    }
    out.push_str("</section>");
    out
}
