// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/markers` (console-ide `src/routes/_console/markers.tsx`, live tree
//! 2026-10-20 21:25 CEST): the 42-deep event ring from `GET /api/v1/markers`,
//! newest first, and a Ping button.
//!
//! Without JavaScript:
//! - Ping is a same-origin-checked `<form method=post>`; the server sends the
//!   TSX's fixed body [`PING`] to `POST /api/v1/markers` (nothing from the
//!   form reaches the upstream body) and renders the ring again;
//! - no 4 s poll and no SSE heartbeat rail (`/api/v1/markers/stream` needs
//!   `EventSource`): a Refresh link.
//!
//! Differences from the TSX: times are on the Zagreb clock with the zone
//! named (the TSX printed UTC `toISOString().slice(11, 19)` without a zone),
//! and the header says how many events the ring holds out of its cap.

use super::p3::{cap, miss, plain, s, zagreb_hms};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// The Ping body, fixed in the binary (markers.tsx `ping()`).
pub const PING: &str = r#"{"zone":"active","kind":"event","source":"desk","note":"manual ping"}"#;

/// Ring size when the answer does not say (`MAX` in `lib/markers.ts`).
pub const RING: usize = 42;

/// Kinds `lib/markers.ts` knows; anything else is shown as `event`.
const KINDS: &[&str] = &["heartbeat", "boot", "hold", "origin", "dns", "event"];

fn kind(m: &Value) -> &str {
    let k = s(m, "kind");
    if KINDS.contains(&k) {
        k
    } else {
        "event"
    }
}

fn row(m: &Value) -> String {
    let seq = m
        .get("seq")
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| format!("{}", n as i64))
        .unwrap_or_else(|| "—".into());
    let tone = if s(m, "zone") == "chill" {
        "chill"
    } else {
        "neutral"
    };
    let t = m
        .get("t")
        .as_f64()
        .map(zagreb_hms)
        .unwrap_or_else(|| "—".into());
    format!(
        "<li class=\"p3-mk\"><span class=\"p3-mkl\"><span class=\"mono tiny subtle\">{}</span><span class=\"badge {tone}\">{}</span>\
<span class=\"mono tiny\">{}</span><span class=\"muted p3-note\">{}</span></span><span class=\"mono tiny subtle\">{}</span></li>",
        esc(&seq),
        esc(kind(m)),
        esc(&cap(&plain(s(m, "source")), 64)),
        esc(&cap(&plain(s(m, "note")), 300)),
        esc(&t)
    )
}

pub fn body(ring: Result<&Value, (Option<u16>, String)>, note: &str) -> String {
    let mut out = format!(
        "<div class=\"page w46\"><header><p class=\"k\">Markers</p><h1>{}<span class=\"ell\">What the desk recorded</span></h1>\
<p class=\"lede\">Cheap, structured ticks. Heartbeats stream. Events persist in a 42-deep ring for the analytics system later. No warehouse yet.</p>",
        icon("markers", "i i-md")
    );
    let (count, capn) = match &ring {
        Ok(v) => (
            v.get("items").as_array().len(),
            v.get("cap")
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n as usize)
                .unwrap_or(RING),
        ),
        Err(_) => (0, RING),
    };
    out.push_str(&format!(
        "<p class=\"lede\">Ring of {capn} · {count} stored · <a href=\"/markers\">Refresh</a></p></header>"
    ));
    out.push_str(
        "<form class=\"actions\" method=\"post\" action=\"/markers\"><input type=\"hidden\" name=\"op\" value=\"ping\"><button class=\"btn primary\" type=\"submit\">Ping</button></form>",
    );
    if !note.is_empty() {
        out.push_str(&format!(
            "<p class=\"note\">{}</p>",
            esc(&cap(&plain(note), 200))
        ));
    }
    match ring {
        Ok(v) => {
            let items = v.get("items").as_array();
            out.push_str("<ul class=\"p3-list\">");
            if items.is_empty() {
                out.push_str("<li class=\"p3-mk muted\">No stored events yet. Heartbeats are live on the rail.</li>");
            }
            for m in items.iter().rev() {
                out.push_str(&row(m));
            }
            out.push_str("</ul>");
        }
        Err((code, err)) => out.push_str(&miss("The marker ring", code, &err)),
    }
    out.push_str("</div>");
    out
}

/// The note after a Ping (`POST /api/v1/markers` answers 201 with the marker).
pub fn ping_note(status: u16, answer: Option<&Value>) -> String {
    match (status, answer) {
        (201 | 200, Some(m)) if m.get("seq").as_f64().is_some() => format!(
            "Ping stored as #{}.",
            m.get("seq").as_f64().unwrap_or(0.0) as i64
        ),
        _ => format!("Ping refused: HTTP {status}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn ping_body_is_the_tsx_one() {
        let v = json::parse(PING.as_bytes()).unwrap();
        assert_eq!(v.get("zone").as_str(), Some("active"));
        assert_eq!(v.get("kind").as_str(), Some("event"));
        assert_eq!(v.get("source").as_str(), Some("desk"));
        assert_eq!(v.get("note").as_str(), Some("manual ping"));
    }

    #[test]
    fn empty_live_ring() {
        let v = json::parse(br#"{"count":0,"cap":42,"items":[]}"#).unwrap();
        let h = body(Ok(&v), "");
        assert!(h.contains("Ring of 42 · 0 stored"));
        assert!(h.contains("No stored events yet."));
        assert!(h.contains("action=\"/markers\""));
        assert!(!h.contains("<script"));
    }

    #[test]
    fn newest_first_zagreb_and_escaped() {
        // 1792552685000 ms = 2026-10-21 03:18:05 UTC = 05:18:05 CEST.
        let v = json::parse(br#"{"count":2,"cap":42,"items":[{"seq":42,"t":1792552685000,"zone":"active","kind":"boot","source":"desk","note":"first"},{"seq":43,"t":1792552690000,"zone":"chill","kind":"weird","source":"<api>","note":"x\ny"}]}"#).unwrap();
        let h = body(Ok(&v), "Ping stored as #43.");
        let a = h.find(">43<").unwrap();
        let b = h.find(">42<").unwrap();
        assert!(a < b, "newest first");
        assert!(h.contains("05:18:05 CEST"));
        assert!(
            h.contains("<span class=\"badge chill\">event</span>"),
            "unknown kind -> event"
        );
        assert!(h.contains("<span class=\"badge neutral\">boot</span>"));
        assert!(h.contains("&lt;api&gt;"));
        assert!(h.contains("x y"));
        assert!(h.contains("Ring of 42 · 2 stored"));
        assert!(h.contains("Ping stored as #43."));
    }

    #[test]
    fn notes_and_misses() {
        let m = json::parse(br#"{"seq":44,"t":1}"#).unwrap();
        assert_eq!(ping_note(201, Some(&m)), "Ping stored as #44.");
        assert_eq!(ping_note(500, None), "Ping refused: HTTP 500.");
        let h = body(Err((None, String::new())), "");
        assert!(h.contains("The marker ring did not load: the console API did not answer."));
    }
}
