// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/hermes` (console-ide `src/routes/_console/hermes.tsx` ->
//! `src/components/hermes-panel.tsx`, 2758 lines, live tree 2026-10-20
//! 21:25 CEST): the **read-only part** of Hermes Research, without
//! JavaScript.
//!
//! Ported: gateway health (`/api/hermes/health`), guard and auto-accept
//! state (`/api/hermes/guard`), plan/action models and saved modes
//! (`/api/hermes/modes`), the model list (`/api/hermes/models`), the thread
//! list and token totals (`/api/hermes/history`) and one thread
//! (`/api/hermes/history?id=`), all GET.
//!
//! Held, on purpose (owner decisions, docs/console-port-2.md):
//! - asking / runs / steer / stop / approvals / attach / mic / speak
//!   (`/api/hermes/chat`, `/runs*`, `/transcribe`, `/speak`): the JS ruling
//!   (decision 1: no-JS form, script island, or keep the SPA) and H1 (a
//!   question may reach a paid model) are Slavik's;
//! - guard / auto-accept toggles, loops, settings, background start/stop:
//!   state-changing POSTs that move Hermes's safety switches;
//! - localStorage (threads, mode, seen marks): server-side history only.
//!
//! The gateway's bind address is not shown (only whether it is loopback).
//!
//! Rust-first since console-route-31 (wave 4 slot 170): no longer in
//! `port2::SPA_FIRST`, so a configured SPA fallback (`--root`) does not take
//! this room; POST is 405, the `/api/hermes/*` proxy for the old SPA is
//! untouched.

use super::p2::{clip, compact, js_num, num, stamp, text, zone_label};
use crate::html::{esc, url_enc};
use crate::icons::icon;
use crate::json::{self, Value};

/// Threads listed (newest first as the API sends them).
pub const MAX_THREADS: usize = 60;
/// Characters shown per message in a thread.
pub const MAX_MESSAGE: usize = 6_000;

/// A thread id may be shown/fetched only if it is 1..=128 of
/// `[A-Za-z0-9._:-]` (the ids the API returns).
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// Upstream path for one thread.
pub fn thread_path(id: &str) -> String {
    format!("/api/hermes/history?id={}", url_enc(id))
}

/// The GETs this room makes.
pub const READS: &[&str] = &[
    "/api/hermes/health",
    "/api/hermes/guard",
    "/api/hermes/modes",
    "/api/hermes/models",
    "/api/hermes/history",
];

#[derive(Debug, Default, Clone, Copy)]
pub struct View<'a> {
    pub health: Option<&'a Value>,
    pub guard: Option<&'a Value>,
    pub modes: Option<&'a Value>,
    pub models: Option<&'a Value>,
    pub history: Option<&'a Value>,
    /// Selected thread: id and its payload (`None` = did not load).
    pub thread: Option<(&'a str, Option<&'a Value>)>,
}

fn fact(label: &str, value: &str, ok: Option<bool>) -> String {
    let tone = match ok {
        Some(true) => "online",
        Some(false) => "offline",
        None => "held",
    };
    format!(
        "<li class=\"fact\"><span class=\"ell\">{}</span><span><span class=\"dot {tone}\"></span><span class=\"ell\">{}</span></span></li>",
        esc(label),
        esc(value)
    )
}

fn on_off(v: &Value) -> Option<bool> {
    v.as_bool()
}

