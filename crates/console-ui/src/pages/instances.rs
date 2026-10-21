// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/instances` (console-ide `src/routes/_console/instances.tsx`, live tree
//! 2026-10-20 21:25 CEST): phone, pod and laptop, the parts on this pod and
//! the private path, from `GET /api/v1/instances` and `GET /api/v1/laptop`.
//!
//! GET only: the room sends nothing that changes state.
//!
//! Differences from the TSX, on purpose:
//! - no 8 s / 15 s polling (needs JavaScript): a Refresh link, and the page
//!   says which call did not answer instead of "Reading…" forever;
//! - the TSX kept the last `model-pull` job across polls in browser state;
//!   a server render has no previous poll, so only the current jobs show;
//! - Copy buttons (`navigator.clipboard`) are gone: the laptop key and the
//!   tunnel command are in `<pre>` blocks that can be selected;
//! - laptop terminal lines are capped (80 lines, 400 characters each) and
//!   control characters are shown as spaces.

use super::p3::{cap, miss, plain, s};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// Lines of the laptop terminal shown (the API sends at most 80).
pub const MAX_LINES: usize = 80;
/// Characters per terminal line shown.
pub const MAX_LINE: usize = 400;

/// What a call gave the page: the JSON, or why not.
pub type Got<'a> = Result<&'a Value, (Option<u16>, String)>;

fn tone(state: &str) -> &'static str {
    match state {
        "online" => "online",
        "held" => "held",
        _ => "offline",
    }
}

fn state_badge(state: &str, word: &str) -> String {
    format!(
        "<span class=\"badge {}\">{}</span>",
        tone(state),
        esc(&cap(&plain(word), 40))
    )
}

fn joined(a: &str, b: &str) -> String {
    [a, b]
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| plain(t))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn card(name: &str, badge: &str, lines: &[String]) -> String {
    let mut out = format!(
        "<li class=\"p3-card\"><div class=\"p3-top\"><span class=\"p3-name ell\">{}</span>{badge}</div>",
        esc(&cap(&plain(name), 80))
    );
    for l in lines.iter().filter(|l| !l.is_empty()) {
        out.push_str(&format!("<p class=\"p3-sub\">{}</p>", esc(&cap(l, 300))));
    }
    out.push_str("</li>");
    out
}

fn copy_block(label: &str, value: &str) -> String {
    format!(
        "<div class=\"p3-copy\"><p class=\"k\">{}</p><pre class=\"p3-pre\">{}</pre></div>",
        esc(label),
        esc(&cap(&plain(value), 2000))
    )
}

