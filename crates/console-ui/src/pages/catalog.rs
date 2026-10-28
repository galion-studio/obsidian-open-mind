// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Read-only catalog rooms: every value comes from a console API GET, none
//! needs JavaScript. Ports of console-ide `src/routes/_console/{domains,
//! repos,docs,cloudflare,pack,chill,jupyter,data}.tsx` (live tree,
//! 2026-10-20 21:25 CEST).
//!
//! Differences from the TSX, on purpose:
//! - the TSX read its catalog from constants compiled into the browser
//!   bundle (`src/lib/fleet.ts`); here the same lists come from the API
//!   payloads that are built from those constants (`/api/v1/domains`,
//!   `/services`, `/repos`, `/catalog`, `/cloudflare`, `/pack`, `/ledger`,
//!   `/health`), so a catalog edit on the server shows without a rebuild;
//! - the Docs filter is a `GET ?q=` form instead of a keystroke filter;
//! - Copy buttons (`navigator.clipboard`) are gone: the text is in a
//!   `<pre>` that can be selected. Clipboard needs JS;
//! - Repos counts come from the payload; the TSX said "Fourteen public
//!   repositories" while the catalog lists 18 (12 public).

use crate::html::{esc, url_enc};
use crate::icons::icon;
use crate::json::Value;

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).as_str().unwrap_or("")
}

fn items(v: &Value) -> &[Value] {
    v.get("items").as_array()
}

fn missing(what: &str) -> String {
    format!(
        "<p class=\"note\">{} did not load. The console API did not answer.</p>",
        esc(what)
    )
}

fn head(kicker: &str, icon_name: Option<&str>, title: &str, lede: &str) -> String {
    let h1 = if title.is_empty() {
        String::new()
    } else {
        format!(
            "<h1>{}<span class=\"ell\">{}</span></h1>",
            icon_name.map(|n| icon(n, "i i-md")).unwrap_or_default(),
            esc(title)
        )
    };
    format!(
        "<header><p class=\"k\">{}</p>{h1}<p class=\"lede\">{}</p></header>",
        esc(kicker),
        esc(lede)
    )
}

fn badge(tone: &str, text: &str) -> String {
    format!("<span class=\"badge {tone}\">{}</span>", esc(text))
}

/// `/domains`: what each host is (no ping).
pub fn domains(domains: Option<&Value>, services: Option<&Value>) -> String {
    let mut out = String::from("<div class=\"page w46\">");
    out.push_str(&head(
        "Edge",
        Some("edge"),
        "Domain map",
        "What each host is. Live codes are on Nerve. This page does not ping them.",
    ));
    match domains {
        Some(d) if !items(d).is_empty() => {
            out.push_str("<ul class=\"rows\">");
            for it in items(d) {
                let sid = s(it, "serviceId");
                let svc = services
                    .map(items)
                    .unwrap_or(&[])
                    .iter()
                    .find(|x| s(x, "id") == sid)
                    .map(|x| s(x, "name"))
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| s(it, "role"));
                out.push_str(&format!(
                    "<li><span>{}</span><span class=\"muted\">{} · {}</span></li>",
                    esc(s(it, "host")),
                    esc(s(it, "brand")),
                    esc(svc)
                ));
            }
            out.push_str("</ul>");
        }
        _ => out.push_str(&missing("The domain map (/api/v1/domains)")),
    }
    out.push_str("</div>");
    out
}

