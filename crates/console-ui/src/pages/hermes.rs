// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/hermes`: the Hermes research room, rebuilt (owner rulings t359u and
//! t360u, 2026-10-29 CET +01:00: JavaScript is allowed here, the paid
//! research model stays, rebuild the whole interface and make it better
//! than the old Node panel `hermes-panel.tsx`).
//!
//! Server-rendered shell + one small vanilla script (`/_ui/hermes.*.js`,
//! [`crate::Ui`]). The shell already shows gateway health, the guard and
//! spend against the cap, the provider picker and the searchable history,
//! and a thread opens without JavaScript (`/hermes?id=`). The script adds
//! asking (Enter sends, Shift+Enter is a new line), streamed replies from
//! the run's SSE events, stop, live history search and status refresh.
//!
//! Data: `/api/hermes/{health,guard,providers,models,modes,history}` on the
//! console itself; `console-static --hermes-api` adapts them to hermes-rs
//! and adds the gateway key server-side. The page never sees the key or the
//! gateway's bind address.
//!
//! galion OS §11 (no flash): this room has no animation, blink or pulse;
//! the pending state is static text. The mic button (t362u: live
//! dictation via galion-stt, `/api/stt/*`) mounts into `#hm-mic-slot`; its
//! AudioWorklet is `data-worklet`, partial text shows in `#hm-partial`.

use super::p2::{clip, compact, num, stamp, text, zone_label};
use crate::html::{esc, url_enc};
use crate::icons::icon;
use crate::json::Value;

/// Threads listed in the server-rendered history.
pub const MAX_THREADS: usize = 120;
/// Characters shown per message in a server-rendered thread.
pub const MAX_MESSAGE: usize = 20_000;
/// Research cap shown when the guard does not answer (matches
/// console-static `hermes::CAP_USD`).
pub const CAP_USD: f64 = 20.0;
/// Warning level (console-static `hermes::WARN_USD`).
pub const WARN_USD: f64 = 16.0;

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

/// The GETs this room makes (in [`View`] order).
pub const READS: &[&str] = &[
    "/api/hermes/health",
    "/api/hermes/guard",
    "/api/hermes/providers",
    "/api/hermes/models",
    "/api/hermes/modes",
    "/api/hermes/history",
];

/// The four picker choices when `/api/hermes/providers` does not answer
/// (all shown, none selectable).
const FALLBACK_PROVIDERS: &[(&str, &str)] = &[
    ("laptop", "Local laptop"),
    ("runpod", "RunPod"),
    ("openrouter", "OpenRouter"),
    ("river", "River AI"),
];

#[derive(Debug, Default, Clone, Copy)]
pub struct View<'a> {
    pub health: Option<&'a Value>,
    pub guard: Option<&'a Value>,
    pub providers: Option<&'a Value>,
    pub models: Option<&'a Value>,
    pub modes: Option<&'a Value>,
    pub history: Option<&'a Value>,
    /// Selected thread: id and its payload (`None` = did not load).
    pub thread: Option<(&'a str, Option<&'a Value>)>,
    /// `/_ui/hermes.<hash>.js`.
    pub script_href: &'a str,
    /// `/_ui/hermes-mic.<hash>.js` (the dictation AudioWorklet module).
    pub mic_href: &'a str,
}

fn usd(n: f64) -> String {
    format!("${:.2}", if n.is_finite() { n } else { 0.0 })
}

fn pill(key: &str, tone: &str, label: &str, value: &str, extra: &str) -> String {
    format!(
        "<li class=\"hm-pill\" data-k=\"{key}\"><span class=\"hm-dot {tone}\" aria-hidden=\"true\"></span><span class=\"hm-pl\">{}</span> <b class=\"hm-pv\">{}</b>{extra}</li>",
        esc(label),
        esc(value)
    )
}

fn gateway_pill(h: Option<&Value>) -> String {
    let Some(h) = h else {
        return pill("gateway", "bad", "Gateway", "no answer", "");
    };
    if h.get("ok").as_bool() != Some(true) {
        return pill(
            "gateway",
            "bad",
            "Gateway",
            &text(h.get("error"), "down"),
            "",
        );
    }
    let imp = text(h.get("implementation"), "hermes");
    let ver = text(h.get("version"), "");
    let ms = h.get("ms").as_f64().unwrap_or(0.0);
    let mut v = if ver.is_empty() {
        imp
    } else {
        format!("{imp} {ver}")
    };
    v.push_str(&format!(" · {ms:.0} ms"));
    let (tone, v) = if h.get("key").as_bool() == Some(false) {
        ("warn", format!("{v} · key missing"))
    } else if h.get("auth_ok").as_bool() == Some(false) {
        ("warn", format!("{v} · key refused"))
    } else {
        ("ok", v)
    };
    pill("gateway", tone, "Gateway", &v, "")
}

