// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/connectors` page body (owner t453u; layout from Mate's reference
//! screenshots, 2026-10-30 12:58 CET). Dark screen: back arrow and title,
//! subtitle, a rounded "Search connectors..." box (GET `?q=`, filtered on
//! the server), then grey section labels over rounded card groups. Each row
//! is a neutral monogram, the name, and "Connect" on the right; tapping it
//! opens a panel (`<details>`, zero JavaScript) with the real next step.
//! Connected lists only what is really wired. Token inputs are
//! `type=password`, never pre-filled; a saved token only ever reads "saved".

use crate::connectors::{wired, Auth, Catalog, Connector, State, Store};
use crate::html::esc;

/// Everything the page needs.
pub struct View<'a> {
    pub catalog: &'a Catalog,
    pub customs: &'a [Connector],
    /// `None`: `--connectors-dir` not set (read-only screen).
    pub store: Option<&'a Store>,
    pub csrf: &'a str,
    /// One line above the list (already plain text).
    pub notice: Option<(bool, String)>,
    /// "Last saved" on the Zagreb clock.
    pub saved: Option<String>,
    /// Search text (`?q=`), trimmed.
    pub q: &'a str,
    /// Brand logos read at start (`--connectors-logo-dir`).
    pub logos: &'a [crate::logos::Logo],
}

const BACK_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"24\" height=\"24\" aria-hidden=\"true\"><path d=\"M20 12H5M11 5l-7 7 7 7\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/></svg>";
const SEARCH_SVG: &str = "<svg viewBox=\"0 0 24 24\" width=\"22\" height=\"22\" aria-hidden=\"true\"><circle cx=\"11\" cy=\"11\" r=\"7\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"/><path d=\"M16.5 16.5L21 21\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\"/></svg>";

fn hidden(csrf: &str, op: &str, id: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"op\" value=\"{op}\">\
<input type=\"hidden\" name=\"id\" value=\"{}\">",
        esc(csrf),
        esc(id)
    )
}

/// State of one connector (also without storage: read-only screen).
pub fn state_of(v: &View<'_>, c: &Connector) -> State {
    match v.store {
        Some(st) => st.state(c),
        None => match c.auth {
            Auth::Builtin => State::Builtin,
            Auth::OAuth => State::NeedsSignIn,
            Auth::Unverified => State::Unavailable,
            _ => State::Available,
        },
    }
}

/// Really wired: built in, or saved while Hermes loads the servers file.
pub fn is_connected(s: State) -> bool {
    matches!(s, State::Connected | State::Builtin)
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
<p class=\"cn-fine\">Stored in a private file on the server (0600). Never shown again. Starts read-only: writes off.</p>\
<button class=\"cn-btn cn-pri\" type=\"submit\">{button}</button></form>",
        h = hidden(v.csrf, "connect", &c.id),
        lab = esc(lab),
        button = esc(button),
    )
}

fn facts(c: &Connector) -> String {
    let target = c.url.as_deref().or(c.command.as_deref()).unwrap_or("");
    let mut s = String::new();
    if !c.blurb.is_empty() {
        s.push_str(&format!("<p>{}</p>", esc(&c.blurb)));
    }
    if !target.is_empty() {
        s.push_str(&format!(
            "<p class=\"cn-fine cn-mono\">MCP server: {}</p>",
            esc(target)
        ));
    }
    if !c.source.is_empty() {
        s.push_str(&format!(
            "<p class=\"cn-fine\">Checked against the vendor docs: {}</p>",
            esc(&c.source)
        ));
    }
    if !c.note.is_empty() {
        s.push_str(&format!("<p class=\"cn-fine\">{}</p>", esc(&c.note)));
    }
    s
}