/// `/repos`: the GitHub inventory.
pub fn repos(v: Option<&Value>) -> String {
    let Some(v) = v.filter(|v| !items(v).is_empty()) else {
        return format!(
            "<div class=\"page w68\">{}{}</div>",
            head("github.com", Some("repos"), "Repositories", ""),
            missing("The repository list (/api/v1/repos)")
        );
    };
    let list = items(v);
    let public = v.get("public").as_f64().unwrap_or(0.0) as u64;
    let private = v.get("privateVisible").as_f64().unwrap_or(0.0) as u64;
    let mut out = String::from("<div class=\"page w68\">");
    out.push_str(&head(
        &format!("github.com/{}", s(v, "owner")),
        Some("repos"),
        "Repositories",
        &format!(
            "{} repositories indexed: {public} public, {private} private visible. Private work outside the current GitHub grant stays sealed.",
            list.len()
        ),
    ));
    out.push_str("<ul class=\"cards\">");
    for r in list {
        let vis = s(r, "visibility");
        let empty = r.get("empty").as_bool() == Some(true);
        let (tone, label) = if empty {
            ("offline", "empty")
        } else if vis == "fork" {
            ("fork", vis)
        } else {
            ("neutral", vis)
        };
        let weight = s(r, "weight");
        let size = r.get("sizeKb").as_f64().unwrap_or(0.0) as u64;
        let lang = r.get("language").as_str().unwrap_or("—");
        out.push_str(&format!(
            "<li><div><p class=\"name\"><span class=\"mono\">{}</span>{}{}</p><p class=\"muted\">{}</p></div>\
<p class=\"mono subtle tiny\">{} · {} · {size} kb</p></li>",
            esc(s(r, "name")),
            badge(tone, label),
            badge(if weight == "heavy" { "held" } else { "neutral" }, weight),
            esc(s(r, "role")),
            esc(lang),
            esc(s(r, "updated")),
        ));
    }
    out.push_str("</ul></div>");
    out
}

fn matches(e: &Value, needle: &str) -> bool {
    needle.is_empty()
        || ["path", "title", "source"]
            .iter()
            .any(|k| s(e, k).to_lowercase().contains(needle))
}

fn entry_list(list: &[&Value], held: bool) -> String {
    if list.is_empty() {
        return "<p class=\"note\">None in this filter.</p>".into();
    }
    let mut out = String::from("<ul class=\"cards\">");
    for e in list {
        out.push_str(&format!(
            "<li class=\"row\"><p class=\"name\">{}<span class=\"mono\">{}</span></p><span class=\"muted\">{}</span></li>",
            badge(if held { "held" } else { "online" }, s(e, "method")),
            esc(s(e, "path")),
            esc(s(e, "title"))
        ));
    }
    out.push_str("</ul>");
    out
}

/// `/docs`: the API ledger with a server-side filter (`?q=`).
pub fn docs(v: Option<&Value>, q: &str) -> String {
    let all = v.map(items).unwrap_or(&[]);
    let needle = q.trim().to_lowercase();
    let shown: Vec<&Value> = all.iter().filter(|e| matches(e, &needle)).collect();
    let live: Vec<&Value> = shown
        .iter()
        .copied()
        .filter(|e| s(e, "zone") == "live")
        .collect();
    let held: Vec<&Value> = shown
        .iter()
        .copied()
        .filter(|e| s(e, "zone") == "held")
        .collect();
    let mut out = String::from("<div class=\"page w68\">");
    out.push_str(&format!(
        "<header><p class=\"k\">Ledger · {} routes</p><p class=\"lede\">Paper trail from galion-console, galion-api-docs, and the backend. Stored in the console ledger. Live answers. VRAM held.</p>\
<form class=\"filter\" method=\"get\" action=\"/docs\" role=\"search\"><input name=\"q\" value=\"{}\" placeholder=\"Filter path, title, source\" aria-label=\"Filter API ledger\"><button class=\"btn ghost\" type=\"submit\">Filter</button></form></header>",
        all.len(),
        esc(q)
    ));
    if v.is_none() {
        out.push_str(&missing("The API ledger (/api/v1/catalog)"));
    } else {
        out.push_str(&format!("<h2>Live · {}</h2>", live.len()));
        out.push_str(&entry_list(&live, false));
        out.push_str(&format!("<h2>Fridge · {}</h2>", held.len()));
        out.push_str(&entry_list(&held, true));
    }
    out.push_str("<p class=\"note\">Gateway or Talk <span class=\"mono\">ledger</span>. <a href=\"/gateway\">Open gateway</a>.</p></div>");
    out
}

