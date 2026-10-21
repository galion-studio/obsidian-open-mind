// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Europe/Zagreb RFC 3339 time, std only (no chrono, no tz database).
//!
//! Same API and output as the `zagreb-time` crate (`now_rfc3339`,
//! `from_unix`, `label`, `require_rfc3339`) for crates that want to stay
//! dependency-free. Zagreb follows the EU rule since 1996: CEST (+02:00) from
//! the last Sunday of March 01:00 UTC to the last Sunday of October 01:00 UTC,
//! CET (+01:00) otherwise. Instants before 1996 use the same rule (the real
//! 1983-1995 autumn change was in September); nothing in this stack prints
//! those. `tests/time_vs_chrono.rs` checks every hour boundary around each
//! switch from 1996 to 2099 against chrono-tz.

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day) for days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Unix second of the last Sunday of `month` (31-day month) at 01:00 UTC.
fn last_sunday_0100_utc(year: i64, month: u32) -> i64 {
    let last = days_from_civil(year, month, 31);
    // 1970-01-01 was a Thursday; weekday 0 = Sunday.
    let wd = (last + 4).rem_euclid(7);
    (last - wd) * 86_400 + 3_600
}

/// Offset from UTC in seconds at unix second `secs` (3600 or 7200).
pub fn offset_at(secs: i64) -> i64 {
    let (year, _, _) = civil_from_days(secs.div_euclid(86_400));
    let start = last_sunday_0100_utc(year, 3);
    let end = last_sunday_0100_utc(year, 10);
    if secs >= start && secs < end {
        7_200
    } else {
        3_600
    }
}

struct Local {
    y: i64,
    mo: u32,
    d: u32,
    h: i64,
    mi: i64,
    s: i64,
    off: i64,
}

fn local(secs: i64) -> Local {
    let off = offset_at(secs);
    let t = secs + off;
    let (y, mo, d) = civil_from_days(t.div_euclid(86_400));
    let sod = t.rem_euclid(86_400);
    Local {
        y,
        mo,
        d,
        h: sod / 3600,
        mi: sod / 60 % 60,
        s: sod % 60,
        off,
    }
}

fn now_secs() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// A unix second as Europe/Zagreb RFC 3339, e.g. `2026-10-18T19:05:00+02:00`.
pub fn from_unix(secs: i64) -> String {
    let l = local(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+{:02}:00",
        l.y,
        l.mo,
        l.d,
        l.h,
        l.mi,
        l.s,
        l.off / 3600
    )
}

/// The current instant as Europe/Zagreb RFC 3339.
pub fn now_rfc3339() -> String {
    from_unix(now_secs())
}

/// A unix second as a human label, e.g. `2026-10-18 19:05 CEST (+02:00)`.
pub fn label(secs: i64) -> String {
    let l = local(secs);
    let (name, off) = if l.off == 7_200 {
        ("CEST", "+02:00")
    } else {
        ("CET", "+01:00")
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} {name} ({off})",
        l.y, l.mo, l.d, l.h, l.mi
    )
}

/// Why [`require_rfc3339`] refused a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampReject {
    /// The timestamp ends in `Z`.
    Zulu,
    /// Not seconds-precision RFC 3339 with `+01:00` or `+02:00`.
    NotZagreb,
}

/// Refuse a timestamp that is not Europe/Zagreb RFC 3339 (`Z` is refused;
/// seconds precision and an explicit `+01:00` / `+02:00` are required).
pub fn require_rfc3339(ts: &str) -> Result<(), StampReject> {
    if ts.ends_with('Z') || ts.ends_with('z') {
        return Err(StampReject::Zulu);
    }
    let b = ts.as_bytes();
    if b.len() != 25 || !(ts.ends_with("+01:00") || ts.ends_with("+02:00")) {
        return Err(StampReject::NotZagreb);
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let s = &b[r];
        if s.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(s).ok()?.parse().ok()
        } else {
            None
        }
    };
    let ok = (|| {
        let y = num(0..4)?;
        let mo = num(5..7)?;
        let d = num(8..10)?;
        let h = num(11..13)?;
        let mi = num(14..16)?;
        let s = num(17..19)?;
        let seps = b[4] == b'-'
            && b[7] == b'-'
            && matches!(b[10], b'T' | b't' | b' ')
            && b[13] == b':'
            && b[16] == b':';
        let dim = match mo {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
            2 => 28,
            _ => return None,
        };
        (seps && (1..=dim).contains(&d) && h < 24 && mi < 60 && s <= 60).then_some(())
    })();
    ok.ok_or(StampReject::NotZagreb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_instants() {
        // 2026-10-20T17:00:00Z
        assert_eq!(from_unix(1_792_515_600), "2026-10-20T19:00:00+02:00");
        // 2026-10-25T01:00:00Z: the switch to CET.
        assert_eq!(from_unix(1_792_890_000 - 1), "2026-10-25T02:59:59+02:00");
        assert_eq!(from_unix(1_792_890_000), "2026-10-25T02:00:00+01:00");
        assert_eq!(from_unix(0), "1970-01-01T01:00:00+01:00");
        assert_eq!(label(1_792_515_600), "2026-10-20 19:00 CEST (+02:00)");
        assert!(now_rfc3339().len() == 25);
    }

    #[test]
    fn civil_round_trip() {
        for z in (-800_000..800_000).step_by(37) {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn stamp_check() {
        assert_eq!(require_rfc3339("2026-10-20T19:00:00+02:00"), Ok(()));
        assert_eq!(
            require_rfc3339("2026-10-20T17:00:00Z"),
            Err(StampReject::Zulu)
        );
        assert_eq!(
            require_rfc3339("2026-02-30T19:00:00+01:00"),
            Err(StampReject::NotZagreb)
        );
        assert_eq!(
            require_rfc3339("2026-10-20T19:00:00+00:00"),
            Err(StampReject::NotZagreb)
        );
        assert_eq!(
            require_rfc3339("2026-10-20X19:00:00+02:00"),
            Err(StampReject::NotZagreb)
        );
    }
}