fn panel(v: &View<'_>, c: &Connector, s: State) -> String {
    let info = if v.store.is_some_and(|st| st.hand_configured(&c.id)) {
        let tok = if c.auth == Auth::Token {
            format!(
                " Its token ({}) is saved to its own private file.",
                esc(&c.token_env)
            )
        } else {
            String::new()
        };
        format!(
            "<p>{}</p><p class=\"cn-fine\">Already set up in the Hermes MCP servers file (read-only, writes off).{tok}</p>",
            esc(&c.blurb)
        )
    } else {
        facts(c)
    };
    if !wired(c) {
        return format!(
            "<div class=\"cn-panel\">{info}<p class=\"cn-fine\">Coming soon: this connector is not wired up in Hermes yet.</p></div>"
        );
    }
    let disconnect = format!(
        "<form method=\"post\" action=\"/connectors\" class=\"cn-form\">{}\
<button class=\"cn-btn\" type=\"submit\">Disconnect</button></form>",
        hidden(v.csrf, "disconnect", &c.id)
    );
    let act = match s {
        State::Unavailable => "<p class=\"cn-fine\">Not available yet: no MCP endpoint for this connector has been checked, so nothing can be saved. If you run your own MCP server for it, use Add your own custom connector.</p>".to_string(),
        State::NeedsSignIn => "<p class=\"cn-wait\">Needs sign-in setup</p>\
<p class=\"cn-fine\">This connector signs in with OAuth, which Hermes does not have yet. Nothing has been saved.</p>\
<button class=\"cn-btn\" type=\"button\" disabled>Sign in (coming later)</button>".to_string(),
        State::Builtin => "<p class=\"cn-fine\">Built in to Hermes; this screen does not change it.</p>".to_string(),
        _ if v.store.is_none() => "<p class=\"cn-fine\">Read-only here: connector storage is not configured on this console.</p>".to_string(),
        State::Connected => {
            let renew = if c.auth == Auth::Token { token_form(v, c, "Replace token") } else { String::new() };
            format!("<p class=\"cn-fine\">Connected, read-only (writes off).</p>{renew}{disconnect}")
        }
        State::Saved => {
            let renew = if c.auth == Auth::Token { token_form(v, c, "Replace token") } else { String::new() };
            format!("<p class=\"cn-wait\">Saved, not live yet</p><p class=\"cn-fine\">Saved after Hermes last started. Hermes loads it at its next restart, which this screen never does. Read-only (writes off).</p>{renew}{disconnect}")
        }
        State::Available | State::Off => {
            if c.auth == Auth::Token {
                token_form(v, c, "Connect")
            } else {
                format!(
                    "<form method=\"post\" action=\"/connectors\" class=\"cn-form\">{}\
<button class=\"cn-btn cn-pri\" type=\"submit\">Connect</button></form>",
                    hidden(v.csrf, "connect", &c.id)
                )
            }
        }
    };
    format!("<div class=\"cn-panel\">{info}{act}</div>")
}

fn right_label(s: State) -> &'static str {
    match s {
        State::Connected | State::Builtin => "",
        State::Saved => "Saved",
        _ => "Connect",
    }
}

fn icon(v: &View<'_>, c: &Connector) -> String {
    match crate::logos::href(v.logos, &c.logo) {
        Some(h) => format!(
            "<img class=\"cn-ic cn-logo\" src=\"{}\" alt=\"\" width=\"36\" height=\"36\" loading=\"lazy\">",
            esc(h)
        ),
        None => format!(
            "<span class=\"cn-ic\" aria-hidden=\"true\">{}</span>",
            esc(&c.initials)
        ),
    }
}

fn row(v: &View<'_>, c: &Connector, s: State) -> String {
    let soon = !wired(c);
    let lab = if soon { "" } else { right_label(s) };
    let soon = if soon {
        "<span class=\"cn-soon\">Coming soon</span>"
    } else {
        ""
    };
    let go = if lab.is_empty() {
        String::new()
    } else {
        format!("<span class=\"cn-go\">{lab}</span>")
    };
    format!(
        "<details class=\"cn-row\" id=\"c-{id}\"><summary>{ic}\
<span class=\"cn-name\">{name}{soon}</span>{go}</summary>{panel}</details>",
        id = esc(&c.id),
        ic = icon(v, c),
        name = esc(&c.name),
        panel = panel(v, c, s),
    )
}

fn add_row(v: &View<'_>) -> String {
    let inner = if v.store.is_none() {
        "<div class=\"cn-panel\"><p class=\"cn-fine\">Read-only here: connector storage is not configured on this console.</p></div>".to_string()
    } else {
        format!(
            "<form method=\"post\" action=\"/connectors\" class=\"cn-form cn-panel\">{h}\
<label>Name<input name=\"name\" required maxlength=\"48\" placeholder=\"My server\" autocomplete=\"off\"></label>\
<label>MCP server URL or local command<input name=\"target\" required maxlength=\"700\" placeholder=\"https://example.com/mcp\" autocomplete=\"off\" spellcheck=\"false\"></label>\
<p class=\"cn-fine\">{cmd_help}</p>\
<fieldset class=\"cn-auth\"><legend>Authentication</legend>\
<label class=\"cn-radio\"><input type=\"radio\" name=\"auth\" value=\"none\" checked> None</label>\
<label class=\"cn-radio\"><input type=\"radio\" name=\"auth\" value=\"token\"> Bearer token / API key</label></fieldset>\
<label>Token (if any)<input type=\"password\" name=\"token\" autocomplete=\"off\" spellcheck=\"false\" maxlength=\"4096\"></label>\
<label>Token variable (optional)<input name=\"env\" maxlength=\"64\" placeholder=\"MY_SERVER_TOKEN\" autocomplete=\"off\"></label>\
<p class=\"cn-fine\">Starts read-only (writes off). OAuth sign-in is not built yet.</p>\
<button class=\"cn-btn cn-pri\" type=\"submit\">Add connector</button></form>",
            h = hidden(v.csrf, "add", ""),
            cmd_help = cmd_help(v),
        )
    };
    format!(
        "<details class=\"cn-row cn-addrow\" id=\"c-add\"><summary><span class=\"cn-ic cn-plus\" aria-hidden=\"true\">+</span>\
<span class=\"cn-name\">Add your own custom connector</span></summary>{inner}</details>"
    )
}

