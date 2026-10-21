// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/status`: the public board (console-ide `src/routes/status.tsx`).
//!
//! Differences from the TSX, on purpose:
//! - the headline follows the real origin health (`/api/v1/health`); the TSX
//!   followed a browser-storage flag that a fresh browser never set, so it
//!   always said "Origins are dark." while listing hosts as online;
//! - a host whose service the desk pulse reports as not ok shows `offline`
//!   (the TSX showed catalog defaults only).

use super::{catalog_state, Pulse, Service};
use crate::html::esc;
use crate::icons::mark;
use crate::json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub online: usize,
    pub held: usize,
}

pub fn state_of(services: &[Service], pulses: &[Pulse], id: &str) -> &'static str {
    if pulses.iter().any(|p| p.id == id && !p.ok) {
        return "offline";
    }
    services
        .iter()
        .find(|s| s.id == id)
        .map(catalog_state)
        .unwrap_or("offline")
}

pub fn counts(services: &[Service], pulses: &[Pulse]) -> Counts {
    let mut c = Counts { online: 0, held: 0 };
    for s in services {
        match state_of(services, pulses, &s.id) {
            "online" => c.online += 1,
            "held" => c.held += 1,
            _ => {}
        }
    }
    c
}

/// Body of the status page (standalone, not inside the console shell).
pub fn body(
    healthy: Option<bool>,
    services: &[Service],
    domains: &Value,
    pulses: &[Pulse],
) -> String {
    let c = counts(services, pulses);
    let headline = match healthy {
        Some(true) => "Light stack is up.",
        Some(false) => "Origins are dark.",
        None => "Origin did not answer.",
    };
    let rows: String = domains
        .get("items")
        .as_array()
        .iter()
        .filter_map(|d| {
            let host = d.get("host").as_str()?;
            let role = d.get("role").as_str().unwrap_or("");
            let st = state_of(services, pulses, d.get("serviceId").as_str().unwrap_or(""));
            Some(format!(
                "<li><div><p class=\"mono\">{}</p><p>{}</p></div><span class=\"state\"><span class=\"dot {st}\"></span>{st}</span></li>",
                esc(host),
                esc(role)
            ))
        })
        .collect();
    let list = if rows.is_empty() {
        "<p class=\"note\">The domain list did not load from /api/v1/domains.</p>".to_string()
    } else {
        format!("<ul class=\"hosts\">{rows}</ul>")
    };
    format!(
        "<div class=\"board\"><header class=\"board-top\"><div>{m}<div><p>Galion</p><p class=\"k\">status.galion.app</p></div></div>\
<a href=\"/console\">Operator</a></header><main>\
<p class=\"k\">Public board</p><h1>{headline}</h1>\
<p class=\"lede\">{on} subsystems online · {held} held · Cloudflare still 530 until DNS is cut over.</p>\
{list}</main></div>",
        m = mark("i i-lg"),
        on = c.online,
        held = c.held,
    )
}
