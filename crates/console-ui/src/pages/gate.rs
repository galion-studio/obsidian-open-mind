// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Gate pages from console-ide `src/start.ts`: the pairing form (PAIR), the
//! quiet page for other hosts (SOON) and the developer links page (LINKS).
//!
//! The TSX pairing page used an inline script to POST JSON. Here the form
//! posts to `/_ui/pair` and the server relays the JSON call and the cookie,
//! so pairing works with JavaScript off.

use crate::html::{esc, url_enc};

pub fn pair(back: &str, error: Option<&str>) -> String {
    let err = error
        .map(|e| format!("<p class=\"err\" role=\"alert\">{}</p>", esc(e)))
        .unwrap_or_default();
    format!(
        "<main class=\"gate\"><p class=\"k\">Admin</p><h1>This phone</h1>\
<p class=\"lede\">One device. Enter the pairing code once. This phone keeps a sealed resume key so Android can return without the code. Other phones and browsers stay out.</p>\
<form method=\"post\" action=\"/_ui/pair?back={back}\">\
<input name=\"code\" type=\"password\" autocomplete=\"one-time-code\" autocapitalize=\"characters\" spellcheck=\"false\" aria-label=\"Pairing code\" required>\
<button class=\"btn primary\" type=\"submit\">Pair</button></form>{err}</main>",
        back = esc(&url_enc(back)),
    )
}

pub fn soon() -> String {
    "<main class=\"gate\"><p class=\"k\">Galion</p><h1>Soon online.</h1><p class=\"lede\">This host is quiet. The desk is not here.</p></main>"
        .to_string()
}

pub fn links() -> String {
    "<main class=\"gate\"><p class=\"k\">developer.galion.app</p><h1>Developer</h1>\
<p class=\"lede\">The desk stays on the phone. This page is only the door.</p>\
<ul class=\"facts\"><li><a href=\"https://github.com/galion-studio/community/discussions\">Discussions</a></li>\
<li><a href=\"https://github.com/galion-studio\">Docs · GitHub</a></li>\
<li><a href=\"https://status.galion.app\">Status</a></li></ul></main>"
        .to_string()
}

pub fn unavailable() -> String {
    "<main class=\"gate\"><p class=\"k\">Admin</p><h1>Console API is not answering</h1>\
<p class=\"lede\">The pairing check runs on the console API. It did not answer, so nothing is shown. Try again in a minute.</p></main>"
        .to_string()
}

/// Brand line per host for the `/soon` room (console-ide
/// `src/components/coming-soon.tsx` `COPY`).
pub const SOON_COPY: &[(&str, &str, &str)] = &[
    ("merlinelement.com", "Merlin Element", "In maintenance."),
    ("sentialink.com", "SentiaLink", "In maintenance."),
    ("galion.studio", "Galion Studio", "Coming soon."),
    ("galion.app", "Galion", "Coming soon."),
    ("mc.galion.studio", "GALION Play", "In maintenance."),
    ("play.galion.studio", "GALION Play", "In maintenance."),
    ("developer.galion.app", "NexusLang", "Coming soon."),
    ("docs.galion.studio", "Galion Docs", "Coming soon."),
    ("voice.galion.app", "Galion Voice", "In maintenance."),
    ("neural.galion.app", "Galion Neural", "In maintenance."),
    ("code.galion.app", "Galion Code", "In maintenance."),
];

/// The `/soon` room: the hibernation card for the host it is opened on
/// (`host` is already normalised). developer.galion.app gets the links.
pub fn coming_soon(host: &str) -> String {
    if host == "developer.galion.app" {
        return links();
    }
    let (name, line) = SOON_COPY
        .iter()
        .find(|(h, _, _)| *h == host)
        .map(|(_, n, l)| (*n, *l))
        .unwrap_or((if host.is_empty() { "Soon" } else { host }, "Coming soon."));
    format!(
        "<div class=\"hib\"><header><span class=\"hib-mark\" aria-hidden=\"true\"></span><p class=\"k\">{h}</p></header>\
<main><p class=\"k\">Hibernating</p><h1>{n}</h1><p class=\"lede\">{l}</p></main></div>",
        h = esc(host),
        n = esc(name),
        l = esc(line)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coming_soon_per_host() {
        assert!(coming_soon("sentialink.com").contains("<h1>SentiaLink</h1>"));
        assert!(coming_soon("sentialink.com").contains("In maintenance."));
        assert!(coming_soon("x.example").contains("<h1>x.example</h1>"));
        assert!(coming_soon("").contains("<h1>Soon</h1>"));
        assert!(coming_soon("developer.galion.app").contains("Discussions"));
        assert!(!coming_soon("<b>").contains("<b>"));
    }
}