fn guard_pills(g: Option<&Value>) -> String {
    let Some(g) = g else {
        return pill("guard", "bad", "Guard", "no answer", "")
            + &pill("spend", "bad", "Spend", "unknown", "");
    };
    let on = g.get("on").as_bool() == Some(true);
    let guard = pill(
        "guard",
        if on { "ok" } else { "warn" },
        "Guard",
        if on { "on · River AI" } else { "off" },
        "",
    );
    let level = g.get("level").as_str().unwrap_or("unknown");
    let cap = g.get("cap_usd").as_f64().unwrap_or(CAP_USD);
    let warn = g.get("warn_usd").as_f64().unwrap_or(WARN_USD);
    let spend = match g.get("spent_usd").as_f64() {
        Some(spent) => {
            let tone = match level {
                "blocked" => "bad",
                "warn" => "warn",
                _ => "ok",
            };
            let meter = format!(
                "<meter class=\"hm-meter\" min=\"0\" max=\"{cap}\" low=\"{warn}\" high=\"{warn}\" optimum=\"0\" value=\"{:.4}\" aria-label=\"Research spend against the cap\"></meter>",
                spent.min(cap)
            );
            let label = match level {
                "blocked" => format!("{} of {} · cap reached", usd(spent), usd(cap)),
                "warn" => format!(
                    "{} of {} · past the {} warning",
                    usd(spent),
                    usd(cap),
                    usd(warn)
                ),
                _ => format!("{} of {}", usd(spent), usd(cap)),
            };
            pill("spend", tone, "Spend", &label, &meter)
        }
        None => pill("spend", "bad", "Spend", "unknown (asking is held)", ""),
    };
    guard + &spend
}

fn providers_html(p: Option<&Value>) -> String {
    let mut out = String::from(
        "<fieldset class=\"hm-prov\" id=\"hm-prov\"><legend class=\"hm-legend\">Model</legend><div class=\"hm-seg\">",
    );
    let default = p
        .and_then(|v| v.get("default").as_str())
        .unwrap_or("openrouter");
    let rows: Vec<(String, String, bool, String)> = match p {
        Some(v) if !v.get("providers").as_array().is_empty() => v
            .get("providers")
            .as_array()
            .iter()
            .map(|r| {
                (
                    text(r.get("id"), ""),
                    text(r.get("label"), ""),
                    r.get("enabled").as_bool() == Some(true),
                    text(r.get("status"), "not set up"),
                )
            })
            .collect(),
        _ => FALLBACK_PROVIDERS
            .iter()
            .map(|(id, l)| (id.to_string(), l.to_string(), false, "no answer".into()))
            .collect(),
    };
    for (id, label, enabled, status) in rows {
        let checked = if id == default && enabled {
            " checked"
        } else {
            ""
        };
        let dis = if enabled { "" } else { " disabled" };
        let tone = match status.as_str() {
            "ready" => "ok",
            "offline" => "bad",
            _ => "off",
        };
        out.push_str(&format!(
            "<label class=\"hm-opt\" data-status=\"{st}\"><input type=\"radio\" name=\"hm-provider\" value=\"{id}\"{checked}{dis}><span class=\"hm-optl\">{label}</span><span class=\"hm-opts\"><span class=\"hm-dot {tone}\" aria-hidden=\"true\"></span>{st}</span></label>",
            id = esc(&id),
            label = esc(&label),
            st = esc(&status),
        ));
    }
    out.push_str("</div></fieldset>");
    out
}

fn selects_html(models: Option<&Value>, modes: Option<&Value>) -> String {
    let mut out = String::from("<div class=\"hm-row\">");
    out.push_str("<label class=\"hm-field\"><span>Depth</span><select id=\"hm-mode\">");
    let rows = modes.map(|m| m.get("modes").as_array()).unwrap_or(&[]);
    if rows.is_empty() {
        out.push_str("<option value=\"deep\">Deep research</option><option value=\"quick\">Quick answer</option>");
    }
    for m in rows {
        out.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            esc(&text(m.get("id"), "")),
            esc(&text(m.get("label"), ""))
        ));
    }
    out.push_str("</select></label>");
    out.push_str("<label class=\"hm-field\" id=\"hm-model-wrap\"><span>OpenRouter model</span><select id=\"hm-model\"><option value=\"\">Gateway default</option>");
    for m in models.map(|m| m.get("models").as_array()).unwrap_or(&[]) {
        let id = text(m.get("id"), "");
        if id.is_empty() || id == "hermes-agent" {
            continue;
        }
        let root = text(m.get("root"), "");
        out.push_str(&format!(
            "<option value=\"{}\">{}{}</option>",
            esc(&id),
            esc(&id),
            if root.is_empty() {
                String::new()
            } else {
                format!(" · {}", esc(&root))
            }
        ));
    }
    out.push_str("</select></label></div>");
    out
}

