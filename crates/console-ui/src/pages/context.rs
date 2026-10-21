// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/context` (console-ide `src/routes/_console/context.tsx`, live tree
//! 2026-10-20 21:25 CEST): the model window, the session road, token use,
//! tool counts and Hermes research, from galion-desk `GET /desk`
//! (`127.0.0.1:8646`, the same source as the TSX server function
//! `desk-foundation.ts pull()`).
//!
//! Differences from the TSX, on purpose:
//! - no 4 s poll (needs JS): the page is read once per request and has a
//!   Refresh link; the refresh strategy waits on owner decision 1;
//! - stamps are on the Zagreb clock (the TSX used the browser's zone) with
//!   the zone named once in the Road heading;
//! - when the desk does not answer the page says so (the TSX silently
//!   showed "— live · no model · idle");
//! - the tool bars are `<progress>` elements (the CSP forbids inline
//!   `style=` widths).

use super::p2::{compact, gb, num, stamp, text, zone_label};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// TSX `shortTitle`: long ids become `first8…last4`.
pub fn short_title(title: &str) -> String {
    let n = title.chars().count();
    if n > 18 {
        let head: String = title.chars().take(8).collect();
        let tail: String = title.chars().skip(n - 4).collect();
        format!("{head}…{tail}")
    } else {
        title.to_string()
    }
}

fn stop(title: &str, meta: &str, body: &str) -> String {
    format!(
        "<li><p title=\"{}\">{}</p><p>{}</p><p>{}</p></li>",
        esc(title),
        esc(&short_title(title)),
        esc(meta),
        esc(body)
    )
}

fn live_or_dash(n: f64) -> String {
    if n == 0.0 {
        "—".into()
    } else {
        text(&Value::Num(n), "—")
    }
}

