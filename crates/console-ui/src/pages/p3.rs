// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Small helpers shared by the console-port-3 rooms (`/instances`, `/keys`,
//! `/markers`). Kept in their own file so the five console-port branches
//! touch as few shared lines as possible.

use crate::html::esc;
use crate::json::Value;

/// A string field, or `""`.
pub fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).as_str().unwrap_or("")
}

/// At most `max` characters of `t` (char boundary safe), with `…` when cut.
pub fn cap(t: &str, max: usize) -> String {
    match t.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

/// Visible text only: control characters (CR, LF, ESC, …) become spaces.
pub fn plain(t: &str) -> String {
    t.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// "Did not load" line with the reason (HTTP status and the API's `error`).
pub fn miss(what: &str, status: Option<u16>, err: &str) -> String {
    let why = match status {
        Some(code) if err.is_empty() => format!("HTTP {code}"),
        Some(code) => format!("HTTP {code} ({})", cap(&plain(err), 120)),
        None => "the console API did not answer".to_string(),
    };
    format!(
        "<p class=\"note p3-bad\">{} did not load: {}.</p>",
        esc(what),
        esc(&why)
    )
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`, public-domain algorithm, written out here).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `(year, month, day)` for days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Unix seconds of the last Sunday of `month` in `year`, 01:00 UTC (the EU
/// summer-time switch).
fn eu_switch(year: i64, month: i64) -> i64 {
    let last = days_from_civil(year, month + 1, 1) - 1;
    // 1970-01-01 was a Thursday: weekday 0 = Sunday -> (days + 4) % 7.
    let wd = (last + 4).rem_euclid(7);
    (last - wd) * 86_400 + 3_600
}

/// Europe/Zagreb offset in hours for a Unix time: CEST +2 between the EU
/// switches, CET +1 otherwise.
pub fn zagreb_offset(unix: i64) -> i64 {
    let (y, _, _) = civil_from_days(unix.div_euclid(86_400));
    if unix >= eu_switch(y, 3) && unix < eu_switch(y, 10) {
        2
    } else {
        1
    }
}

/// `HH:MM:SS CEST` (or `CET`) on the Zagreb clock for Unix milliseconds.
/// The TSX printed UTC (`toISOString().slice(11, 19)`) without a zone.
pub fn zagreb_hms(ms: f64) -> String {
    if !ms.is_finite() {
        return "—".into();
    }
    let unix = (ms / 1000.0).floor() as i64;
    let off = zagreb_offset(unix);
    let local = unix + off * 3_600;
    let sod = local.rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02} {}",
        sod / 3_600,
        sod % 3_600 / 60,
        sod % 60,
        if off == 2 { "CEST" } else { "CET" }
    )
}

/// `YYYY-MM-DD HH:MM` on the Zagreb clock for an ISO-8601 UTC stamp such as
/// `2026-10-21T03:12:09.123Z` (what `new Date().toISOString()` writes); (repo-check: allow, input format)
/// anything else is shown as given.
pub fn zagreb_iso(iso: &str) -> String {
    let b = iso.as_bytes();
    let num = |a: usize, z: usize| -> Option<i64> {
        iso.get(a..z)
            .filter(|t| t.bytes().all(|c| c.is_ascii_digit()))?
            .parse()
            .ok()
    };
    let ok = b.len() >= 20 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && iso.ends_with('Z');
    let parts = (
        num(0, 4),
        num(5, 7),
        num(8, 10),
        num(11, 13),
        num(14, 16),
        num(17, 19),
    );
    match (ok, parts) {
        (true, (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(se))) => {
            let unix = days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + se;
            let off = zagreb_offset(unix);
            let local = unix + off * 3_600;
            let (ly, lm, ld) = civil_from_days(local.div_euclid(86_400));
            let sod = local.rem_euclid(86_400);
            format!(
                "{ly:04}-{lm:02}-{ld:02} {:02}:{:02} {}",
                sod / 3_600,
                sod % 3_600 / 60,
                if off == 2 { "CEST" } else { "CET" }
            )
        }
        _ => cap(&plain(iso), 40),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for z in [-1_000_000i64, -1, 0, 1, 20_747, 2_932_896] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2026, 10, 21), 20_747);
    }

    #[test]
    fn zagreb_dst_edges_2026() {
        // Summer time 2026-03-29 01:00 UTC .. 2026-10-25 01:00 UTC.
        let start = days_from_civil(2026, 3, 29) * 86_400 + 3_600;
        let end = days_from_civil(2026, 10, 25) * 86_400 + 3_600;
        assert_eq!(zagreb_offset(start - 1), 1);
        assert_eq!(zagreb_offset(start), 2);
        assert_eq!(zagreb_offset(end - 1), 2);
        assert_eq!(zagreb_offset(end), 1);
        // 2027: 2027-03-28 and 2027-10-31.
        assert_eq!(
            eu_switch(2027, 3),
            days_from_civil(2027, 3, 28) * 86_400 + 3_600
        );
        assert_eq!(
            eu_switch(2027, 10),
            days_from_civil(2027, 10, 31) * 86_400 + 3_600
        );
    }

    #[test]
    fn hms_and_iso() {
        // 2026-10-21 03:38:05 UTC = 05:38:05 CEST.
        let ms = (days_from_civil(2026, 10, 21) * 86_400 + 3 * 3_600 + 38 * 60 + 5) as f64 * 1000.0;
        assert_eq!(zagreb_hms(ms + 999.0), "05:38:05 CEST");
        // 2026-12-01 23:30:00 UTC = 00:30:00 CET next day.
        let ms = (days_from_civil(2026, 12, 1) * 86_400 + 23 * 3_600 + 30 * 60) as f64 * 1000.0;
        assert_eq!(zagreb_hms(ms), "00:30:00 CET");
        assert_eq!(zagreb_hms(f64::NAN), "—");
        assert_eq!(
            zagreb_iso("2026-10-21T03:12:09.123Z"), // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
            "2026-10-21 05:12 CEST"
        );
        assert_eq!(zagreb_iso("2026-12-31T23:30:00Z"), "2027-01-01 00:30 CET"); // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
        assert_eq!(zagreb_iso("yesterday\nnight"), "yesterday night");
    }

    #[test]
    fn caps_and_plain() {
        assert_eq!(cap("abcdef", 3), "abc…");
        assert_eq!(cap("čćž", 3), "čćž");
        assert_eq!(cap("čćžđ", 2), "čć…");
        assert_eq!(plain("a\r\nb\u{1b}[0m"), "a  b [0m");
        assert!(miss("Keys", Some(401), "phone required").contains("HTTP 401 (phone required)"));
        assert!(miss("Keys", None, "").contains("did not answer"));
        assert!(miss("K", Some(500), "<b>").contains("&lt;b&gt;"));
    }
}