fn history_html(history: Option<&Value>, current: Option<&str>) -> String {
    let mut out = String::new();
    let Some(h) = history else {
        out.push_str("<p class=\"hm-empty t-bad\" id=\"hm-hist-err\">History did not load.</p><ul class=\"hm-list\" id=\"hm-history\"></ul>");
        return out;
    };
    let items = h.get("items").as_array();
    let zone = items
        .iter()
        .map(|t| num(t.get("at")))
        .find(|a| *a != 0.0)
        .map(zone_label)
        .unwrap_or("CET");
    out.push_str(&format!(
        "<p class=\"hm-sub\" id=\"hm-hist-count\">{} threads · Europe/Zagreb ({zone})</p><ul class=\"hm-list\" id=\"hm-history\">",
        items.len()
    ));
    for t in items.iter().take(MAX_THREADS) {
        let id = t.get("id").as_str().unwrap_or("");
        if !id_ok(id) {
            continue;
        }
        let title = text(t.get("title"), id);
        let cur = if current == Some(id) {
            " aria-current=\"true\""
        } else {
            ""
        };
        let at = num(t.get("at"));
        let mut meta = vec![stamp(at), format!("{} msgs", text(t.get("count"), "0"))];
        let tok = num(t.get("tokens").get("total"));
        if tok > 0.0 {
            meta.push(format!("{} tok", compact(tok)));
        }
        let cost = num(t.get("cost_usd"));
        if cost > 0.0 {
            meta.push(usd(cost));
        }
        if t.get("console").as_bool() == Some(true) {
            meta.push("console".into());
        }
        out.push_str(&format!(
            "<li><a class=\"hm-item\" href=\"/hermes?id={href}\" data-id=\"{id}\" data-at=\"{at}\"{cur}><span class=\"hm-it\">{title}</span><span class=\"hm-im\">{meta}</span></a></li>",
            href = esc(&url_enc(id)),
            id = esc(id),
            at = at,
            title = esc(&title),
            meta = esc(&meta.join(" · ")),
        ));
    }
    out.push_str("</ul>");
    if items.is_empty() {
        out.push_str("<p class=\"hm-empty\" id=\"hm-hist-none\">No threads yet.</p>");
    }
    out
}

fn thread_html(id: &str, v: Option<&Value>) -> String {
    let Some(v) = v else {
        return format!(
            "<p class=\"hm-empty t-bad\">Thread <span class=\"mono\">{}</span> did not load.</p>",
            esc(id)
        );
    };
    let mut out = format!(
        "<p class=\"hm-thread-meta\"><span class=\"hm-it\">{}</span> <span class=\"hm-im\">{} · {} · {} tokens{}</span></p>",
        esc(&text(v.get("title"), id)),
        esc(&stamp(num(v.get("at")))),
        esc(&text(v.get("model"), "model ?")),
        compact(num(v.get("tokens").get("total"))),
        match num(v.get("cost_usd")) {
            c if c > 0.0 => format!(" · {}", usd(c)),
            _ => String::new(),
        }
    );
    for it in v.get("items").as_array() {
        let kind = text(it.get("kind"), "note");
        match kind.as_str() {
            "tool" => out.push_str(&format!(
                "<div class=\"hm-tool\"><span class=\"mono\">{}</span></div>",
                esc(&text(it.get("name"), "tool"))
            )),
            "user" | "assistant" => out.push_str(&format!(
                "<article class=\"hm-msg hm-{kind}\"><p class=\"hm-who\">{}</p><div class=\"hm-text\">{}</div></article>",
                if kind == "user" { "You" } else { "Hermes" },
                esc(&clip(it.get("text").as_str().unwrap_or(""), MAX_MESSAGE))
            )),
            _ => {}
        }
    }
    out
}