/// Render the room. `desk` is the `GET /desk` JSON, `None` when galion-desk
/// did not answer (or no `--desk` address is configured).
pub fn body(desk: Option<&Value>) -> String {
    let null = Value::Null;
    let d = desk.unwrap_or(&null);
    let mind = d.get("mindpack");
    let live = num(d.get("ctx"));
    let window_n = num(mind.get("context").get("model_window"));
    let model = text(d.get("model"), "no model");
    let vram = gb(num(d.get("vram")));
    let mut out = String::from("<div class=\"page w46\"><header>");
    out.push_str(&format!(
        "<p class=\"k\"><a href=\"/console\">{}Desk</a></p>",
        icon("desk", "i i-sm")
    ));
    out.push_str(&format!(
        "<p class=\"lede\">{} live{} · {} · {}</p></header>",
        esc(&live_or_dash(live)),
        if window_n != 0.0 {
            format!(" · {} window", text(&Value::Num(window_n), "0"))
        } else {
            String::new()
        },
        esc(&model),
        esc(&vram)
    ));
    if desk.is_none() {
        out.push_str(
            "<p class=\"note\">The desk foundation is not answering (galion-desk GET /desk). Numbers below are empty, not zero.</p>",
        );
    }

    // Road.
    let road = mind.get("road").as_array();
    let zone = road
        .iter()
        .map(|r| num(r.get("at")))
        .find(|a| *a != 0.0)
        .map(zone_label)
        .unwrap_or("CEST/CET");
    out.push_str(&format!(
        "<section class=\"sec\" aria-label=\"Roadmap\"><h2>Road <span class=\"subtle\">· times Europe/Zagreb ({zone})</span></h2><ol class=\"road\">"
    ));
    let line_up = d.get("line").as_bool() == Some(true);
    out.push_str(&stop(
        "Now",
        &format!(
            "{} · {} context · {}",
            if line_up { "Line up" } else { "Line down" },
            live_or_dash(live),
            vram
        ),
        &text(d.get("model"), "No model loaded"),
    ));
    let ctx = mind.get("context");
    out.push_str(&stop(
        "Cache",
        &format!(
            "{} sessions · {} turns",
            text(&Value::Num(num(ctx.get("cached_sessions"))), "0"),
            text(&Value::Num(num(ctx.get("cached_turns"))), "0")
        ),
        &text(mind.get("mind").get("line"), "No memory written."),
    ));
    for r in road {
        out.push_str(&stop(
            &text(r.get("id"), "session"),
            &format!(
                "{} · {} msgs · {}",
                stamp(num(r.get("at"))),
                text(r.get("messages"), "0"),
                text(r.get("source"), "session")
            ),
            &text(r.get("model"), "—"),
        ));
    }
    out.push_str("</ol></section>");

    // Tokens.
    let cache = mind.get("cache");
    let roles = mind.get("roles");
    out.push_str(&format!(
        "<section class=\"sec\" aria-label=\"Tokens\"><h2>Tokens</h2>\
<p class=\"small\">{} in · {} out · {} cache read · {} reasoning</p>\
<p class=\"small\">{} user · {} assistant · {} tool messages</p><ul class=\"rows\">",
        compact(num(cache.get("input"))),
        compact(num(cache.get("output"))),
        compact(num(cache.get("read"))),
        compact(num(cache.get("reasoning"))),
        text(&Value::Num(num(roles.get("user"))), "0"),
        text(&Value::Num(num(roles.get("assistant"))), "0"),
        text(&Value::Num(num(roles.get("tool"))), "0"),
    ));
    for m in mind.get("models").as_array() {
        out.push_str(&format!(
            "<li><span class=\"ell\">{}</span><span class=\"muted tiny\">{} calls · {} in · {} out · {} read</span></li>",
            esc(&text(m.get("name"), "")),
            esc(&text(m.get("calls"), "0")),
            compact(num(m.get("input"))),
            compact(num(m.get("output"))),
            compact(num(m.get("read"))),
        ));
    }
    out.push_str("</ul></section>");

    // Tools.
    let tools = mind.get("tools").as_array();
    let max_tool = tools
        .iter()
        .map(|t| num(t.get("n")))
        .fold(1.0_f64, f64::max);
    out.push_str("<section class=\"sec\" aria-label=\"Tools\"><h2>Tools</h2><ul class=\"bars\">");
    for t in tools {
        let pct = ((num(t.get("n")) / max_tool) * 100.0).round().max(4.0);
        out.push_str(&format!(
            "<li><span class=\"n\">{}</span><progress class=\"bar\" max=\"100\" value=\"{}\">{}%</progress><span class=\"ell\">{}</span></li>",
            esc(&text(t.get("n"), "0")),
            pct as u32,
            pct as u32,
            esc(&text(t.get("name"), "")),
        ));
    }
    out.push_str("</ul></section>");

    // Research.
    let m = mind.get("mind");
    out.push_str(&format!(
        "<section class=\"sec\" aria-label=\"Research\"><h2>Hermes research</h2><p class=\"small\">{} sessions · {} messages</p><ul class=\"rows\">",
        text(&Value::Num(num(m.get("sessions"))), "0"),
        text(&Value::Num(num(m.get("messages"))), "0"),
    ));
    for h in mind.get("history").as_array() {
        out.push_str(&format!(
            "<li><span class=\"ell tiny\"><span class=\"mono\">{}</span> {} {}</span></li>",
            esc(&stamp(num(h.get("at")))),
            esc(&text(h.get("kind"), "")),
            esc(&text(h.get("text"), "")),
        ));
    }
    for t in mind.get("tasks").as_array() {
        let title = text(t.get("title"), "");
        out.push_str(&format!(
            "<li><span class=\"ell tiny\"><span class=\"mono\">{}</span> {}{} {}</span></li>",
            esc(&text(t.get("id"), "")),
            esc(&text(t.get("state"), "")),
            if title.is_empty() {
                String::new()
            } else {
                format!(" · {}", esc(&title))
            },
            esc(&stamp(num(t.get("at")))),
        ));
    }
    out.push_str(&format!(
        "</ul><p class=\"actions\"><a class=\"btn ghost\" href=\"/hermes\">{}Open research</a><a class=\"btn ghost\" href=\"/context\">Refresh</a></p></section></div>",
        icon("research", "i i-sm")
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    const DESK: &str = r#"{"foundation":"rust","line":false,"model":"","vram":0,"ctx":65536,
"mindpack":{"context":{"cached_sessions":35,"cached_turns":153,"cache_bytes":47118,"model_window":262144},
"cache":{"read":109107266,"write":1193851,"input":19391995,"output":1830315,"reasoning":789359},
"roles":{"user":1085,"assistant":3625,"tool":3836},
"mind":{"line":"The live model is <b>galion</b>.","sessions":269,"messages":8546},
"road":[{"id":"openmind-boss-20261018","source":"api_server","model":"anthropic/claude-opus-5.5","messages":182,"at":1792340805}],
"tools":[{"name":"terminal","n":1456},{"name":"read_file","n":775},{"name":"tiny","n":1}],
"models":[{"name":"deepseek/deepseek-v4.1-flash","calls":205,"input":6593248,"output":424512,"read":47941393}],
"history":[{"at":1792340805,"kind":"ask","text":"undefined hello"}],
"tasks":[{"id":"t1","state":"done","title":"","at":1792340805}]}}"#;

    #[test]
    fn renders_the_desk_like_the_tsx() {
        let v = parse(DESK.as_bytes()).unwrap();
        let b = body(Some(&v));
        for needle in [
            "65536 live · 262144 window · no model · idle",
            "Line down · 65536 context · idle",
            "No model loaded",
            "35 sessions · 153 turns",
            "The live model is &lt;b&gt;galion&lt;/b&gt;.",
            "title=\"openmind-boss-20261018\">openmind…1018<",
            "10-18 18:26 · 182 msgs · api_server",
            "anthropic/claude-opus-5.5",
            "19.4M in · 1.8M out · 109.1M cache read · 789.4k reasoning",
            "1085 user · 3625 assistant · 3836 tool messages",
            "205 calls · 6.6M in · 424.5k out · 47.9M read",
            "value=\"100\">100%</progress><span class=\"ell\">terminal",
            "value=\"53\">53%",
            "value=\"4\">4%",
            "269 sessions · 8546 messages",
            "ask hello",
            "<span class=\"mono\">t1</span> done 10-18 18:26",
            "Europe/Zagreb (CEST)",
            "href=\"/hermes\"",
        ] {
            assert!(b.contains(needle), "missing {needle}\n{b}");
        }
        assert!(!b.contains("not answering"));
        assert!(!b.contains("style="));
    }

    #[test]
    fn desk_down_is_said() {
        let b = body(None);
        assert!(b.contains("— live · no model · idle"));
        assert!(b.contains("The desk foundation is not answering"));
        assert!(b.contains("No memory written."));
        assert!(b.contains("0 sessions · 0 messages"));
    }

    #[test]
    fn short_titles() {
        assert_eq!(short_title("abc"), "abc");
        assert_eq!(short_title("openmind-boss-20261018"), "openmind…1018");
    }
}