/// `/cloudflare`: the tunnel cutover sheet.
pub fn cloudflare(v: Option<&Value>, health: Option<&Value>) -> String {
    let pod = health
        .map(|h| s(h.get("target"), "pod"))
        .filter(|p| !p.is_empty())
        .unwrap_or("—");
    let mut out = String::from("<div class=\"page w46\">");
    out.push_str(&format!(
        "<header><p class=\"k\">Cloudflare</p><p class=\"lede\">Error 1033: the hostname is a tunnel, but DNS still points at the dead one. Point CNAME at this tunnel. Origin stays HTTP on the pod.</p><p class=\"lede\">Cutover · {}</p></header>",
        esc(pod)
    ));
    let Some(v) = v else {
        out.push_str(&missing("The cutover sheet (/api/v1/cloudflare)"));
        out.push_str("</div>");
        return out;
    };
    out.push_str("<ol class=\"steps\">");
    for (i, st) in v.get("steps").as_array().iter().enumerate() {
        out.push_str(&format!(
            "<li><span class=\"subtle\">{}</span><p>{}</p></li>",
            i + 1,
            esc(st.as_str().unwrap_or(""))
        ));
    }
    out.push_str("</ol>");
    let cname = s(v.get("tunnel"), "cname");
    out.push_str(&format!(
        "<ul class=\"rows\"><li><span class=\"muted\">CNAME target: {}</span></li><li><span class=\"muted\">RunPod proxy (debug only): {}</span></li></ul>",
        esc(cname),
        esc(s(v, "proxyHint"))
    ));
    for z in v.get("zones").as_array() {
        out.push_str(&format!(
            "<section><h2 class=\"split\"><span>{}</span><span>CNAME → tunnel</span></h2><ul class=\"rows\">",
            esc(s(z, "zone"))
        ));
        let mut plain = String::new();
        for r in z.get("records").as_array() {
            out.push_str(&format!(
                "<li class=\"dns\"><span>{}</span><span>{}</span><span class=\"muted\">{}</span><span class=\"muted\">orange</span></li>",
                esc(s(r, "type")),
                esc(s(r, "name")),
                esc(s(r, "host"))
            ));
            plain.push_str(&format!(
                "{} {} {} {}\n",
                s(r, "type"),
                s(r, "name"),
                cname,
                s(r, "host")
            ));
        }
        out.push_str(&format!(
            "</ul><details class=\"plain\"><summary>Records as text</summary><pre class=\"out\">{}</pre></details></section>",
            esc(plain.trim_end())
        ));
    }
    out.push_str("</div>");
    out
}

/// `/pack`: the chilled compose and Caddyfile (read only, never run).
pub fn pack(v: Option<&Value>) -> String {
    let provider = v.map(|v| s(v.get("target"), "provider")).unwrap_or("");
    let mut out = String::from("<div class=\"page w68\">");
    out.push_str(&format!(
        "<header><p class=\"name\">{}<span class=\"k ice\">{} · not this pass</span></p><h1 class=\"frost\">{}<span class=\"ell\">Chilled pack</span></h1>\
<p class=\"lede ice\">Docker compose is in the fridge. Active zone uses a single Python origin on the pod. Leave this here until we thaw it.</p>\
<a class=\"frost\" href=\"/chill\">Back to the fridge</a></header>",
        badge("chill", "chill zone"),
        esc(provider),
        icon("pack", "i i-md")
    ));
    match v {
        Some(v) => {
            for (title, key) in [
                ("docker-compose.light.yml", "compose"),
                ("Caddyfile", "caddy"),
            ] {
                out.push_str(&format!(
                    "<section class=\"cold-file\"><h2>{title}</h2><pre class=\"code\">{}</pre></section>",
                    esc(s(v, key))
                ));
            }
        }
        None => out.push_str(&missing("The pack (/api/v1/pack)")),
    }
    out.push_str("</div>");
    out
}

/// `/chill`: heavy services and the fridge extras (`/api/v1/ledger`).
pub fn chill(ledger: Option<&Value>) -> String {
    let mut out = String::from("<div class=\"page w68\">");
    out.push_str(&format!(
        "<header><p class=\"k ice\">Winter · fridge · hibernate</p><h1 class=\"frost\">{}<span class=\"ell\">Chill zone.</span></h1>\
<p class=\"lede ice\">Everything we already built that this pass does not need. Docker stays here. Generation stays here. They are cold on purpose.</p></header>",
        icon("fridge", "i i-md")
    ));
    match ledger {
        Some(l) => {
            out.push_str("<ul class=\"cards ice-cards\">");
            let heavy = l
                .get("services")
                .as_array()
                .iter()
                .filter(|x| s(x, "weight") == "heavy");
            for x in heavy.chain(l.get("fridge").as_array().iter()) {
                out.push_str(&format!(
                    "<li><div><p class=\"name\"><span>{}</span>{}</p><p class=\"ice\">{}</p></div></li>",
                    esc(s(x, "name")),
                    badge("chill", "asleep"),
                    esc(s(x, "summary"))
                ));
            }
            out.push_str("</ul>");
        }
        None => out.push_str(&missing("The fridge list (/api/v1/ledger)")),
    }
    out.push_str("<p class=\"note ice\">Compose lives in the fridge too. <a class=\"frost\" href=\"/pack\">View chilled pack</a></p></div>");
    out
}