/// The page body (inside the console shell).
pub fn body(v: &View<'_>) -> String {
    let current = v.thread.map(|(id, _)| id);
    let mut out = format!(
        "<div class=\"hm\" id=\"hm\" data-thread=\"{}\">",
        esc(current.unwrap_or(""))
    );
    out.push_str(&format!(
        "<header class=\"hm-head\"><div class=\"hm-title\"><p class=\"k\">Hermes Research</p><h1>{}<span>The mind</span></h1></div><ul class=\"hm-status\" id=\"hm-status\" aria-label=\"Status\">{}{}</ul></header>",
        icon("research", "i i-md"),
        gateway_pill(v.health),
        guard_pills(v.guard),
    ));
    out.push_str("<div class=\"hm-grid\">");
    // History.
    out.push_str("<aside class=\"hm-side\" aria-label=\"History\"><details class=\"hm-hist\" open><summary class=\"hm-hist-sum\">History</summary><div class=\"hm-side-tools\"><a class=\"hm-btn\" id=\"hm-new\" href=\"/hermes\">New thread</a><label class=\"sr-only\" for=\"hm-search\">Search history</label><input id=\"hm-search\" class=\"hm-search\" type=\"search\" placeholder=\"Search history\" autocomplete=\"off\" spellcheck=\"false\"></div>");
    out.push_str(&history_html(v.history, current));
    out.push_str("</details></aside>");
    // Conversation.
    out.push_str("<section class=\"hm-main\" aria-label=\"Conversation\"><div class=\"hm-log\" id=\"hm-log\" aria-live=\"polite\" aria-busy=\"false\" tabindex=\"-1\">");
    match v.thread {
        Some((id, t)) => out.push_str(&thread_html(id, t)),
        None => out.push_str(
            "<div class=\"hm-welcome\" id=\"hm-welcome\"><p class=\"hm-it\">Ask Hermes a research question.</p><p class=\"hm-im\">He searches first and answers with figures a tool returned. Pick the model below; OpenRouter is the default. Every ask counts against the research cap.</p></div>",
        ),
    }
    out.push_str("</div>");
    // Composer.
    out.push_str("<div class=\"hm-composer\" id=\"hm-composer\">");
    out.push_str(&providers_html(v.providers));
    out.push_str(&selects_html(v.models, v.modes));
    out.push_str(&format!(
        "<div class=\"hm-box\"><label class=\"sr-only\" for=\"hm-q\">Ask Hermes</label><textarea id=\"hm-q\" class=\"hm-q\" rows=\"3\" maxlength=\"12000\" placeholder=\"Ask Hermes. Enter sends, Shift+Enter adds a line.\" aria-describedby=\"hm-note\" disabled></textarea><p class=\"hm-partial\" id=\"hm-partial\" aria-live=\"polite\" hidden></p><div class=\"hm-actions\"><span id=\"hm-mic-slot\" class=\"hm-mic-slot\" data-hook=\"mic\" data-worklet=\"{}\" data-stt=\"/api/stt/health\" data-stream=\"/api/stt/stream\" hidden></span><button type=\"button\" class=\"hm-btn\" id=\"hm-stop\" hidden>Stop</button><button type=\"button\" class=\"hm-btn hm-primary\" id=\"hm-send\" disabled>Send</button></div></div>",
        esc(v.mic_href)
    ));
    out.push_str("<p class=\"hm-note\" id=\"hm-note\" role=\"status\">Loading the composer.</p><noscript><p class=\"hm-note t-bad\">JavaScript is off: you can read status and threads, but asking needs JavaScript on this page.</p></noscript>");
    out.push_str("</div></section></div>");
    out.push_str(&format!(
        "<script src=\"{}\" defer></script></div>",
        esc(v.script_href)
    ));
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
        assert!(!id_ok(&"a".repeat(129)));
        assert_eq!(thread_path("x-1"), "/api/hermes/history?id=x-1");
    }

    fn sample() -> [Value; 7] {
        [
            parse(br#"{"ok":true,"status":200,"ms":0.6,"loopback":true,"mode":"gateway","implementation":"hermes-rs","version":"0.4.0","key":true,"auth_ok":true}"#).unwrap(),
            parse(br#"{"ok":true,"on":true,"provider":"river","level":"warn","spent_usd":16.5,"cap_usd":20,"warn_usd":16}"#).unwrap(),
            parse(br#"{"ok":true,"default":"openrouter","providers":[{"id":"laptop","label":"Local laptop","enabled":false,"status":"offline"},{"id":"runpod","label":"RunPod","enabled":false,"status":"not set up"},{"id":"openrouter","label":"OpenRouter","enabled":true,"status":"ready"},{"id":"river","label":"River AI","enabled":true,"status":"ready"}]}"#).unwrap(),
            parse(br#"{"ok":true,"models":[{"id":"hermes-agent","root":"hermes-agent"},{"id":"galion","root":"anthropic/claude-haiku-5.5"}]}"#).unwrap(),
            parse(br#"{"ok":true,"modes":[{"id":"deep","label":"Deep research"},{"id":"quick","label":"Quick answer"}]}"#).unwrap(),
            parse(br#"{"ok":true,"items":[{"id":"api-1","title":"boss <x>","count":4,"at":1792340805,"console":true,"cost_usd":0.012,"tokens":{"total":1200}},{"id":"bad id","title":"x"}]}"#).unwrap(),
            parse(br#"{"ok":true,"title":"boss <x>","at":1792340805,"model":"m","tokens":{"total":3},"items":[{"kind":"user","text":"hi <b>"},{"kind":"tool","name":"web_search"},{"kind":"assistant","text":"ok"}]}"#).unwrap(),
        ]
    }

    #[test]
    fn renders_interactive_room() {
        let [h, g, p, m, mo, hi, th] = sample();
        let b = body(&View {
            health: Some(&h),
            guard: Some(&g),
            providers: Some(&p),
            models: Some(&m),
            modes: Some(&mo),
            history: Some(&hi),
            thread: Some(("api-1", Some(&th))),
            script_href: "/_ui/hermes.0123.js",
            mic_href: "/_ui/hermes-mic.0123.js",
        });
        for n in [
            "hermes-rs 0.4.0 · 1 ms",
            "on · River AI",
            "$16.50 of $20.00 · past the $16.00 warning",
            "<meter class=\"hm-meter\" min=\"0\" max=\"20\" low=\"16\" high=\"16\"",
            "<textarea id=\"hm-q\"",
            "id=\"hm-send\"",
            "id=\"hm-search\"",
            "id=\"hm-mic-slot\" class=\"hm-mic-slot\" data-hook=\"mic\" data-worklet=\"/_ui/hermes-mic.0123.js\" data-stt=\"/api/stt/health\" data-stream=\"/api/stt/stream\" hidden",
            "id=\"hm-partial\"",
            "value=\"openrouter\" checked>",
            "value=\"river\">",
            "value=\"runpod\" disabled>",
            "value=\"laptop\" disabled>",
            ">offline</span>",
            ">not set up</span>",
            "<option value=\"galion\">galion · anthropic/claude-haiku-5.5</option>",
            "href=\"/hermes?id=api-1\" data-id=\"api-1\"",
            "aria-current=\"true\"><span class=\"hm-it\">boss &lt;x&gt;</span>",
            "4 msgs · 1.2k tok · $0.01 · console",
            "<div class=\"hm-text\">hi &lt;b&gt;</div>",
            "<span class=\"mono\">web_search</span>",
            "<script src=\"/_ui/hermes.0123.js\" defer></script>",
            "data-thread=\"api-1\"",
        ] {
            assert!(b.contains(n), "missing {n}\n{b}");
        }
        assert!(!b.contains("bad id"), "invalid ids are not listed");
        assert!(!b.contains("8642"), "bind address must not show");
        assert!(!b.contains("<form"), "no form post: the script sends JSON");
        assert!(!b.contains("animation") && !b.contains("blink"), "§11");
    }

    #[test]
    fn down_states() {
        let b = body(&View {
            thread: Some(("x", None)),
            script_href: "/_ui/hermes.0.js",
            mic_href: "/_ui/hermes-mic.0.js",
            ..View::default()
        });
        assert!(b.contains("Gateway</span> <b class=\"hm-pv\">no answer"));
        assert!(b.contains("Guard</span> <b class=\"hm-pv\">no answer"));
        assert!(b.contains("History did not load."));
        assert!(b.contains("did not load."));
        // Picker still lists the four choices, none selectable.
        for id in ["laptop", "runpod", "openrouter", "river"] {
            assert!(b.contains(&format!("value=\"{id}\" disabled>")), "{id}");
        }
    }

    #[test]
    fn blocked_spend() {
        let g = parse(
            br#"{"ok":true,"on":true,"level":"blocked","spent_usd":21,"cap_usd":20,"warn_usd":16}"#,
        )
        .unwrap();
        let b = body(&View {
            guard: Some(&g),
            ..View::default()
        });
        assert!(b.contains("$21.00 of $20.00 · cap reached"));
        assert!(b.contains("value=\"20.0000\""), "meter clamps at the cap");
    }
}