fn gateway_line(h: &Value) -> (String, Option<bool>) {
    if h.get("ok").as_bool() != Some(true) {
        let why = text(h.get("error"), "not ok");
        return (why, Some(false));
    }
    let raw = h
        .get("raw")
        .as_str()
        .and_then(|r| json::parse(r.as_bytes()).ok())
        .unwrap_or(Value::Null);
    let imp = text(raw.get("implementation"), "hermes");
    let ver = text(raw.get("implementation_version"), "");
    let mode = text(h.get("mode"), "");
    let ms = h.get("ms").as_f64().map(js_num).unwrap_or_default();
    let mut parts = vec![if ver.is_empty() {
        imp
    } else {
        format!("{imp} {ver}")
    }];
    if !mode.is_empty() {
        parts.push(mode);
    }
    parts.push(
        if h.get("loopback").as_bool() == Some(true) {
            "loopback"
        } else {
            "not loopback"
        }
        .into(),
    );
    if !ms.is_empty() {
        parts.push(format!("{ms} ms"));
    }
    (parts.join(" · "), Some(true))
}

fn threads(out: &mut String, history: Option<&Value>, current: Option<&str>) {
    let Some(h) = history else {
        out.push_str(
            "<p class=\"note t-bad\">The thread list (/api/hermes/history) did not load.</p>",
        );
        return;
    };
    let items = h.get("items").as_array();
    let zone = items
        .iter()
        .map(|t| num(t.get("at")))
        .find(|a| *a != 0.0)
        .map(zone_label)
        .unwrap_or("CEST/CET");
    out.push_str(&format!(
        "<section class=\"sec\" aria-label=\"Threads\"><h2>Threads · {} <span class=\"subtle\">· times Europe/Zagreb ({zone})</span></h2><ul class=\"rows\">",
        items.len()
    ));
    for t in items.iter().take(MAX_THREADS) {
        let id = t.get("id").as_str().unwrap_or("");
        let title = text(t.get("title"), id);
        let total = num(t.get("tokens").get("total"));
        let pin = if t.get("pinned").as_bool() == Some(true) {
            "pinned · "
        } else {
            ""
        };
        let name = if id_ok(id) {
            let cur = if current == Some(id) {
                " aria-current=\"page\""
            } else {
                ""
            };
            format!(
                "<a class=\"ell\" href=\"/hermes?id={}\"{cur}>{}</a>",
                esc(&url_enc(id)),
                esc(&title)
            )
        } else {
            format!("<span class=\"ell\">{}</span>", esc(&title))
        };
        out.push_str(&format!(
            "<li>{name}<span class=\"muted tiny\">{pin}{} msgs · {} · {} tokens · {}</span></li>",
            esc(&text(t.get("count"), "0")),
            esc(&text(t.get("source"), "session")),
            compact(total),
            esc(&stamp(num(t.get("at")))),
        ));
    }
    out.push_str("</ul>");
    if items.len() > MAX_THREADS {
        out.push_str(&format!(
            "<p class=\"note\">{} older threads not listed.</p>",
            items.len() - MAX_THREADS
        ));
    }
    out.push_str("</section>");
}

fn thread(out: &mut String, id: &str, v: Option<&Value>) {
    out.push_str(&format!(
        "<section class=\"sec\" aria-label=\"Thread\" id=\"thread\"><h2><a href=\"/hermes\">Threads</a> · <span class=\"mono\">{}</span></h2>",
        esc(id)
    ));
    let Some(v) = v else {
        out.push_str("<p class=\"note t-bad\">This thread did not load.</p></section>");
        return;
    };
    let items = v.get("items").as_array();
    let tok = v.get("tokens");
    out.push_str(&format!(
        "<p class=\"small\">{} items · {} in · {} out · {} total</p><ul class=\"msgs\">",
        items.len(),
        compact(num(tok.get("input"))),
        compact(num(tok.get("output"))),
        compact(num(tok.get("total"))),
    ));
    for it in items {
        let kind = text(it.get("kind"), "note");
        if kind == "tool" {
            let cmd = it
                .get("command")
                .as_str()
                .and_then(|c| c.lines().find(|l| !l.trim().is_empty()))
                .unwrap_or("");
            out.push_str(&format!(
                "<li class=\"tool\"><span class=\"mono\">{}</span> <span class=\"muted\">{}</span>{}</li>",
                esc(&text(it.get("name"), "tool")),
                esc(&text(it.get("status"), "")),
                if cmd.is_empty() {
                    String::new()
                } else {
                    format!(
                        " <span class=\"subtle mono\">{}</span>",
                        esc(&clip(cmd.trim(), 160))
                    )
                }
            ));
        } else {
            let body = it.get("text").as_str().unwrap_or("");
            out.push_str(&format!(
                "<li><p class=\"k\">{}</p><pre class=\"say\">{}</pre></li>",
                esc(&kind),
                esc(&clip(body, MAX_MESSAGE))
            ));
        }
    }
    out.push_str("</ul></section>");
}