fn cmd_help(v: &View<'_>) -> String {
    match v.store.and_then(|s| s.cmd_dir.as_deref()) {
        Some(d) => format!(
            "URL must be https://. A local command must be directly in {} with \"mcp\" in its name; arguments after spaces.",
            esc(d)
        ),
        None => "URL must be https://. Local commands are off on this console.".into(),
    }
}

fn matches(c: &Connector, q: &str) -> bool {
    q.is_empty() || c.name.to_lowercase().contains(&q.to_lowercase())
}

fn group(title: &str, rows: &str) -> String {
    format!(
        "<h2 class=\"cn-lab\">{}</h2><div class=\"cn-group\">{rows}</div>",
        esc(title)
    )
}

/// The page body.
pub fn body(v: &View<'_>) -> String {
    let mut out = format!(
        "<main class=\"cn\"><header class=\"cn-top\"><a class=\"cn-back\" href=\"/console\" aria-label=\"Back to the console\">{BACK_SVG}</a>\
<h1 class=\"cn-h\">Connectors</h1></header>\
<p class=\"cn-sub\">Connectors let Hermes use external tools and data sources.</p>\
<form class=\"cn-search\" method=\"get\" action=\"/connectors\" role=\"search\">{SEARCH_SVG}\
<input type=\"search\" name=\"q\" value=\"{q}\" placeholder=\"Search connectors...\" aria-label=\"Search connectors\" maxlength=\"64\" autocomplete=\"off\"></form>",
        q = esc(v.q),
    );
    if let Some((ok, msg)) = &v.notice {
        out.push_str(&format!(
            "<p class=\"cn-msg {}\" role=\"status\">{}</p>",
            if *ok { "cn-ok" } else { "cn-err" },
            esc(msg)
        ));
    }
    let all: Vec<(&Connector, State)> = v
        .catalog
        .connectors
        .iter()
        .chain(v.customs.iter())
        .map(|c| (c, state_of(v, c)))
        .collect();
    let mut shown = 0usize;
    let connected: String = all
        .iter()
        .filter(|(c, s)| is_connected(*s) && matches(c, v.q))
        .map(|(c, s)| row(v, c, *s))
        .collect();
    if !connected.is_empty() {
        shown += 1;
        out.push_str(&group("Connected", &connected));
    }
    for sec in &v.catalog.sections {
        let rows: String = all
            .iter()
            .filter(|(c, s)| {
                !c.custom && c.section == sec.id && !is_connected(*s) && matches(c, v.q)
            })
            .map(|(c, s)| row(v, c, *s))
            .collect();
        if !rows.is_empty() {
            shown += 1;
            out.push_str(&group(&sec.title, &rows));
        }
    }
    let mut custom: String = all
        .iter()
        .filter(|(c, s)| c.custom && !is_connected(*s) && matches(c, v.q))
        .map(|(c, s)| row(v, c, *s))
        .collect();
    if v.q.is_empty() || matches_add(v.q) {
        custom.push_str(&add_row(v));
    }
    if !custom.is_empty() {
        shown += 1;
        out.push_str(&group("Custom Connector", &custom));
    }
    if shown == 0 {
        out.push_str("<p class=\"cn-none\">No connectors match.</p>");
    }
    out.push_str(&status_note(v));
    out.push_str("</main>");
    out
}

fn matches_add(q: &str) -> bool {
    "add your own custom connector".contains(&q.to_lowercase())
}

fn status_note(v: &View<'_>) -> String {
    if v.store.is_none() {
        return "<p class=\"cn-foot\">Connector storage is not configured on this console, so the list is read-only.</p>".into();
    }
    let saved = v
        .saved
        .as_deref()
        .map(|s| format!(" Last saved {}.", esc(s)))
        .unwrap_or_default();
    let live = if v.store.is_some_and(|s| s.hermes_file.is_some()) {
        "Connectors are read from and saved to the live Hermes MCP servers file. Hermes loads changes at its next restart; this screen never restarts it."
    } else {
        "Connectors are saved here only; this console is not pointed at the Hermes MCP servers file."
    };
    format!("<p class=\"cn-foot\">{live}{saved}</p>")
}
