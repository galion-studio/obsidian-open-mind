// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/instances` with console-static `--instances DIR` (console-instances,
//! 2026-11-01): the owner's devices, each with CPU, RAM and GPU/VRAM, an
//! "Add a device" button that shows a one-time code and a one-line join
//! command, and Rename / Remove. Plus the public `/join` page a phone or
//! tablet uses to join with that code.
//!
//! Server-rendered, plain forms, no script on `/instances`. Every value
//! that came from a device is escaped and capped. The view JSON never holds
//! a token or an IP address (only "same network" yes/no).

use super::p3::{cap, plain, s};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// A code just made, shown once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fresh {
    /// `ABCD-EFGH`.
    pub code: String,
    /// The address a device uses, e.g. `https://console.example`.
    pub console_url: String,
    /// Minutes the code works.
    pub minutes: u64,
    /// LAN discovery is on (the short command works on the same network).
    pub lan: bool,
}

/// One line to the owner after a form post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub ok: bool,
    pub text: String,
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).as_f64().filter(|x| x.is_finite() && *x >= 0.0)
}

/// `16384` MB -> `16 GB`, `1536` -> `1.5 GB`, `512` -> `512 MB`.
pub fn size(mb: f64) -> String {
    if mb < 1024.0 {
        format!("{} MB", mb.round() as u64)
    } else {
        let gb = mb / 1024.0;
        if gb >= 100.0 || (gb - gb.round()).abs() < 0.05 {
            format!("{} GB", gb.round() as u64)
        } else {
            format!("{gb:.1} GB")
        }
    }
}

/// Seconds -> "just now", "4 min ago", "3 h ago", "2 days ago".
pub fn ago(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", s / 86_400),
    }
}

fn bar(pct: f64, label: &str) -> String {
    let p = pct.clamp(0.0, 100.0).round() as u32;
    format!(
        "<progress class=\"p3-bar\" max=\"100\" value=\"{p}\" aria-label=\"{}\">{p}%</progress>",
        esc(label)
    )
}

fn gauge(title: &str, value: &str, sub: &str, pct: Option<f64>) -> String {
    let mut out = format!(
        "<div class=\"fl-g\"><p class=\"k\">{}</p><p class=\"fl-v\">{}</p>",
        esc(title),
        esc(&cap(&plain(value), 120))
    );
    if !sub.is_empty() {
        out.push_str(&format!(
            "<p class=\"fl-s\">{}</p>",
            esc(&cap(&plain(sub), 160))
        ));
    }
    if let Some(p) = pct {
        out.push_str(&bar(p, &format!("{title} {value}")));
    }
    out.push_str("</div>");
    out
}

