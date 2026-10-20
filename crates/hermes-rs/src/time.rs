// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Clock helpers. Logs and reports use Europe/Zagreb with an explicit offset
//! (EU summer-time rule, no tz database needed); wire timestamps stay unix
//! seconds like upstream.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_f64() -> f64 {
    SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn now_secs() -> i64 {
    now_f64() as i64
}

/// Days since 1970-01-01 -> (year, month, day). Howard Hinnant's algorithm.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of the last Sunday of `month` at 01:00 UTC.
fn last_sunday_0100_utc(year: i64, month: u32) -> i64 {
    let last_day = if month == 3 || month == 10 { 31 } else { 30 };
    let days = days_from_civil(year, month, last_day);
    // 1970-01-01 was a Thursday (weekday 4 with Sunday = 0)
    let wd = (days + 4).rem_euclid(7);
    (days - wd) * 86_400 + 3_600
}

/// Europe/Zagreb offset in seconds at a unix instant (CET +1, CEST +2).
pub fn zagreb_offset(secs: i64) -> i64 {
    let (y, _, _) = civil_from_days(secs.div_euclid(86_400));
    let start = last_sunday_0100_utc(y, 3);
    let end = last_sunday_0100_utc(y, 10);
    if secs >= start && secs < end {
        7_200
    } else {
        3_600
    }
}

/// RFC 3339 with an explicit Europe/Zagreb offset, e.g. 2026-10-18T21:05:00+02:00.
pub fn zagreb_rfc3339(secs: i64) -> String {
    let off = zagreb_offset(secs);
    let local = secs + off;
    let (y, m, d) = civil_from_days(local.div_euclid(86_400));
    let s = local.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+{:02}:00",
        s / 3600,
        (s / 60) % 60,
        s % 60,
        off / 3600
    )
}

/// RFC 3339 in UTC with "+00:00" (the Python jobs.json style).
pub fn utc_rfc3339(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        s / 3600,
        (s / 60) % 60,
        s % 60
    )
}

/// Parse "YYYY-MM-DDTHH:MM:SS[.frac](Z|+HH:MM|-HH:MM)" to unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (
        num(0, 4)?,
        num(5, 7)?,
        num(8, 10)?,
        num(11, 13)?,
        num(14, 16)?,
        num(17, 19)?,
    );
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        let n = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
        rest = &r[n..];
    }
    let off = if rest.is_empty() || rest == "Z" || rest == "z" {
        0
    } else {
        let sign = if rest.starts_with('-') { -1 } else { 1 };
        let hh = rest.get(1..3)?.parse::<i64>().ok()?;
        let mm = rest
            .get(4..6)
            .and_then(|x| x.parse::<i64>().ok())
            .unwrap_or(0);
        sign * (hh * 3600 + mm * 60)
    };
    let days = days_from_civil(y, mo as u32, d as u32);
    Some(days * 86_400 + h * 3600 + mi * 60 + se - off)
}

/// "YYYY-MM-DD" in Europe/Zagreb, used as the spend-day key.
pub fn zagreb_day(secs: i64) -> String {
    zagreb_rfc3339(secs)[..10].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zagreb_summer_and_winter() {
        // 2026-10-18T21:00:00+02:00 is CEST
        let t = parse_rfc3339("2026-10-18T21:00:00+02:00").unwrap();
        assert_eq!(t, 1_792_350_000);
        // upstream wire stamps may still carry Z; the parser must accept them
        assert_eq!(parse_rfc3339("2026-10-18T19:00:00Z"), Some(t)); // repo-check: allow (Z input accepted on the wire)
        assert_eq!(zagreb_rfc3339(t), "2026-10-18T21:00:00+02:00");
        // 2026-10-25 is the last Sunday of October; from 03:00+02:00 (= 02:00+01:00) it is CET
        let t = parse_rfc3339("2026-10-25T02:00:00+01:00").unwrap();
        assert_eq!(t, 1_792_890_000);
        assert_eq!(zagreb_rfc3339(t), "2026-10-25T02:00:00+01:00");
        let t = parse_rfc3339("2026-03-29T01:59:59+01:00").unwrap();
        assert_eq!(zagreb_offset(t), 3600);
        assert_eq!(zagreb_offset(t + 1), 7200);
    }

    #[test]
    fn parse_python_style() {
        let a = parse_rfc3339("2026-10-19T08:10:59.865722+00:00").unwrap();
        let b = parse_rfc3339("2026-10-19T10:10:59+02:00").unwrap();
        assert_eq!(a, b);
        assert_eq!(utc_rfc3339(a), "2026-10-19T08:10:59+00:00");
    }
}