pub fn body(instances: Got<'_>, laptop: Got<'_>) -> String {
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Instances</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">Phone, pod, laptop</span></h1>\
<p class=\"lede\">The phone is the screen. This pod runs the console. The laptop is the open model, and that port is not on the internet.</p>\
<p class=\"p3-refresh\"><a href=\"/instances\">Refresh</a></p></header>",
        icon("instances", "i i-md")
    ));

    // The three machines.
    out.push_str("<section class=\"p3-sec\">");
    match &instances {
        Ok(v) => {
            let nodes = v.get("nodes").as_array();
            if nodes.is_empty() {
                out.push_str("<p class=\"note\">No machines in the answer.</p>");
            } else {
                out.push_str("<ul class=\"p3-list\">");
                for n in nodes {
                    let state = s(n, "state");
                    let mark = n.get("mark").as_str().unwrap_or(state);
                    out.push_str(&card(
                        s(n, "name"),
                        &state_badge(state, mark),
                        &[plain(s(n, "detail")), joined(s(n, "role"), s(n, "place"))],
                    ));
                }
                out.push_str("</ul>");
            }
        }
        Err((code, err)) => out.push_str(&miss("The machine list", *code, err)),
    }
    out.push_str("</section>");

    // Laptop jobs and terminal.
    out.push_str("<section class=\"p3-sec\">");
    let (word, st) = match &laptop {
        Ok(v) if v.get("connected").as_bool() == Some(true) => ("linked", "online"),
        Ok(_) => ("no path", "offline"),
        Err(_) => ("no answer", "offline"),
    };
    out.push_str(&format!(
        "<div class=\"p3-top p3-h\"><h2>Laptop</h2>{}</div>",
        state_badge(st, word)
    ));
    match &laptop {
        Ok(v) => {
            let jobs = v.get("jobs").as_array();
            if jobs.is_empty() {
                out.push_str("<p class=\"note\">Nothing is running on the laptop.</p>");
            } else {
                out.push_str("<ul class=\"p3-list\">");
                for j in jobs {
                    let pct = j
                        .get("pct")
                        .as_f64()
                        .filter(|p| p.is_finite())
                        .map(|p| p.clamp(0.0, 100.0).round() as u32);
                    let badge = pct
                        .map(|p| format!("<span class=\"p3-pct\">{p}%</span>"))
                        .unwrap_or_default();
                    let mut li = card(s(j, "name"), &badge, &[plain(s(j, "detail"))]);
                    if let Some(p) = pct {
                        li = li.replace(
                            "</li>",
                            &format!(
                                "<progress class=\"p3-bar\" max=\"100\" value=\"{p}\">{p}%</progress></li>"
                            ),
                        );
                    }
                    out.push_str(&li);
                }
                out.push_str("</ul>");
            }
            let lines = v.get("lines").as_array();
            let shown: Vec<String> = lines
                .iter()
                .filter_map(Value::as_str)
                .map(|l| cap(&plain(l), MAX_LINE))
                .collect();
            if !shown.is_empty() {
                let from = shown.len().saturating_sub(MAX_LINES);
                out.push_str(&format!(
                    "<pre class=\"p3-term\">{}</pre>",
                    esc(&shown[from..].join("\n"))
                ));
            }
        }
        Err((code, err)) => out.push_str(&miss("The laptop status", *code, err)),
    }
    out.push_str("</section>");

    // Parts on this pod.
    out.push_str("<section class=\"p3-sec\"><h2>On this pod</h2>");
    match &instances {
        Ok(v) => {
            let parts = v.get("parts").as_array();
            if parts.is_empty() {
                out.push_str("<p class=\"note\">Nothing answered.</p>");
            } else {
                let up = parts.iter().filter(|p| s(p, "state") == "online").count();
                out.push_str(&format!(
                    "<p class=\"p3-sub\">{up} of {} online</p><ul class=\"p3-list\">",
                    parts.len()
                ));
                for p in parts {
                    let state = s(p, "state");
                    out.push_str(&card(
                        s(p, "name"),
                        &state_badge(state, state),
                        &[plain(s(p, "note")), joined(s(p, "where"), s(p, "bind"))],
                    ));
                }
                out.push_str("</ul>");
            }
        }
        Err(_) => out.push_str("<p class=\"note\">Not read (see above).</p>"),
    }
    out.push_str("</section>");

    // Private path.
    out.push_str(
        "<section class=\"p3-sec\"><h2>Private path</h2><p class=\"lede\">Ollama on the laptop listens on 127.0.0.1 only. \
The pod sees that port only as 127.0.0.1:11434, through SSH. Nothing is published.</p>",
    );
    if let Ok(v) = &instances {
        let link = v.get("link");
        let key = s(link, "pubkey");
        if !key.is_empty() {
            out.push_str(&copy_block("Laptop key", key));
        }
        let tunnel = s(link, "tunnel");
        if !tunnel.is_empty() {
            out.push_str(&copy_block("From the laptop", tunnel));
        }
    }
    out.push_str("</section></div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    /// Shape of the live answer (VPS, 2026-10-21 05:40 CEST), key shortened.
    pub const LIVE: &str = r#"{"gpu":null,"nodes":[{"id":"phone","name":"This phone","role":"Screen","place":"Your hand","detail":"Opens the console. Runs no model.","state":"online","mark":"online"},{"id":"pod","name":"RunPod","role":"Machine","place":"RTX 2000 Ada","detail":"GPU unread","state":"online","mark":"online"},{"id":"laptop","name":"GALION-STUDIO","role":"Open model","place":"RTX 4060 Laptop · 8 GB","detail":"Windows 11 · 64 GB RAM · private tunnel only","state":"offline","mark":"offline"}],"parts":[{"id":"front","name":"Front","where":"This pod","bind":":8080","note":"The console.","state":"online"},{"id":"voice","name":"Voice","where":"This pod","bind":"127.0.0.1:8002","note":"Inside One.","state":"offline"},{"id":"laptop","name":"Laptop model","where":"GALION-STUDIO","bind":"127.0.0.1:11434","note":"Path is up.","state":"held"}],"link":{"public":false,"pubkey":"ssh-ed25519 AAAAexample galion-laptop","tunnel":"ssh -N -R 127.0.0.1:11434:127.0.0.1:11434"}}"#;

    #[test]
    fn renders_live_shape() {
        let i = json::parse(LIVE.as_bytes()).unwrap();
        let l = json::parse(br#"{"connected":false,"jobs":[],"lines":[]}"#).unwrap();
        let h = body(Ok(&i), Ok(&l));
        assert_eq!(h.matches("<li class=\"p3-card\">").count(), 6);
        assert!(h.contains("GALION-STUDIO"));
        assert!(h.contains("Screen · Your hand"));
        assert!(h.contains("RTX 4060 Laptop · 8 GB"));
        assert!(h.contains("<span class=\"badge held\">held</span>"));
        assert!(h.contains("1 of 3 online"));
        assert!(h.contains("no path"));
        assert!(h.contains("Nothing is running on the laptop."));
        assert!(h.contains("ssh-ed25519 AAAAexample galion-laptop"));
        assert!(h.contains("From the laptop"));
        assert!(!h.contains("<form"));
        assert!(!h.contains("<script"));
    }

    #[test]
    fn jobs_lines_and_escaping() {
        let i = json::parse(LIVE.as_bytes()).unwrap();
        let mut lines = String::new();
        for n in 0..100 {
            lines.push_str(&format!("\"line {n}\","));
        }
        let raw = format!(
            r#"{{"connected":true,"jobs":[{{"id":"model-pull","name":"<qwen>","detail":"1 / 2 MB","pct":42}},{{"id":"run-CMD","name":"CMD","detail":"Running","pct":null}},{{"id":"x","name":"x","detail":"","pct":250}}],"lines":[{lines}"live a\u001b[31m\r\nb"]}}"#
        );
        let l = json::parse(raw.as_bytes()).unwrap();
        let h = body(Ok(&i), Ok(&l));
        assert!(h.contains("linked"));
        assert!(h.contains("&lt;qwen&gt;"));
        assert!(h.contains("<span class=\"p3-pct\">42%</span>"));
        assert!(h.contains("value=\"42\""));
        assert!(h.contains("value=\"100\""), "pct clamped");
        assert_eq!(h.matches("<progress").count(), 2);
        let term = h.split("<pre class=\"p3-term\">").nth(1).unwrap();
        let term = term.split("</pre>").next().unwrap();
        assert_eq!(term.lines().count(), MAX_LINES);
        assert!(!term.contains("line 20\n"), "oldest dropped");
        assert!(term.ends_with("live a [31m  b"));
        assert!(!term.contains('\u{1b}'));
    }

    #[test]
    fn says_what_did_not_answer() {
        let h = body(
            Err((None, String::new())),
            Err((Some(502), "bad gateway".into())),
        );
        assert!(h.contains("The machine list did not load: the console API did not answer."));
        assert!(h.contains("The laptop status did not load: HTTP 502 (bad gateway)."));
        assert!(h.contains("no answer"));
        assert!(!h.contains("Laptop key"));
        let empty = json::parse(br#"{"nodes":[],"parts":[],"link":{}}"#).unwrap();
        let h = body(Ok(&empty), Ok(&empty));
        assert!(h.contains("No machines in the answer."));
        assert!(h.contains("Nothing answered."));
    }
}
