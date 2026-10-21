// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/security` (Defence, "The fort"): port of console-ide
//! `src/routes/_console/security.tsx` (live tree, 2026-10-20 21:25 CEST).
//! console-port-5, wave 4 slot 139.
//!
//! Data (all need the paired phone; the browser's admin cookie is
//! forwarded): `GET /api/v1/security` (guard state, floor, secret-file
//! modes), `GET /api/v3/guardian/status`, `GET /api/v3/guardian/incidents`.
//! Scan and Heal are same-origin-checked form posts (see `port5.rs`).
//!
//! Differences from the TSX, on purpose:
//! - no 5 s poll: a Refresh link (the JS ruling, decision 1);
//! - Heal asks for the Defender PIN in a password field (the TSX used
//!   `window.prompt`); the PIN goes upstream only as `X-Galion-Pin` and is
//!   never rendered or logged;
//! - the **Denylist switch is shown read-only**: it posts
//!   `POST /api/hermes/guard {"on":…}`, i.e. it switches Hermes's command
//!   denylist off. Porting it is an owner decision (as for the Hermes
//!   switches on `/hermes`, console-port-2 decision 2);
//! - the TSX showed defaults when a source failed (Denylist "on", score 0,
//!   "None open."), which reads as a healthy fort while nothing loaded; here
//!   each source that did not load says so (live, unpaired: `HTTP 401
//!   (phone required)`) and its values show "—";
//! - the last-scan clock is Europe/Zagreb with the zone named, not UTC.

use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;
use crate::pages::session::Miss;

/// Everything the page was given.
pub struct View<'a> {
    pub guard: Result<&'a Value, &'a Miss>,
    pub guardian: Result<&'a Value, &'a Miss>,
    pub incidents: Result<&'a Value, &'a Miss>,
    /// Result line of a Scan / Heal post.
    pub note: &'a str,
}

/// Defender PIN accepted for the header: 1-64 visible ASCII, no spaces.
pub fn pin_ok(pin: &str) -> bool {
    !pin.is_empty() && pin.len() <= 64 && pin.bytes().all(|b| b.is_ascii_graphic())
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of the last Sunday of `month`, 01:00 UTC (EU switch).
fn last_sunday_0100(year: i64, month: i64) -> i64 {
    let last = days_from_civil(year, month, 31);
    // 1970-01-01 was a Thursday (weekday 4, Sunday 0).
    let wd = (last + 4).rem_euclid(7);
    (last - wd) * 86_400 + 3_600
}

/// Europe/Zagreb offset in hours (CET +1, CEST +2) at Unix seconds.
pub fn zagreb_offset(secs: i64) -> i64 {
    let year = 1970 + secs.div_euclid(86_400 * 365 + 86_400 / 4);
    // Year estimate can be off by one near New Year; both are winter time.
    if secs >= last_sunday_0100(year, 3) && secs < last_sunday_0100(year, 10) {
        2
    } else {
        1
    }
}

/// `2026-10-21T03:12:45.123Z` (or with `+hh:mm`) -> Unix seconds.
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        let k = r.bytes().take_while(u8::is_ascii_digit).count();
        rest = &r[k..];
    }
    let off = match rest {
        "Z" | "z" => 0,
        r if r.len() == 6 && (r.starts_with('+') || r.starts_with('-')) && &r[3..4] == ":" => {
            let hh: i64 = r[1..3].parse().ok()?;
            let mm: i64 = r[4..6].parse().ok()?;
            let o = hh * 3600 + mm * 60;
            if r.starts_with('-') {
                -o
            } else {
                o
            }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - off)
}

/// `HH:MM:SS CEST` on the Zagreb clock (the TSX printed `HH:MM:SS UTC`).
pub fn zagreb_clock(iso: &str) -> Option<String> {
    let t = parse_iso(iso)?;
    let off = zagreb_offset(t);
    let local = (t + off * 3600).rem_euclid(86_400);
    Some(format!(
        "{:02}:{:02}:{:02} {}",
        local / 3600,
        local % 3600 / 60,
        local % 60,
        if off == 2 { "CEST" } else { "CET" }
    ))
}

