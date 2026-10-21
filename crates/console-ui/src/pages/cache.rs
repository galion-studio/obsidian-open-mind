// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/cache`: port of console-ide `src/routes/_console/cache.tsx`.
//!
//! Data: the desk snapshot `GET 127.0.0.1:8646/desk` (galion-desk), the same
//! loopback service the TSX server function `pull()` of
//! `src/lib/desk-foundation.ts` fetched; the front is told where it lives
//! with `--desk`. Only `.mindpack` is used (cache, cached, stores, models,
//! recent).
//!
//! Differences from the TSX, on purpose:
//! - no 4 s polling (no JS); each load is a fresh read;
//! - the "On disk" bars are `<meter>` elements instead of inline `width`
//!   styles (the CSP forbids inline style);
//! - file times print in Europe/Zagreb (the TSX used the browser's zone,
//!   which for the owner is Zagreb) and the heading says so.

use super::jsfmt::{finite, num, round, text, to_fixed};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// `num(value)`: a finite number or 0.
fn n0(v: &Value) -> f64 {
    finite(v).unwrap_or(0.0)
}

/// `compact(n)`.
pub fn compact(n: f64) -> String {
    if n == 0.0 || n.is_nan() {
        "0".into()
    } else if n >= 1_000_000.0 {
        format!("{}M", to_fixed(n / 1_000_000.0, 1))
    } else if n >= 1_000.0 {
        format!("{}k", to_fixed(n / 1_000.0, 1))
    } else {
        num(n)
    }
}

/// `bytes(n)`.
pub fn bytes(n: f64) -> String {
    if n == 0.0 || n.is_nan() {
        "0".into()
    } else if n >= 1_000_000.0 {
        format!("{} MB", to_fixed(n / 1_000_000.0, 1))
    } else {
        format!("{} KB", num(round(n / 1024.0).max(1.0)))
    }
}

