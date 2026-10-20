// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/services` (console-ide `src/routes/_console/services.tsx`, live tree).
//!
//! Same grouping as the TSX: light services that are not probed down go to
//! "Up" (online first), heavy ones and probed-down light ones go to "Down",
//! and pulse rows the catalog does not know are listed when down.
//!
//! Not carried over: the per-row Start/Stop button. In the TSX it only
//! flipped a value in browser storage (`zustand persist`) and never reached
//! a service, so it showed a state that nothing on the pod had.

use super::{catalog_state, Pulse, Service};
use crate::html::esc;
use crate::icons::icon;

/// Services the desk keeps cold on this CPU pod (TSX `COLD`).
pub const COLD: &[&str] = &["whisper", "voice", "vision"];

pub fn cold(id: &str) -> bool {
    COLD.iter().any(|c| c.eq_ignore_ascii_case(id))
}

fn probed_down(id: &str, pulses: &[Pulse]) -> bool {
    cold(id) || pulses.iter().any(|p| p.id == id && !p.ok)
}

fn where_of(s: &Service) -> String {
    let mut w = match s.port {
        Some(p) => format!(":{p}"),
        None => String::new(),
    };
    if !s.hosts.is_empty() {
        w.push_str(" · ");
        w.push_str(&s.hosts.join(" · "));
    }
    if let Some(r) = &s.repo {
        w.push_str(" · ");
        w.push_str(r);
    }
    w
}

fn live_name(id: &str) -> String {
    let mut c = id.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// (up, down, extras) ids in display order.
pub fn groups<'a>(
    services: &'a [Service],
    pulses: &'a [Pulse],
) -> (Vec<&'a Service>, Vec<&'a Service>, Vec<&'a Pulse>) {
    let mut up: Vec<&Service> = services
        .iter()
        .filter(|s| !s.heavy && !probed_down(&s.id, pulses))
        .collect();
    // Stable sort, online first (TSX `rank`).
    up.sort_by_key(|s| if catalog_state(s) == "online" { 0 } else { 1 });
    let down: Vec<&Service> = services
        .iter()
        .filter(|s| s.heavy || probed_down(&s.id, pulses))
        .collect();
    let mut extras: Vec<&Pulse> = pulses
        .iter()
        .filter(|p| !services.iter().any(|s| s.id == p.id) && (cold(&p.id) || !p.ok))
        .collect();
    extras.sort_by(|a, b| a.id.cmp(&b.id));
    (up, down, extras)
}

pub fn body(services: &[Service], pulses: &[Pulse], pulse_ok: bool) -> String {
    let (up, down, extras) = groups(services, pulses);
    let mut up_html = String::new();
    for s in &up {
        up_html.push_str(&format!(
            "<div class=\"svc\"><div><span class=\"ell\">{name}</span><span class=\"meta\"><span>{st}</span><span>{kind}</span></span></div>\
<div><span class=\"ell\">{sum}</span><span class=\"ell where\">{wh}</span></div></div>",
            name = esc(&s.name),
            st = catalog_state(s),
            kind = esc(&s.kind),
            sum = esc(&s.summary),
            wh = esc(&where_of(s)),
        ));
    }
    let mut down_html = String::new();
    for s in &down {
        if s.heavy {
            let st = if probed_down(&s.id, pulses) {
                "down"
            } else {
                "held"
            };
            down_html.push_str(&format!(
                "<div class=\"svc\"><div><span class=\"ell\">{}</span><span class=\"muted\">{st}</span></div><div><span class=\"ell muted\">{}</span></div></div>",
                esc(&s.name),
                esc(&s.summary)
            ));
        } else {
            down_html.push_str(&format!(
                "<div class=\"svc\"><div><span class=\"ell\">{name}</span><span class=\"meta\"><span>down</span><span>{kind}</span></span></div>\
<div><span class=\"ell\">{sum}</span><span class=\"ell where\">{wh}</span></div></div>",
                name = esc(&s.name),
                kind = esc(&s.kind),
                sum = esc(&s.summary),
                wh = esc(&where_of(s)),
            ));
        }
    }
    for p in &extras {
        let n = live_name(&p.id);
        if n.is_empty() {
            continue;
        }
        down_html.push_str(&format!(
            "<div class=\"svc\"><div><span class=\"ell\">{}</span><span class=\"muted\">down</span></div></div>",
            esc(&n)
        ));
    }
    if services.is_empty() {
        up_html.push_str("<p class=\"note\">The catalog did not load from /api/v1/services.</p>");
    }
    let pulse_note = if pulse_ok {
        format!("Desk pulse: {} services reported.", pulses.len())
    } else {
        "Desk pulse did not answer; cold services are still listed as down.".to_string()
    };
    format!(
        "<div class=\"page w46\"><header><p class=\"subtle\">Services</p>\
<h1>{ic}<span class=\"ell\">API world first</span></h1>\
<p><a href=\"/analytics\">The nerve sample is on Nerve.</a></p>\
<p class=\"lede\">Light subsystems can run on this CPU pod. Heavy ones — generation, voice, search cluster, game runtime — stay held on purpose.</p></header>\
<section><h2>Up</h2>{up_html}</section><section><h2>Down</h2>{down_html}</section>\
<p class=\"note\">{pulse_note} States come from the catalog and the desk pulse; nothing is started or stopped from this page.</p></div>",
        ic = icon("services", "i i-md"),
    )
}
