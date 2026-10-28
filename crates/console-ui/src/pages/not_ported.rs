// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Placeholder for console rooms that have no Rust page yet (only used when
//! no pre-built SPA is configured as a fallback).

use crate::html::esc;
use crate::icons::icon;
use crate::rooms::Room;

pub fn body(room: &Room, ported: &[&str]) -> String {
    let links: String = ported
        .iter()
        .map(|p| format!("<li class=\"fact\"><a href=\"{0}\">{0}</a></li>", esc(p)))
        .collect();
    format!(
        "<div class=\"page w42\"><p class=\"k\">Not in Rust yet</p><h1>{ic}<span class=\"ell\">{label}</span></h1>\
<p class=\"lede\">{line}</p><p class=\"note\">This room has not been ported to the Rust console yet. Ported rooms:</p>\
<ul class=\"facts\">{links}</ul></div>",
        ic = icon(room.icon, "i i-md"),
        label = esc(room.label),
        line = esc(room.line),
    )
}
