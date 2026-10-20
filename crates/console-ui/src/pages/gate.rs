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
<p class=\"lede\">One device. Enter the pairing code once. Other phones and browsers stay out.</p>\
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
