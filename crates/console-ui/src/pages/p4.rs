// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Small helpers for the console-port-4 rooms (`/md/$name`, `/memory`,
//! `/mesh`): the Zagreb wall clock (EU summer-time rule, zone always named)
//! and JavaScript's `String.prototype.slice(0, n)` on UTF-16 units.

/// Days since 1970-01-01 to a civil (proleptic Gregorian) date.
/// Howard Hinnant's `civil_from_days` (public domain algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
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

/// Civil date to days since 1970-01-01 (Hinnant's `days_from_civil`).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds of the last Sunday of `month` at 01:00 UTC (the EU switch).
fn last_sunday_0100_utc(year: i64, month: u32) -> i64 {
    let last = if month == 3 || month == 10 { 31 } else { 30 };
    let days = days_from_civil(year, month, last);
    // 1970-01-01 was a Thursday (weekday 4, Sunday = 0).
    let wd = (days + 4).rem_euclid(7);
    (days - wd) * 86_400 + 3_600
}

/// Europe/Zagreb offset in seconds for a Unix time (CET +1, CEST +2).
pub fn zagreb_offset(unix: i64) -> i64 {
    let (y, _, _) = civil_from_days(unix.div_euclid(86_400));
    if unix >= last_sunday_0100_utc(y, 3) && unix < last_sunday_0100_utc(y, 10) {
        7_200
    } else {
        3_600
    }
}

/// `2026-10-21 05:40 CEST (+02:00)`.
pub fn zagreb(unix: i64) -> String {
    let off = zagreb_offset(unix);
    let t = unix + off;
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    let (zone, sign) = if off == 7_200 {
        ("CEST", "+02:00")
    } else {
        ("CET", "+01:00")
    };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} {zone} ({sign})",
        s / 3600,
        (s % 3600) / 60
    )
}

/// `s.slice(0, n)` as JavaScript counts it (UTF-16 code units). A surrogate
/// pair that would be cut in half is left out instead of split.
pub fn js_slice(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// Integer count as the TSX prints it (`String(n)`), `—` when unknown.
pub fn count(n: Option<u64>) -> String {
    n.map(|v| v.to_string()).unwrap_or_else(|| "—".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for d in [-1_000_000i64, -1, 0, 1, 19_000, 20_747, 2_000_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2026, 10, 21), 20_747);
    }

    #[test]
    fn zagreb_dst_edges() {
        // 2026-03-29 01:00 UTC: CET -> CEST.
        let spring = days_from_civil(2026, 3, 29) * 86_400 + 3_600;
        assert_eq!(zagreb(spring - 60), "2026-03-29 01:59 CET (+01:00)");
        assert_eq!(zagreb(spring), "2026-03-29 03:00 CEST (+02:00)");
        // 2026-10-25 01:00 UTC: CEST -> CET.
        let autumn = days_from_civil(2026, 10, 25) * 86_400 + 3_600;
        assert_eq!(zagreb(autumn - 60), "2026-10-25 02:59 CEST (+02:00)");
        assert_eq!(zagreb(autumn), "2026-10-25 02:00 CET (+01:00)");
        // Live persona.md mtime shape: 2026-10-18 17:40 UTC.
        let t = days_from_civil(2026, 10, 18) * 86_400 + 17 * 3600 + 40 * 60;
        assert_eq!(zagreb(t), "2026-10-18 19:40 CEST (+02:00)");
        // Winter, new year boundary.
        let ny = days_from_civil(2027, 1, 1) * 86_400 - 1;
        assert_eq!(zagreb(ny), "2027-01-01 00:59 CET (+01:00)");
        // 2027: last Sunday of March is the 28th.
        let s27 = days_from_civil(2027, 3, 28) * 86_400 + 3_600;
        assert_eq!(zagreb_offset(s27 - 1), 3_600);
        assert_eq!(zagreb_offset(s27), 7_200);
    }

    #[test]
    fn js_slice_units() {
        assert_eq!(js_slice("abc", 2), "ab");
        assert_eq!(js_slice("abc", 9), "abc");
        assert_eq!(js_slice("čćž", 2), "čć");
        // U+1F600 is two UTF-16 units.
        assert_eq!(js_slice("a\u{1F600}b", 2), "a");
        assert_eq!(js_slice("a\u{1F600}b", 3), "a\u{1F600}");
        assert_eq!(js_slice("", 400), "");
        assert_eq!(count(None), "—");
        assert_eq!(count(Some(249)), "249");
    }
}
