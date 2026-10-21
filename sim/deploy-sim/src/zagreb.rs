// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Europe/Zagreb wall-clock stamps with an explicit offset (never `Z`), std only.
//! EU rule: CEST (+02:00) from the last Sunday of March 01:00 UTC to the last
//! Sunday of October 01:00 UTC, CET (+01:00) otherwise.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// (year, month, day) to days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of 01:00 UTC on the last Sunday of `month` (31-day months only).
fn last_sunday_0100(y: i64, month: u32) -> i64 {
    let d31 = days_from_civil(y, month, 31);
    let wd = (d31 + 4).rem_euclid(7); // 0 = Sunday (1970-01-01 was a Thursday)
    (d31 - wd) * 86_400 + 3_600
}

/// Offset of Europe/Zagreb at `unix` seconds, in seconds east of UTC.
pub fn offset(unix: i64) -> i64 {
    let (y, _, _) = civil(unix.div_euclid(86_400));
    if unix >= last_sunday_0100(y, 3) && unix < last_sunday_0100(y, 10) {
        7_200
    } else {
        3_600
    }
}

/// `YYYY-MM-DDTHH:MM:SS.mmm+HH:00` for a unix time in milliseconds.
pub fn stamp_ms(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let off = offset(secs);
    let local = secs + off;
    let (y, m, d) = civil(local.div_euclid(86_400));
    let sod = local.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}+{:02}:00",
        sod / 3600,
        sod / 60 % 60,
        sod % 60,
        unix_ms.rem_euclid(1000),
        off / 3600
    )
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn now() -> String {
    stamp_ms(now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dst_edges_2026() {
        // 2026-10-25 01:00 UTC = 03:00 CEST -> 02:00 CET.
        let switch = days_from_civil(2026, 10, 25) * 86_400 + 3_600;
        assert_eq!(offset(switch - 1), 7_200);
        assert_eq!(offset(switch), 3_600);
        // 2026-03-29 01:00 UTC = 02:00 CET -> 03:00 CEST.
        let spring = days_from_civil(2026, 3, 29) * 86_400 + 3_600;
        assert_eq!(offset(spring - 1), 3_600);
        assert_eq!(offset(spring), 7_200);
    }

    #[test]
    fn known_stamp() {
        // 2026-10-20T18:41:44.975Z
        let ms = (days_from_civil(2026, 10, 20) * 86_400 + 18 * 3600 + 41 * 60 + 44) * 1000 + 975;
        assert_eq!(stamp_ms(ms), "2026-10-20T20:41:44.975+02:00");
        let winter = days_from_civil(2026, 12, 31) * 86_400 * 1000 + 23 * 3_600_000;
        assert_eq!(stamp_ms(winter), "2027-01-01T00:00:00.000+01:00");
        assert!(!stamp_ms(now_ms()).contains('Z'));
    }

    #[test]
    fn civil_roundtrip() {
        for days in [-1_000_000i64, -1, 0, 1, 19_000, 20_746, 2_000_000] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }
}
