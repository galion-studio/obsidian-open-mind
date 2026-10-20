// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Europe/Zagreb wall clock without a time-zone database or a dependency.
//
// What was wrong: `clock()` printed `unix % 86400` as `HH:MM:SS`, which is the
// UTC time of day. The process TZ (even `TZ=Europe/Zagreb`) never entered into
// it, so the analytics `at` field and the `at` in one.jsonl / session.jsonl
// were two hours off in summer and carried no offset at all.
//
// Rule used here: Croatia keeps the EU summer-time rule (Directive 2000/84/EC,
// in force since 1996): CEST (+02:00) from the last Sunday of March 01:00 UTC
// to the last Sunday of October 01:00 UTC, CET (+01:00) otherwise. The rule is
// computed from the unix second, so the host TZ and /usr/share/zoneinfo do not
// matter. It is cross-checked against the workspace's chrono-tz based
// `zagreb-time` crate (dev-dependency only) in the tests below.

/// Offset from UTC in seconds at the given unix second: 7200 (CEST) or 3600 (CET).
pub fn offset_secs(unix: i64) -> i64 {
    let (year, _, _) = civil_from_days(unix.div_euclid(86_400));
    let start = last_sunday_utc_1am(year, 3);
    let end = last_sunday_utc_1am(year, 10);
    if unix >= start && unix < end {
        7200
    } else {
        3600
    }
}

/// RFC 3339, seconds precision, explicit offset, never `Z`:
/// `2026-10-20T20:40:00+02:00`.
pub fn rfc3339(unix: i64) -> String {
    let off = offset_secs(unix);
    let local = unix + off;
    let (y, m, d) = civil_from_days(local.div_euclid(86_400));
    let s = local.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+{:02}:00",
        s / 3600,
        (s % 3600) / 60,
        s % 60,
        off / 3600
    )
}

/// Short wall-clock label with the offset, for nerve event lines:
/// `20:40:00+02:00`.
pub fn clock(unix: i64) -> String {
    let off = offset_secs(unix);
    let s = (unix + off).rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}+{:02}:00",
        s / 3600,
        (s % 3600) / 60,
        s % 60,
        off / 3600
    )
}

/// Unix second of 01:00 UTC on the last Sunday of `month` in `year`.
fn last_sunday_utc_1am(year: i64, month: u32) -> i64 {
    // Last day of March and October is the 31st.
    let last = days_from_civil(year, month, 31);
    // 1970-01-01 was a Thursday: weekday 0 = Sunday gives (days + 4) % 7.
    let weekday = (last + 4).rem_euclid(7);
    (last - weekday) * 86_400 + 3600
}

// Howard Hinnant's public-domain civil-date algorithms
// (http://howardhinnant.github.io/date_algorithms.html), rewritten here.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        // unix 1792521240 is 2026-10-20 18:34:00 +00:00, i.e. 20:34 CEST.
        assert_eq!(rfc3339(1_792_521_240), "2026-10-20T20:34:00+02:00");
        assert_eq!(clock(1_792_521_240), "20:34:00+02:00");
        // 2026 switch-back: 2026-10-25 03:00 CEST becomes 02:00 CET.
        assert_eq!(rfc3339(1_792_890_000 - 1), "2026-10-25T02:59:59+02:00");
        assert_eq!(rfc3339(1_792_890_000), "2026-10-25T02:00:00+01:00");
        // 2027 spring forward: 2027-03-28 02:00 CET becomes 03:00 CEST.
        assert_eq!(rfc3339(1_806_195_600 - 1), "2027-03-28T01:59:59+01:00");
        assert_eq!(rfc3339(1_806_195_600), "2027-03-28T03:00:00+02:00");
        // Winter, new year.
        assert_eq!(rfc3339(1_798_758_000), "2027-01-01T00:00:00+01:00");
        assert_eq!(rfc3339(0), "1970-01-01T01:00:00+01:00");
        assert!(!rfc3339(1_792_521_240).ends_with('Z'));
    }

    #[test]
    fn matches_chrono_tz_hourly_2020_to_2040() {
        // Every hour from 2020-01-01 to 2040-01-01, plus the seconds either
        // side, compared with the tz database via the workspace crate.
        let mut t: i64 = 1_577_836_800;
        while t < 2_208_988_800 {
            for s in [t - 1, t, t + 1] {
                assert_eq!(rfc3339(s), zagreb_time::from_unix(s), "unix {s}");
            }
            t += 3600;
        }
    }
}