/// The pod id is spliced into a hostname: keep it to `[A-Za-z0-9-]`.
pub fn pod_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `/jupyter`: link to the notebook on the pod (`target.podId` from health).
pub fn jupyter(health: Option<&Value>) -> String {
    let pod = health.map(|h| s(h.get("target"), "podId")).unwrap_or("");
    let mut out = String::from(
        "<div class=\"page w46\"><header><p class=\"k\">From dashboard/jupyter</p><p class=\"lede\">Notebook on the pod. CPU only. This light boot does not embed the full lab.</p></header>",
    );
    if pod_id_ok(pod) {
        let href = format!("https://{}-8888.proxy.runpod.net/lab", url_enc(pod));
        out.push_str(&format!(
            "<div class=\"actions\"><a class=\"btn primary\" href=\"{0}\">Open Jupyter on pod</a></div><p class=\"note mono\">{0}</p>",
            esc(&href)
        ));
    } else {
        out.push_str(&missing("The pod id (/api/v1/health target.podId)"));
    }
    out.push_str("</div>");
    out
}

/// `/data`: static note (no database server on the light boot).
pub fn data() -> String {
    "<div class=\"page w42\"><header><p class=\"k\">Obsidian OpenMind</p><p class=\"lede\">This light boot has no database server. Hermes reads through the db-read skill. A write waits for you.</p>\
<a href=\"/services\">Services</a></header></div>"
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn docs_filter_is_case_insensitive_and_escaped() {
        let v = parse(br#"{"items":[{"method":"GET","path":"/api/v1/health","title":"Health","source":"console","zone":"live"},{"method":"POST","path":"/api/v3/voice","title":"Voice","source":"backend","zone":"held"}]}"#).unwrap();
        let all = docs(Some(&v), "");
        assert!(all.contains("Live · 1") && all.contains("Fridge · 1"));
        let f = docs(Some(&v), "VOICE");
        assert!(f.contains("Live · 0") && f.contains("Fridge · 1"));
        assert!(f.contains("None in this filter."));
        let x = docs(Some(&v), "\"><script>");
        assert!(!x.contains("<script>"));
        assert!(docs(None, "").contains("did not load"));
    }

    #[test]
    fn repos_badges_follow_tsx_tones() {
        let v = parse(br#"{"owner":"o","public":1,"privateVisible":0,"items":[{"name":"a","visibility":"fork","weight":"heavy","role":"r","updated":"u","sizeKb":3},{"name":"b","visibility":"public","empty":true,"weight":"light","role":"r","updated":"u","sizeKb":0,"language":"Rust"}]}"#).unwrap();
        let h = repos(Some(&v));
        assert!(h.contains(
            "<span class=\"badge fork\">fork</span><span class=\"badge held\">heavy</span>"
        ));
        assert!(h.contains("<span class=\"badge offline\">empty</span>"));
        assert!(h.contains("— · u · 3 kb") && h.contains("Rust · u · 0 kb"));
        assert!(h.contains("2 repositories indexed: 1 public, 0 private visible"));
    }

    #[test]
    fn jupyter_refuses_odd_pod_ids() {
        let ok = parse(br#"{"target":{"podId":"ip50"}}"#).unwrap();
        assert!(jupyter(Some(&ok)).contains("https://ip50-8888.proxy.runpod.net/lab"));
        let bad = parse(br#"{"target":{"podId":"x.evil.example/"}}"#).unwrap();
        assert!(!jupyter(Some(&bad)).contains("href"));
        assert!(!jupyter(None).contains("href"));
    }

    #[test]
    fn domains_fall_back_to_role() {
        let d = parse(br#"{"items":[{"host":"a.example","brand":"B","role":"Role","serviceId":"zz"},{"host":"c.example","brand":"C","role":"R2","serviceId":"s"}]}"#).unwrap();
        let sv = parse(br#"{"items":[{"id":"s","name":"Svc"}]}"#).unwrap();
        let h = domains(Some(&d), Some(&sv));
        assert!(h.contains("B · Role") && h.contains("C · Svc"));
    }
}
