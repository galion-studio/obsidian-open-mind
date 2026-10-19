// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Europe/Zagreb timestamps without a tz database.
//!
//! EU rule (in force since 1996): CEST (+02:00) from the last Sunday of March
//! 01:00 UTC until the last Sunday of October 01:00 UTC, CET (+01:00) otherwise.
//! Output is RFC 3339 with an explicit offset, never `Z`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

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

/// Unix second of the last Sunday of `month` (31-day month) at 01:00 UTC.
fn last_sunday_0100_utc(year: i64, month: i64) -> i64 {
    let d31 = days_from_civil(year, month, 31);
    // 1970-01-01 was a Thursday; (days + 4) % 7 == 0 means Sunday.
    let back = (d31 + 4).rem_euclid(7);
    (d31 - back) * 86_400 + 3_600
}

/// Offset from UTC in seconds for a unix instant in Europe/Zagreb.
pub fn offset_secs(unix: i64) -> i64 {
    let (y, _, _) = civil_from_days(unix.div_euclid(86_400));
    let start = last_sunday_0100_utc(y, 3);
    let end = last_sunday_0100_utc(y, 10);
    if unix >= start && unix < end {
        7_200
    } else {
        3_600
    }
}

/// RFC 3339 in Europe/Zagreb, e.g. `2026-10-18T20:15:00+02:00`.
pub fn rfc3339(unix: i64) -> String {
    let off = offset_secs(unix);
    let local = unix + off;
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

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_rfc3339() -> String {
    rfc3339(now_unix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summer_and_winter() {
        // 2026-10-18T18:00:00Z
        assert_eq!(rfc3339(1_792_346_400), "2026-10-18T20:00:00+02:00");
        // 2026-12-24T12:00:00Z
        assert_eq!(rfc3339(1_798_113_600), "2026-12-24T13:00:00+01:00");
    }

    #[test]
    fn october_switch_2026() {
        // Last Sunday of October 2026 is the 25th; switch at 01:00 UTC.
        let switch = days_from_civil(2026, 10, 25) * 86_400 + 3_600;
        assert_eq!(rfc3339(switch - 1), "2026-10-25T02:59:59+02:00");
        assert_eq!(rfc3339(switch), "2026-10-25T02:00:00+01:00");
    }

    #[test]
    fn march_switch_2027() {
        // Last Sunday of March 2027 is the 28th.
        let switch = days_from_civil(2027, 3, 28) * 86_400 + 3_600;
        assert_eq!(rfc3339(switch - 1), "2027-03-28T01:59:59+01:00");
        assert_eq!(rfc3339(switch), "2027-03-28T03:00:00+02:00");
    }

    #[test]
    fn never_z() {
        assert!(!now_rfc3339().ends_with('Z'));
    }
}