/// CPU, RAM and GPU gauges for one reading (`metrics` object).
pub fn gauges(m: &Value) -> String {
    let mut out = String::from("<div class=\"fl-gauges\">");
    // CPU
    let cores = num(m, "cores").map(|c| c as u64);
    let cpu_sub = [
        plain(s(m, "cpu_model")),
        cores
            .map(|c| format!("{c} core{}", if c == 1 { "" } else { "s" }))
            .unwrap_or_default(),
    ]
    .into_iter()
    .filter(|x| !x.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let cpu_pct = num(m, "cpu_pct").map(|p| p.min(100.0));
    out.push_str(&gauge(
        "Processor (CPU)",
        &cpu_pct
            .map(|p| format!("{}% busy", p.round() as u32))
            .unwrap_or_else(|| "Not reported".into()),
        &cpu_sub,
        cpu_pct,
    ));
    // RAM
    let total = num(m, "ram_total_mb").filter(|t| *t > 0.0);
    let used = num(m, "ram_used_mb");
    let (v, pct) = match (used, total) {
        (Some(u), Some(t)) => (
            format!("{} of {} used", size(u.min(t)), size(t)),
            Some(u * 100.0 / t),
        ),
        (None, Some(t)) => (format!("{} in total", size(t)), None),
        _ => ("Not reported".into(), None),
    };
    out.push_str(&gauge("Memory (RAM)", &v, "", pct));
    // GPU(s)
    let gpus = m.get("gpus").as_array();
    if gpus.is_empty() {
        out.push_str(&gauge(
            "Graphics (GPU)",
            "None found",
            "Runs on the processor. That is fine: Galion works without a graphics card.",
            None,
        ));
    } else {
        for g in gpus.iter().take(8) {
            let vt = num(g, "vram_total_mb").filter(|t| *t > 0.0);
            let vu = num(g, "vram_used_mb");
            let (v, pct) = match (vu, vt) {
                (Some(u), Some(t)) => (
                    format!("{} of {} video memory used", size(u.min(t)), size(t)),
                    Some(u * 100.0 / t),
                ),
                (None, Some(t)) => (format!("{} video memory", size(t)), None),
                _ => ("Video memory not reported".into(), None),
            };
            let busy = num(g, "busy_pct")
                .map(|b| format!(" · {}% busy", b.min(100.0).round() as u32))
                .unwrap_or_default();
            let name = s(g, "name");
            out.push_str(&gauge(
                "Graphics (GPU)",
                &v,
                &format!(
                    "{}{busy}",
                    if name.is_empty() {
                        "Graphics card"
                    } else {
                        name
                    }
                ),
                pct,
            ));
        }
    }
    out.push_str("</div>");
    let models: Vec<String> = m
        .get("models")
        .as_array()
        .iter()
        .filter_map(Value::as_str)
        .map(|x| match x {
            "ollama" => "Ollama".to_string(),
            other => plain(other),
        })
        .take(4)
        .collect();
    if !models.is_empty() {
        out.push_str(&format!(
            "<p class=\"fl-s fl-ok\">Has a model running: {}.</p>",
            esc(&cap(&models.join(", "), 120))
        ));
    }
    out
}

fn os_line(m: &Value) -> String {
    [plain(s(m, "os")), plain(s(m, "arch"))]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn device(d: &Value) -> String {
    let id = s(d, "id");
    let name = cap(&plain(s(d, "name")), 60);
    let removed = d.get("removed").as_bool() == Some(true);
    let online = d.get("online").as_bool() == Some(true);
    let seen = num(d, "seen_ago").unwrap_or(f64::MAX);
    let badge = if removed {
        "<span class=\"badge neutral\">Removed</span>".to_string()
    } else if online {
        "<span class=\"badge online\">Online</span>".to_string()
    } else {
        format!(
            "<span class=\"badge offline\">Seen {}</span>",
            esc(&ago(seen))
        )
    };
    let m = d.get("metrics");
    let how = [
        if d.get("lan").as_bool() == Some(true) {
            "Same network".to_string()
        } else {
            "Over the internet".to_string()
        },
        if s(d, "via") == "browser" {
            "joined in a browser".to_string()
        } else {
            "galion-join app".to_string()
        },
        os_line(m),
    ]
    .into_iter()
    .filter(|x| !x.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let mut out = format!(
        "<li class=\"fl-dev\"><div class=\"p3-top\"><span class=\"p3-name ell\">{}</span>{badge}</div><p class=\"fl-s\">{}</p>",
        esc(&name),
        esc(&cap(&how, 200))
    );
    if removed {
        out.push_str("<p class=\"fl-s\">This device can no longer report. To add it back, get a new code.</p></li>");
        return out;
    }
    if !online {
        out.push_str("<p class=\"fl-s\">Last reading below. The device shows as online again when it reports (every 30 seconds while galion-join or the join page runs).</p>");
    }
    out.push_str(&gauges(m));
    let id = esc(&cap(&plain(id), 40));
    out.push_str(&format!(
        "<details class=\"fl-more\"><summary>Rename or remove</summary>\
<form method=\"post\" action=\"/instances\" class=\"fl-row\"><input type=\"hidden\" name=\"op\" value=\"rename\"><input type=\"hidden\" name=\"id\" value=\"{id}\">\
<input name=\"name\" maxlength=\"40\" required aria-label=\"New name\" placeholder=\"New name\" value=\"{}\"><button class=\"btn ghost\" type=\"submit\">Rename</button></form>\
<form method=\"post\" action=\"/instances\" class=\"fl-row\"><input type=\"hidden\" name=\"op\" value=\"forget\"><input type=\"hidden\" name=\"id\" value=\"{id}\">\
<button class=\"btn ghost fl-bad\" type=\"submit\">Remove this device</button><span class=\"fl-s\">It stops reporting at once. Its files are not touched.</span></form></details></li>",
        esc(&name)
    ));
    out
}

fn fresh_block(f: &Fresh) -> String {
    let url = esc(&cap(&plain(&f.console_url), 200));
    let code = esc(&cap(&plain(&f.code), 12));
    let mut out = format!(
        "<section class=\"fl-add\" aria-live=\"polite\"><p class=\"k\">Your code</p><p class=\"fl-code\">{code}</p>\
<p class=\"fl-s\">Works once, for the next {} minutes. It is shown only now; if you leave this page, just get a new one.</p>\
<h2>On a laptop or desktop (Mac, Windows, Linux)</h2><p class=\"fl-s\">Open a terminal and run this one line:</p>\
<pre class=\"p3-pre\">galion-join {code} --console {url}</pre>",
        f.minutes
    );
    if f.lan {
        out.push_str(&format!(
            "<p class=\"fl-s\">Same Wi-Fi or cable as this console? This shorter line finds it by itself, even with the internet down:</p><pre class=\"p3-pre\">galion-join {code}</pre>"
        ));
    }
    out.push_str(&format!(
        "<h2>On a phone or tablet</h2><p class=\"fl-s\">Open <b>{url}/join</b> in the browser and type the code.</p></section>"
    ));
    out
}

/// The `/instances` body.
pub fn body(view: &Value, fresh: Option<&Fresh>, notice: Option<&Notice>) -> String {
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Instances</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">Your devices</span></h1>\
<p class=\"lede\">Every computer and phone that works with Galion. Galion runs on the processor (CPU) and also uses a graphics card (GPU) when a device has one.</p>\
<p class=\"p3-refresh\"><a href=\"/instances\">Refresh</a></p></header>",
        icon("instances", "i i-md")
    ));
    if let Some(n) = notice {
        out.push_str(&format!(
            "<p class=\"note {}\" role=\"status\">{}</p>",
            if n.ok { "fl-ok" } else { "p3-bad" },
            esc(&cap(&plain(&n.text), 300))
        ));
    }

    // Add a device.
    match fresh {
        Some(f) => out.push_str(&fresh_block(f)),
        None => {
            let waiting = num(view, "pending_codes").unwrap_or(0.0) as u64;
            out.push_str(
                "<section class=\"fl-add\"><h2>Add a device</h2>\
<p class=\"fl-s\">Get a one-time code, then use it on the laptop, desktop or phone you want to add. Galion sets up the connection by itself.</p>\
<form method=\"post\" action=\"/instances\" class=\"fl-row\"><input type=\"hidden\" name=\"op\" value=\"add\">\
<input name=\"name\" maxlength=\"40\" aria-label=\"Name for the device (optional)\" placeholder=\"Name, e.g. Studio laptop (optional)\">\
<button class=\"btn primary\" type=\"submit\">Get a code</button></form>",
            );
            if waiting > 0 {
                out.push_str(&format!(
                    "<p class=\"fl-s\">{waiting} unused code{} still waiting (each stops working 10 minutes after it was made).</p>",
                    if waiting == 1 { " is" } else { "s are" }
                ));
            }
            out.push_str("</section>");
        }
    }

    // Summary.
    let devices = view.get("devices").as_array();
    let live: Vec<&Value> = devices
        .iter()
        .filter(|d| d.get("removed").as_bool() != Some(true))
        .collect();
    let online = live
        .iter()
        .filter(|d| d.get("online").as_bool() == Some(true))
        .count();
    let with_gpu = live
        .iter()
        .filter(|d| !d.get("metrics").get("gpus").as_array().is_empty())
        .count();
    out.push_str(&format!(
        "<section class=\"p3-sec\"><div class=\"p3-top p3-h\"><h2>Devices</h2><span class=\"fl-s\">{} added · {online} online · {with_gpu} with a graphics card</span></div>",
        live.len()
    ));

    // This console first.
    let me = view.get("console");
    if !me.is_null() {
        out.push_str(&format!(
            "<ul class=\"p3-list\"><li class=\"fl-dev\"><div class=\"p3-top\"><span class=\"p3-name ell\">{}</span><span class=\"badge online\">This console</span></div><p class=\"fl-s\">{}</p>{}</li></ul>",
            esc(&cap(&plain(s(me, "name")), 60)),
            esc(&cap(
                &["The server this page comes from".to_string(), os_line(me.get("metrics"))]
                    .into_iter()
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
                200
            )),
            gauges(me.get("metrics"))
        ));
    }
    if devices.is_empty() {
        out.push_str("<p class=\"note\">No devices added yet. Press <b>Get a code</b> above to add your first laptop or phone.</p>");
    } else {
        out.push_str("<ul class=\"p3-list\">");
        for d in devices.iter().take(200) {
            out.push_str(&device(d));
        }
        out.push_str("</ul>");
    }
    out.push_str("</section>");

    out.push_str(
        "<section class=\"p3-sec\"><h2>How it connects</h2><ul class=\"fl-help\">\
<li>A device joins with a code that works once and only for 10 minutes. The console keeps only a fingerprint of each device key, never the key itself.</li>\
<li>On the same network the device finds the console by itself (a local broadcast), so it keeps working with the internet down.</li>\
<li>Each device sends its processor, memory and graphics card readings every 30 seconds. Nothing else is read from it.</li>\
<li>Remove a device to stop it at once. Its own files are never touched.</li></ul></section></div>",
    );
    out
}

/// What the public `/join` page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinState {
    /// The code form (with an error line if a try failed).
    Ask { error: Option<String> },
    /// This browser has joined as `name`.
    Joined { name: String, fresh: bool },
}

