// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! Std-only Europe/Zagreb timestamps (RFC 3339 with an explicit offset,
//! never `Z`), so this crate keeps zero dependencies. EU rule: CEST (+02:00)
//! from the last Sunday of March 01:00 UTC to the last Sunday of October
//! 01:00 UTC, CET (+01:00) otherwise. The full tz database lives in the
//! `zagreb-time` crate; this is the same rule for the years that matter.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date for days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

/// Unix second of the last Sunday of `month` (31-day months only) at 01:00 UTC.
fn last_sunday_0100_utc(y: i64, month: i64) -> i64 {
    let last = days_from_civil(y, month, 31);
    let wd = (last + 4).rem_euclid(7); // 0 = Sunday (1970-01-01 was a Thursday)
    (last - wd) * 86_400 + 3_600
}

/// Zagreb UTC offset in seconds at a unix second.
pub fn offset(secs: i64) -> i64 {
    let (y, _, _) = civil_from_days(secs.div_euclid(86_400));
    if secs >= last_sunday_0100_utc(y, 3) && secs < last_sunday_0100_utc(y, 10) {
        7_200
    } else {
        3_600
    }
}

/// A unix second as Zagreb RFC 3339, e.g. `2026-10-20T21:05:00+02:00`.
pub fn from_unix(secs: i64) -> String {
    let off = offset(secs);
    let l = secs + off;
    let (y, m, d) = civil_from_days(l.div_euclid(86_400));
    let s = l.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+{:02}:00",
        s / 3600,
        s % 3600 / 60,
        s % 60,
        off / 3600
    )
}

/// Now as Zagreb RFC 3339.
pub fn now_rfc3339() -> String {
    from_unix(now_secs())
}

/// Seconds since the epoch (0 if the clock is before 1970).
pub fn now_secs() -> i64 {
    now_parts().0
}

/// (seconds, subsecond nanoseconds) since the epoch; the one clock read.
pub fn now_parts() -> (i64, u32) {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() as i64, d.subsec_nanos()))
        .unwrap_or((0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dst_edges_2026() {
        // 2026-03-29 01:00 UTC: CET -> CEST; 2026-10-25 01:00 UTC: CEST -> CET
        let spring = days_from_civil(2026, 3, 29) * 86_400 + 3_600;
        let autumn = days_from_civil(2026, 10, 25) * 86_400 + 3_600;
        assert_eq!(from_unix(spring - 1), "2026-03-29T01:59:59+01:00");
        assert_eq!(from_unix(spring), "2026-03-29T03:00:00+02:00");
        assert_eq!(from_unix(autumn - 1), "2026-10-25T02:59:59+02:00");
        assert_eq!(from_unix(autumn), "2026-10-25T02:00:00+01:00");
    }

    #[test]
    fn known_instants() {
        assert_eq!(from_unix(0), "1970-01-01T01:00:00+01:00");
        // 2026-10-20 18:55:00 UTC
        let t = days_from_civil(2026, 10, 20) * 86_400 + 18 * 3600 + 55 * 60;
        assert_eq!(from_unix(t), "2026-10-20T20:55:00+02:00");
        assert_eq!(civil_from_days(days_from_civil(2028, 2, 29)), (2028, 2, 29));
        assert!(!now_rfc3339().ends_with('Z'));
    }
}