fn civil(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days_from_civil inverse (public domain algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of the last Sunday of `month` at 01:00 UTC (EU switch).
fn eu_switch(year: i64, month: u32) -> i64 {
    let last = days_from_civil(year, month, 31);
    // 1970-01-01 was a Thursday (weekday 4, Sunday = 0).
    let wd = (last + 4).rem_euclid(7);
    (last - wd) * 86_400 + 3_600
}

/// Europe/Zagreb offset in seconds at Unix time `t`.
pub fn zagreb_offset(t: i64) -> i64 {
    let (y, _, _) = civil(t.div_euclid(86_400));
    if t >= eu_switch(y, 3) && t < eu_switch(y, 10) {
        7_200
    } else {
        3_600
    }
}

/// `stamp(n)`: `MM-DD HH:MM` in Zagreb time, "" for 0.
pub fn stamp(secs: f64) -> String {
    if secs == 0.0 || !secs.is_finite() {
        return String::new();
    }
    let t = secs.floor() as i64;
    let local = t + zagreb_offset(t);
    let (_, m, d) = civil(local.div_euclid(86_400));
    let sod = local.rem_euclid(86_400);
    format!("{m:02}-{d:02} {:02}:{:02}", sod / 3600, (sod % 3600) / 60)
}

fn stop(title: &str, meta: &str, body: &str) -> String {
    let t = text(&Value::Str(title.into()), "");
    format!(
        "<li><p class=\"ell\" title=\"{0}\">{0}</p><p class=\"ell muted tiny\">{1}</p><p class=\"ell muted tiny\">{2}</p></li>",
        esc(&t),
        esc(&text(&Value::Str(meta.into()), "")),
        esc(&text(&Value::Str(body.into()), ""))
    )
}

/// Page body. `desk` is the `/desk` JSON, `None` when galion-desk did not
/// answer (the TSX then showed its empty mindpack: zeros and empty lists).
pub fn body(desk: Option<&Value>) -> String {
    let null = Value::Null;
    let mind = desk.map(|d| d.get("mindpack")).unwrap_or(&null);
    let c = mind.get("cache");
    let stores = mind.get("stores").as_array();
    let max_store = stores
        .iter()
        .map(|r| n0(r.get("bytes")))
        .fold(1.0f64, f64::max);
    let mut out = String::from("<div class=\"page w46\">");
    out.push_str(&format!(
        "<header><p class=\"k\"><a href=\"/console\">{}Desk</a></p><p class=\"lede\">{} read · {} write · {} files · {}</p></header>",
        icon("desk", "i i-sm"),
        compact(n0(c.get("read"))),
        compact(n0(c.get("write"))),
        esc(&text(c.get("files"), "0")),
        bytes(n0(c.get("file_bytes")))
    ));
    if desk.is_none() {
        out.push_str("<p class=\"note\">The desk (galion-desk /desk) did not answer; showing zeros like the TypeScript page.</p>");
    }
    out.push_str("<section aria-label=\"Roadmap\"><h2>Road</h2><ol class=\"road\">");
    out.push_str(&stop(
        "Token cache",
        &format!(
            "{} read · {} write",
            compact(n0(c.get("read"))),
            compact(n0(c.get("write")))
        ),
        &format!(
            "{} in · {} out · {} reasoning",
            compact(n0(c.get("input"))),
            compact(n0(c.get("output"))),
            compact(n0(c.get("reasoning")))
        ),
    ));
    for r in mind.get("cached").as_array() {
        out.push_str(&stop(
            &text(r.get("id"), "session"),
            &format!("{} turns in the context cache", text(r.get("turns"), "0")),
            &text(r.get("model"), "—"),
        ));
    }
    for r in stores {
        out.push_str(&stop(
            &text(r.get("name"), ""),
            &format!(
                "{} files · {}",
                text(r.get("n"), "0"),
                bytes(n0(r.get("bytes")))
            ),
            "Kept on this machine",
        ));
    }
    out.push_str("</ol></section>");
    out.push_str("<section aria-label=\"By model\"><h2>Read by model</h2><ul class=\"rows\">");
    for r in mind.get("models").as_array() {
        out.push_str(&format!(
            "<li class=\"stack\"><p class=\"ell\">{}</p><p class=\"muted tiny\">{} read · {} calls · {} in · {} out</p></li>",
            esc(&text(r.get("name"), "")),
            compact(n0(r.get("read"))),
            esc(&text(r.get("calls"), "0")),
            compact(n0(r.get("input"))),
            compact(n0(r.get("output")))
        ));
    }
    out.push_str("</ul></section>");
    out.push_str("<section aria-label=\"Files\"><h2>On disk</h2><ul class=\"rows\">");
    for r in stores {
        let pct = round(n0(r.get("bytes")) / max_store * 100.0).max(4.0);
        out.push_str(&format!(
            "<li class=\"disk\"><span class=\"amt mono\">{}</span><meter min=\"0\" max=\"100\" value=\"{}\"></meter><span class=\"ell\">{} · {}</span></li>",
            bytes(n0(r.get("bytes"))),
            num(pct),
            esc(&text(r.get("name"), "")),
            esc(&text(r.get("n"), "0"))
        ));
    }
    out.push_str("</ul><ul class=\"rows\">");
    for r in mind.get("recent").as_array() {
        out.push_str(&format!(
            "<li class=\"tiny muted\"><span class=\"ell\"><span class=\"fg\">{}</span> {} · {}</span></li>",
            stamp(n0(r.get("at"))),
            esc(&text(r.get("name"), "")),
            bytes(n0(r.get("bytes")))
        ));
    }
    out.push_str("</ul><p class=\"note\">Times: Europe/Zagreb.</p></section>");
    out.push_str(&format!(
        "<p class=\"plain\"><a class=\"btn ghost\" href=\"/context\">{}Open context</a></p>",
        icon("context", "i i-sm")
    ));
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn number_helpers_match_the_tsx() {
        assert_eq!(compact(0.0), "0");
        assert_eq!(compact(999.0), "999");
        assert_eq!(compact(1193851.0), "1.2M");
        assert_eq!(compact(109107266.0), "109.1M");
        assert_eq!(compact(1830.0), "1.8k");
        assert_eq!(compact(1050.0), "1.1k");
        assert_eq!(bytes(0.0), "0");
        assert_eq!(bytes(16731597.0), "16.7 MB");
        assert_eq!(bytes(100.0), "1 KB");
        assert_eq!(bytes(1536.0), "2 KB");
        assert_eq!(bytes(999_999.0), "977 KB");
    }

    #[test]
    fn zagreb_time() {
        // 2026-10-21 02:50:00 UTC = 04:50 CEST
        assert_eq!(stamp(1_792_551_000.0), "10-21 04:50");
        // 2026-10-25 01:00 UTC switch: 00:59 UTC = 02:59 CEST, 01:00 UTC = 02:00 CET
        assert_eq!(stamp(1_792_890_000.0 - 60.0), "10-25 02:59");
        assert_eq!(stamp(1_792_890_000.0), "10-25 02:00");
        // 2026-03-29 01:00 UTC: 03:00 CEST
        assert_eq!(stamp(1_774_746_000.0), "03-29 03:00");
        assert_eq!(stamp(1_774_746_000.0 - 1.0), "03-29 01:59");
        assert_eq!(stamp(0.0), "");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil(20_747), (2026, 10, 21));
    }

    #[test]
    fn page_from_a_desk_snapshot() {
        let v = parse(br#"{"mindpack":{"cache":{"read":109107266,"write":1193851,"input":19391995,"output":1830315,"reasoning":789359,"files":1142,"file_bytes":16731597},
          "cached":[{"id":"galion-one","model":"qwen <9b>","turns":8},{"id":"","model":"","turns":null}],
          "stores":[{"name":"scratch","n":1019,"bytes":12629743},{"name":"tiny","n":1,"bytes":10}],
          "models":[{"name":"m1","calls":3,"input":1500,"output":20,"read":2000000,"reasoning":0}],
          "recent":[{"name":"a.txt","bytes":2048,"at":1792551000}]}}"#).unwrap();
        let b = body(Some(&v));
        assert!(b.contains("109.1M read · 1.2M write · 1142 files · 16.7 MB"));
        assert!(b.contains("19.4M in · 1.8M out · 789.4k reasoning"));
        assert!(b.contains("title=\"galion-one\">galion-one</p><p class=\"ell muted tiny\">8 turns in the context cache</p><p class=\"ell muted tiny\">qwen &lt;9b&gt;</p>"));
        assert!(b.contains(">session</p><p class=\"ell muted tiny\">0 turns in the context cache</p><p class=\"ell muted tiny\">—</p>"));
        assert!(b.contains(">scratch</p><p class=\"ell muted tiny\">1019 files · 12.6 MB</p><p class=\"ell muted tiny\">Kept on this machine"));
        assert!(b.contains("2.0M read · 3 calls · 1.5k in · 20 out"));
        assert!(b.contains(
            "<meter min=\"0\" max=\"100\" value=\"100\"></meter><span class=\"ell\">scratch · 1019"
        ));
        assert!(b.contains("value=\"4\"></meter><span class=\"ell\">tiny · 1"));
        assert!(b.contains("10-21 04:50</span> a.txt · 2 KB"));
        assert!(b.contains("href=\"/context\""));
        assert!(!b.contains("style=") && !b.contains("<script"));
    }

    #[test]
    fn desk_down_shows_zeros() {
        let b = body(None);
        assert!(b.contains("0 read · 0 write · 0 files · 0"));
        assert!(b.contains("0 in · 0 out · 0 reasoning"));
        assert!(b.contains("did not answer"));
    }
}