pub fn body(v: &View<'_>) -> String {
    let mut out =
        String::from("<div class=\"page w46\"><header><p class=\"k\">Hermes Research</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">The mind</span></h1><p class=\"lede\">One ask, then he waits. Read-only here: threads, models and the guard.</p></header><ul class=\"facts\">",
        icon("research", "i i-md")
    ));
    let (gw, gw_ok) = match v.health {
        Some(h) => gateway_line(h),
        None => ("no answer".into(), Some(false)),
    };
    out.push_str(&fact("Gateway", &gw, gw_ok));
    if let Some(h) = v.health {
        let n = h.get("skills").as_array().len();
        out.push_str(&fact("Skills", &n.to_string(), Some(n > 0)));
    }
    match v.guard {
        Some(g) => {
            let on = on_off(g.get("on"));
            out.push_str(&fact(
                "Guard",
                match on {
                    Some(true) => "on",
                    Some(false) => "off",
                    None => "unknown",
                },
                on,
            ));
            let acc = on_off(g.get("autoAccept"));
            out.push_str(&fact(
                "Auto-accept",
                match acc {
                    Some(true) => "on",
                    Some(false) => "off",
                    None => "unknown",
                },
                None,
            ));
        }
        None => out.push_str(&fact("Guard", "no answer", Some(false))),
    }
    if let Some(m) = v.modes {
        let modes = m.get("modes");
        out.push_str(&fact("Plan model", &text(modes.get("plan"), "—"), None));
        out.push_str(&fact("Action model", &text(modes.get("action"), "—"), None));
    }
    out.push_str("</ul>");
    out.push_str(
        "<p class=\"note\">Asking, runs, the mic and the guard switches are not on this page yet: they wait on Slavik's JavaScript ruling and the paid-model decision (H1). The Hermes gateway is unchanged; this read-only room is served from Rust even when the old console is installed.</p>",
    );

    if let Some((id, t)) = v.thread {
        thread(&mut out, id, t);
    }

    if let Some(m) = v.modes {
        let saved = m.get("saved").as_array();
        if !saved.is_empty() {
            out.push_str("<section class=\"sec\" aria-label=\"Modes\"><h2>Saved modes</h2><ul class=\"rows\">");
            for s in saved {
                out.push_str(&format!(
                    "<li><span>{}</span><span class=\"muted tiny\">{} · {}</span></li>",
                    esc(&text(s.get("name"), "")),
                    esc(&text(s.get("model"), "")),
                    esc(&text(s.get("note"), "")),
                ));
            }
            out.push_str("</ul></section>");
        }
    }
    if let Some(m) = v.models {
        out.push_str(
            "<section class=\"sec\" aria-label=\"Models\"><h2>Models</h2><ul class=\"rows\">",
        );
        for x in m.get("models").as_array() {
            out.push_str(&format!(
                "<li><span>{}</span><span class=\"muted tiny mono\">{}</span></li>",
                esc(&text(x.get("name"), "")),
                esc(&text(x.get("id"), "")),
            ));
        }
        out.push_str("</ul></section>");
    }
    let current = v.thread.map(|(id, _)| id);
    threads(&mut out, v.history, current);
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn ids() {
        assert!(id_ok("openmind-boss-20261018"));
        assert!(id_ok("a.b:c_d"));
        assert!(!id_ok(""));
        assert!(!id_ok("../etc"));
        assert!(!id_ok("a b"));
        assert!(!id_ok("a&view=databases"));
        assert!(!id_ok(&"a".repeat(129)));
        assert_eq!(thread_path("x-1"), "/api/hermes/history?id=x-1");
    }

    #[test]
    fn renders_read_only_room() {
        let health = parse(br#"{"ok":true,"status":200,"ms":3,"bind":"127.0.0.1:8642","loopback":true,"mode":"gateway","skills":[{"id":"a"},{"id":"b"}],"raw":"{\"status\":\"ok\",\"implementation\":\"hermes-rs\",\"implementation_version\":\"0.1.0\"}"}"#).unwrap();
        let guard = parse(br#"{"ok":true,"on":true,"autoAccept":false}"#).unwrap();
        let modes = parse(br#"{"ok":true,"modes":{"plan":"zai-org/GLM-5.3-Flash","action":"m2"},"saved":[{"id":"river","name":"River","model":"deepseek","note":"default"}]}"#).unwrap();
        let models = parse(r#"{"ok":true,"models":[{"id":"google/gemini-2.5-flash","name":"OpenRouter · Gemini"}]}"#.as_bytes()).unwrap();
        let hist = parse(br#"{"ok":true,"items":[{"id":"openmind-boss-20261018","title":"boss <x>","count":29,"at":1792340805.07,"source":"api_server","pinned":true,"tokens":{"total":1237223}},{"id":"bad id","title":"","count":1,"at":0,"source":"one"}],"files":[]}"#).unwrap();
        let th = parse(br#"{"ok":true,"tokens":{"input":4925718,"output":75876,"total":5001594},"items":[{"kind":"user","text":"hi <b>"},{"kind":"tool","name":"terminal","status":"done","command":"\n  ls -la\nrm x","output":"secret-ish output"},{"kind":"assistant","text":"ok"}]}"#).unwrap();
        let b = body(&View {
            health: Some(&health),
            guard: Some(&guard),
            modes: Some(&modes),
            models: Some(&models),
            history: Some(&hist),
            thread: Some(("openmind-boss-20261018", Some(&th))),
        });
        for n in [
            "hermes-rs 0.1.0 · gateway · loopback · 3 ms",
            ">2<",
            "<span class=\"ell\">on</span>",
            "<span class=\"ell\">off</span>",
            "zai-org/GLM-5.3-Flash",
            "River</span><span class=\"muted tiny\">deepseek · default",
            "OpenRouter · Gemini",
            "Threads · 2 ",
            "href=\"/hermes?id=openmind-boss-20261018\" aria-current=\"page\">boss &lt;x&gt;</a>",
            "pinned · 29 msgs · api_server · 1.2M tokens · 10-18 18:26",
            "<span class=\"ell\">bad id</span>",
            "3 items · 4.9M in · 75.9k out · 5.0M total",
            "<pre class=\"say\">hi &lt;b&gt;</pre>",
            "<span class=\"mono\">terminal</span> <span class=\"muted\">done</span> <span class=\"subtle mono\">ls -la</span>",
            "Slavik's JavaScript ruling",
        ] {
            assert!(b.contains(n), "missing {n}\n{b}");
        }
        assert!(!b.contains("127.0.0.1:8642"), "bind address must not show");
        assert!(!b.contains("secret-ish output"), "tool output is not shown");
        assert!(!b.contains("<form"), "no state-changing form");
    }

    #[test]
    fn down_and_missing_thread() {
        let b = body(&View {
            thread: Some(("x", None)),
            ..View::default()
        });
        assert!(b.contains(
            "Gateway</span><span><span class=\"dot offline\"></span><span class=\"ell\">no answer"
        ));
        assert!(b.contains("This thread did not load."));
        assert!(b.contains("The thread list (/api/hermes/history) did not load."));
    }
}