/// The `/join` body (for phones and tablets; works without the script,
/// the script only adds the readings and keeps the device "online").
pub fn join_body(state: &JoinState) -> String {
    let mut out = String::from("<section class=\"gate fl-join\"><p class=\"k\">Galion</p>");
    match state {
        JoinState::Ask { error } => {
            out.push_str("<h1>Add this device</h1><p class=\"lede\">Type the code shown on the console's Instances page.</p>");
            if let Some(e) = error {
                out.push_str(&format!(
                    "<p class=\"note p3-bad\" role=\"alert\">{}</p>",
                    esc(&cap(&plain(e), 200))
                ));
            }
            out.push_str(
                "<form method=\"post\" action=\"/join\" id=\"fl-join\">\
<input class=\"fl-codein\" name=\"code\" required maxlength=\"12\" autocomplete=\"one-time-code\" autocapitalize=\"characters\" spellcheck=\"false\" aria-label=\"Code\" placeholder=\"ABCD-EFGH\">\
<input class=\"fl-codein fl-namein\" name=\"name\" maxlength=\"40\" aria-label=\"Name for this device (optional)\" placeholder=\"Name (optional)\">\
<input type=\"hidden\" name=\"metrics\" id=\"fl-m\" value=\"\">\
<button class=\"btn primary\" type=\"submit\">Add this device</button></form>",
            );
        }
        JoinState::Joined { name, fresh } => {
            out.push_str(&format!(
                "<h1>{}</h1><p class=\"lede\">This device is on the console as <b>{}</b>. Keep this page open to show it as online; it reports every 30 seconds.</p>\
<p class=\"note\" id=\"fl-st\" role=\"status\"></p>",
                if *fresh { "Added" } else { "Connected" },
                esc(&cap(&plain(name), 60))
            ));
        }
    }
    out.push_str("</section>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    const VIEW: &str = r#"{"pending_codes":1,"console":{"name":"Galion console","metrics":{"os":"Ubuntu 24.04","arch":"x86_64","cpu_model":"AMD EPYC","cores":8,"cpu_pct":12.5,"ram_total_mb":16000,"ram_used_mb":4000,"gpus":[],"models":[]}},
"devices":[
{"id":"a1","name":"Studio <laptop>","via":"agent","lan":true,"removed":false,"online":true,"seen_ago":3,"metrics":{"os":"Windows 11","cpu_model":"Intel i7","cores":16,"cpu_pct":40,"ram_total_mb":65536,"ram_used_mb":20480,"gpus":[{"name":"NVIDIA GeForce RTX 4060 Laptop GPU","vram_total_mb":8188,"vram_used_mb":1024,"busy_pct":7}],"models":["ollama"]}},
{"id":"b2","name":"Phone","via":"browser","lan":false,"removed":false,"online":false,"seen_ago":400,"metrics":{"cores":6,"ram_total_mb":4096,"gpus":[{"name":"Apple GPU"}]}},
{"id":"c3","name":"Old","via":"agent","lan":false,"removed":true,"online":false,"seen_ago":99999,"metrics":{}}]}"#;

    #[test]
    fn renders_devices_and_gauges() {
        let v = json::parse(VIEW.as_bytes()).unwrap();
        let h = body(&v, None, None);
        assert!(h.contains("Your devices"));
        assert!(h.contains("Get a code"));
        assert!(h.contains("1 unused code is still waiting"));
        assert!(h.contains("2 added · 1 online · 2 with a graphics card"));
        assert!(h.contains("Studio &lt;laptop&gt;"));
        assert!(!h.contains("<laptop>"));
        assert!(h.contains("40% busy"));
        assert!(h.contains("Intel i7 · 16 cores"));
        assert!(h.contains("20 GB of 64 GB used"));
        assert!(h.contains("1 GB of 8 GB video memory used"));
        assert!(h.contains("NVIDIA GeForce RTX 4060 Laptop GPU · 7% busy"));
        assert!(h.contains("Has a model running: Ollama."));
        assert!(h.contains("Same network · galion-join app · Windows 11"));
        assert!(h.contains("Seen 6 min ago"));
        assert!(h.contains("Over the internet · joined in a browser"));
        assert!(h.contains("Video memory not reported"));
        assert!(h.contains("<span class=\"badge neutral\">Removed</span>"));
        assert!(h.contains("This console"));
        assert!(h.contains("None found"), "the console has no GPU");
        assert!(h.contains("3.9 GB of 15.6 GB used"));
        assert_eq!(h.matches("value=\"rename\"").count(), 2, "not for removed");
        assert!(!h.contains("<script"));
        assert!(!h.contains("galion-join ABCD"));
    }

    #[test]
    fn fresh_code_shows_both_ways() {
        let v = json::parse(br#"{"devices":[]}"#).unwrap();
        let f = Fresh {
            code: "K7QM-3XPD".into(),
            console_url: "https://console.example".into(),
            minutes: 10,
            lan: true,
        };
        let h = body(
            &v,
            Some(&f),
            Some(&Notice {
                ok: true,
                text: "Code ready.".into(),
            }),
        );
        assert!(h.contains("<p class=\"fl-code\">K7QM-3XPD</p>"));
        assert!(h.contains("galion-join K7QM-3XPD --console https://console.example"));
        assert!(h.contains("<pre class=\"p3-pre\">galion-join K7QM-3XPD</pre>"));
        assert!(h.contains("https://console.example/join"));
        assert!(!h.contains("value=\"add\""));
        assert!(h.contains("No devices added yet"));
        let h = body(&v, Some(&Fresh { lan: false, ..f }), None);
        assert!(!h.contains("<pre class=\"p3-pre\">galion-join K7QM-3XPD</pre>"));
    }

    #[test]
    fn helpers() {
        assert_eq!(size(512.0), "512 MB");
        assert_eq!(size(1536.0), "1.5 GB");
        assert_eq!(size(16384.0), "16 GB");
        assert_eq!(ago(5.0), "just now");
        assert_eq!(ago(130.0), "2 min ago");
        assert_eq!(ago(7300.0), "2 h ago");
        assert_eq!(ago(90_000.0), "yesterday");
        assert_eq!(ago(300_000.0), "3 days ago");
    }

    #[test]
    fn join_page() {
        let h = join_body(&JoinState::Ask {
            error: Some("Wrong <code>".into()),
        });
        assert!(h.contains("action=\"/join\""));
        assert!(h.contains("Wrong &lt;code&gt;"));
        let h = join_body(&JoinState::Joined {
            name: "My <phone>".into(),
            fresh: true,
        });
        assert!(h.contains("Added"));
        assert!(h.contains("My &lt;phone&gt;"));
    }

    #[test]
    fn fleet_classes_are_defined() {
        let v = json::parse(VIEW.as_bytes()).unwrap();
        let f = Fresh {
            code: "K7QM-3XPD".into(),
            console_url: "https://x".into(),
            minutes: 10,
            lan: true,
        };
        let all = format!(
            "{}{}{}",
            body(
                &v,
                None,
                Some(&Notice {
                    ok: false,
                    text: "x".into()
                })
            ),
            body(
                &v,
                Some(&f),
                Some(&Notice {
                    ok: true,
                    text: "x".into()
                })
            ),
            join_body(&JoinState::Ask { error: None })
        );
        let defined = crate::css::defined_classes();
        for chunk in all.split("class=\"").skip(1) {
            let list = chunk.split('"').next().unwrap();
            for c in list.split_whitespace() {
                assert!(defined.contains(&c.to_string()), "class {c} has no rule");
            }
        }
    }
}