fn num(v: &Value) -> Option<String> {
    v.as_f64().filter(|n| n.is_finite()).map(|n| {
        if n.fract() == 0.0 {
            format!("{}", n as i64)
        } else {
            format!("{n}")
        }
    })
}

fn row(label: &str, value: &str, warn: bool) -> String {
    format!(
        "<li><span>{}</span><span class=\"{}\">{}</span></li>",
        esc(label),
        if warn { "err" } else { "muted" },
        esc(value)
    )
}

fn missing(what: &str, m: &Miss) -> String {
    format!(
        "<p class=\"err\">{} did not load: {}</p>",
        esc(what),
        esc(&m.say())
    )
}

/// TSX `FileList`: last two path segments, mode, ok.
fn files(rows: &[Value]) -> String {
    let mut out = String::from("<ul class=\"rows\">");
    for r in rows {
        let path = r.get("path").as_str().unwrap_or("");
        let segs: Vec<&str> = path.split('/').collect();
        let short = segs[segs.len().saturating_sub(2)..].join("/");
        out.push_str(&row(
            &short,
            r.get("mode").as_str().unwrap_or(""),
            r.get("ok").as_bool() != Some(true),
        ));
    }
    out.push_str("</ul>");
    out
}

pub fn body(v: &View<'_>) -> String {
    let g = v.guardian.ok();
    let guard = v.guard.ok();
    let mut out = String::from("<div class=\"page w46\"><header><p class=\"k\">Defence</p>");
    out.push_str(&format!(
        "<h1>{}<span class=\"ell\">The fort</span></h1>",
        icon("defence", "i i-md")
    ));
    let beat = g
        .and_then(|g| g.get("health").as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("no beat yet");
    let scan = g
        .and_then(|g| g.get("lastScan").as_str())
        .and_then(zagreb_clock)
        .map(|c| format!(" · {c}"))
        .unwrap_or_default();
    out.push_str(&format!(
        "<p class=\"lede\">Watch · 24/7 · {}{}</p><a href=\"/security\">Refresh</a></header>",
        esc(beat),
        esc(&scan)
    ));
    if let Err(m) = v.guard {
        out.push_str(&missing("Security", m));
    }
    if let Err(m) = v.guardian {
        out.push_str(&missing("Guardian status", m));
    }
    if !v.note.is_empty() {
        out.push_str(&format!("<p class=\"note\">{}</p>", esc(v.note)));
    }

    let dash = "—".to_string();
    let alert = g.and_then(|g| g.get("status").as_str()) == Some("alert");
    let denylist = guard.map(|x| {
        if x.get("denylist").as_bool() == Some(false) {
            "off"
        } else {
            "on"
        }
    });
    out.push_str("<ul class=\"rows\">");
    out.push_str(&row(
        "Score",
        &g.map(|g| num(g.get("score")).unwrap_or_else(|| "0".into()))
            .unwrap_or(dash.clone()),
        alert,
    ));
    out.push_str(&row("Floor", "on", false));
    out.push_str(&row("Denylist", denylist.unwrap_or("—"), false));
    out.push_str(&row(
        "Incidents",
        &g.map(|g| num(g.get("activeIncidents")).unwrap_or_else(|| "0".into()))
            .unwrap_or(dash.clone()),
        false,
    ));
    out.push_str(&row(
        "Notes",
        &g.map(|g| num(g.get("library").get("count")).unwrap_or_else(|| "0".into()))
            .unwrap_or(dash.clone()),
        false,
    ));
    out.push_str("</ul>");

    // Scan / Heal: same-origin form posts; the PIN only travels as a header.
    out.push_str(
        "<div class=\"actions\"><form method=\"post\" action=\"/security\"><input type=\"hidden\" name=\"act\" value=\"scan\"><button class=\"btn ghost\" type=\"submit\">Scan</button></form>\
<a class=\"btn ghost\" href=\"/session\">Session notes</a><a class=\"btn ghost\" href=\"/hermes\">Hermes</a></div>\
<form class=\"filter\" method=\"post\" action=\"/security\" autocomplete=\"off\"><input type=\"hidden\" name=\"act\" value=\"heal\">\
<input type=\"password\" name=\"pin\" aria-label=\"Defender PIN\" placeholder=\"Defender PIN\" maxlength=\"64\" autocomplete=\"off\">\
<button class=\"btn ghost\" type=\"submit\">Heal secrets</button></form>",
    );

    out.push_str("<h2>Controls</h2><ul class=\"rows\">");
    out.push_str(&row("Denylist", denylist.unwrap_or("—"), false));
    out.push_str("</ul>");
    let auto = guard.map(|x| {
        if x.get("autoAccept").as_bool() == Some(true) {
            "on"
        } else {
            "off"
        }
    });
    out.push_str(&format!(
        "<p class=\"lede\">Floor stays on when the denylist is off. Auto-accept is {} and stays on the Hermes drawer. History redaction is on. This phone is the only admin.</p>\
<p class=\"note\">The denylist switch is read-only here until Slavik decides (it turns Hermes's command denylist off).</p>",
        auto.unwrap_or("—")
    ));

    out.push_str("<h2>Floor</h2><ul class=\"rows\">");
    for line in guard.map(|x| x.get("floor").as_array()).unwrap_or(&[]) {
        if let Some(s) = line.as_str() {
            out.push_str(&format!("<li><span>{}</span></li>", esc(s)));
        }
    }
    out.push_str("</ul>");

    out.push_str("<h2>Probes</h2>");
    let checks = g.map(|g| g.get("checks").as_array()).unwrap_or(&[]);
    let errors = g.map(|g| g.get("errors").as_array()).unwrap_or(&[]);
    let probes = if checks.is_empty() { errors } else { checks };
    out.push_str("<ul class=\"rows\">");
    for p in probes {
        let st = p.get("status").as_str().unwrap_or("");
        out.push_str(&row(
            p.get("service").as_str().unwrap_or(""),
            st,
            st != "healthy",
        ));
    }
    out.push_str("</ul>");
    if g.is_some() && checks.is_empty() && errors.is_empty() {
        out.push_str("<p class=\"note\">Waiting for the first beat.</p>");
    }

    out.push_str("<h2>Live secrets · 400</h2>");
    out.push_str(&files(
        guard.map(|x| x.get("files").as_array()).unwrap_or(&[]),
    ));
    out.push_str("<h2>Volume backup</h2><p class=\"lede\">The volume cannot hold a private mode. These copies are sealed. The key stays on the container disk, mode 400.</p>");
    out.push_str(&files(
        guard.map(|x| x.get("volume").as_array()).unwrap_or(&[]),
    ));

    out.push_str("<h2>Incidents</h2>");
    match v.incidents {
        Ok(list) => {
            let rows = list.as_array();
            if rows.is_empty() {
                out.push_str("<p class=\"lede\">None open.</p>");
            } else {
                out.push_str("<ul class=\"rows\">");
                for r in rows {
                    out.push_str(&format!(
                        "<li><span>{}</span><span class=\"subtle\">{}</span></li>",
                        esc(r.get("title").as_str().unwrap_or("")),
                        esc(r.get("severity").as_str().unwrap_or(""))
                    ));
                }
                out.push_str("</ul>");
            }
        }
        Err(m) => out.push_str(&missing("Incidents", m)),
    }
    out.push_str(&format!(
        "<p class=\"note\">Heals logged {}. Guardian does not kill the gateway.</p></div>",
        g.map(|g| num(g.get("healingActions")).unwrap_or_else(|| "0".into()))
            .unwrap_or(dash)
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    const GUARD: &str = r#"{"ok":true,"denylist":false,"autoAccept":true,"floorAlways":true,"historyRedaction":true,"auth":false,"floor":["gateway stop","/etc, docker.sock <x>"],"files":[{"path":"/root/galion-secrets/github.token","mode":"400","ok":true},{"path":"/root/galion-secrets/hermes.env","mode":"644","ok":false}],"volume":[{"path":"/workspace/hermes/.env","mode":"open","ok":false}]}"#;
    const GUARDIAN: &str = r#"{"status":"alert","health":"degraded","score":61,"activeIncidents":2,"healingActions":3,"library":{"count":7},"checks":[{"service":"console","status":"healthy"},{"service":"tuner","status":"down"}],"lastScan":"2026-10-21T03:12:45.123Z"}"#;
    const INC: &str = r#"[{"id":"1","title":"Loose <mode>","severity":"high","status":"open"}]"#;

    #[test]
    fn zagreb_clock_both_seasons() {
        assert_eq!(
            zagreb_clock("2026-10-21T03:12:45.123Z").as_deref(),
            Some("05:12:45 CEST")
        );
        // EU switch 2026-10-25 01:00 UTC.
        assert_eq!(
            zagreb_clock("2026-10-25T00:59:59+00:00").as_deref(),
            Some("02:59:59 CEST")
        );
        assert_eq!(
            zagreb_clock("2026-10-25T01:00:00+00:00").as_deref(),
            Some("02:00:00 CET")
        );
        assert_eq!(
            zagreb_clock("2026-03-29T00:59:59+00:00").as_deref(),
            Some("01:59:59 CET")
        );
        assert_eq!(
            zagreb_clock("2026-03-29T01:00:00+00:00").as_deref(),
            Some("03:00:00 CEST")
        );
        assert_eq!(
            zagreb_clock("2026-01-01T23:30:00+02:00").as_deref(),
            Some("22:30:00 CET")
        );
        assert_eq!(
            zagreb_clock("2026-12-31T23:30:00+00:00").as_deref(),
            Some("00:30:00 CET")
        );
        for bad in [
            "",
            "2026-10-21",
            "2026-13-01T00:00:00+00:00",
            "2026-10-21T03:12:45",
            "x026-10-21T03:12:45+00:00",
        ] {
            assert!(zagreb_clock(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn full_fort_renders_like_the_tsx() {
        let (a, b, c) = (
            parse(GUARD.as_bytes()).unwrap(),
            parse(GUARDIAN.as_bytes()).unwrap(),
            parse(INC.as_bytes()).unwrap(),
        );
        let h = body(&View {
            guard: Ok(&a),
            guardian: Ok(&b),
            incidents: Ok(&c),
            note: "",
        });
        assert!(h.contains("Watch · 24/7 · degraded · 05:12:45 CEST"));
        assert!(h.contains("<li><span>Score</span><span class=\"err\">61</span></li>"));
        assert!(h.contains("<li><span>Denylist</span><span class=\"muted\">off</span></li>"));
        assert!(h.contains("<li><span>Notes</span><span class=\"muted\">7</span></li>"));
        assert!(h.contains("Auto-accept is on"));
        assert!(h.contains("<li><span>tuner</span><span class=\"err\">down</span></li>"));
        assert!(h.contains(
            "<li><span>galion-secrets/github.token</span><span class=\"muted\">400</span></li>"
        ));
        assert!(h.contains(
            "<li><span>galion-secrets/hermes.env</span><span class=\"err\">644</span></li>"
        ));
        assert!(h.contains("<li><span>hermes/.env</span><span class=\"err\">open</span></li>"));
        assert!(h.contains("Loose &lt;mode&gt;") && h.contains("docker.sock &lt;x&gt;"));
        assert!(h.contains("Heals logged 3."));
        assert!(h.contains("type=\"password\" name=\"pin\""));
        assert!(
            !h.contains("/api/hermes/guard"),
            "no denylist write from this page"
        );
        assert!(!h.contains("did not load") && !h.contains("<script"));
    }

    #[test]
    fn unpaired_says_why_not_healthy() {
        let m = Miss::Status(401, "phone required".into());
        let h = body(&View {
            guard: Err(&m),
            guardian: Err(&m),
            incidents: Err(&m),
            note: "",
        });
        assert!(h.contains("Security did not load: HTTP 401 (phone required)."));
        assert!(h.contains("Guardian status did not load: HTTP 401 (phone required)."));
        assert!(h.contains("Incidents did not load"));
        assert!(h.contains("<li><span>Denylist</span><span class=\"muted\">—</span></li>"));
        assert!(h.contains("<li><span>Score</span><span class=\"muted\">—</span></li>"));
        assert!(!h.contains("None open.") && !h.contains("Waiting for the first beat"));
        assert!(h.contains("Watch · 24/7 · no beat yet</p>"));
    }

    #[test]
    fn pin_rules() {
        assert!(pin_ok("4242") && pin_ok("a!B#"));
        for bad in ["", " ", "12 34", "12\r\nX: y", "ü", &"9".repeat(65)] {
            assert!(!pin_ok(bad), "{bad:?}");
        }
    }
}
