// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Timestamps at the start of syslog lines (auth.log), without a date crate.

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`, public-domain algorithm, re-implemented).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Year of a unix second (UTC).
pub fn year_of(secs: i64) -> i64 {
    let days = secs.div_euclid(86_400);
    // civil_from_days, year only.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + if m <= 2 { 1 } else { 0 }
}

fn num(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn hms(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.splitn(3, ':');
    let h = num(it.next()?)?;
    let m = num(it.next()?)?;
    let sec = num(it.next()?)?;
    (h < 24 && m < 60 && sec < 61).then_some((h, m, sec))
}

/// Split a syslog line into (unix seconds, rest). Understands
/// * RFC 3339 / rsyslog high precision: `2026-10-20T18:49:14.028938+00:00 host ...`
///   (also `Z` and `+hhmm`),
/// * BSD: `Oct 20 18:49:14 host ...` (no year, no zone: `bsd_offset_secs`
///   is the zone of the log, the year is inferred from `now` and a date more
///   than a day in the future is taken from the previous year).
pub fn split_stamp(line: &str, now: i64, bsd_offset_secs: i64) -> Option<(i64, &str)> {
    let (stamp, rest) = line.split_once(' ')?;
    if stamp.len() >= 19 && stamp.as_bytes()[4] == b'-' {
        return parse_rfc3339(stamp).map(|t| (t, rest));
    }
    // BSD: "Mmm dd hh:mm:ss " (day may be space padded: "Oct  1").
    let mon = month(stamp)?;
    let rest = rest.trim_start_matches(' ');
    let (day, rest) = rest.split_once(' ')?;
    let day = num(day).filter(|d| (1..=31).contains(d))?;
    let (clock, rest) = rest.split_once(' ')?;
    let (h, mi, s) = hms(clock)?;
    let mut year = year_of(now + bsd_offset_secs);
    loop {
        let t = days_from_civil(year, mon, day) * 86_400 + (h * 3600 + mi * 60 + s) as i64
            - bsd_offset_secs;
        if t > now + 86_400 && year > 1970 {
            year -= 1;
            continue;
        }
        return Some((t, rest));
    }
}

fn month(s: &str) -> Option<u32> {
    const M: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    M.iter().position(|m| *m == s).map(|i| i as u32 + 1)
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|+hh:mm|-hh:mm|+hhmm)` to unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    if s.len() < 20 || !s.is_ascii() {
        return None;
    }
    let b = s.as_bytes();
    if b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') {
        return None;
    }
    let y = num(&s[0..4])? as i64;
    let mo = num(&s[5..7]).filter(|m| (1..=12).contains(m))?;
    let d = num(&s[8..10]).filter(|d| (1..=31).contains(d))?;
    let (h, mi, sec) = hms(&s[11..19])?;
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        let n = r.bytes().take_while(|c| c.is_ascii_digit()).count();
        if n == 0 {
            return None;
        }
        rest = &r[n..];
    }
    let off = if rest == "Z" {
        0
    } else {
        let sign = match rest.as_bytes().first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let z = rest[1..].replace(':', "");
        if z.len() != 4 {
            return None;
        }
        let oh = num(&z[0..2])? as i64;
        let om = num(&z[2..4])? as i64;
        if oh > 23 || om > 59 {
            return None;
        }
        sign * (oh * 3600 + om * 60)
    };
    Some(days_from_civil(y, mo, d) * 86_400 + (h * 3600 + mi * 60 + sec) as i64 - off)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        // 2026-10-20T18:49:14+00:00 == 1792522154
        let t = parse_rfc3339("2026-10-20T18:49:14.028938+00:00").unwrap();
        assert_eq!(
            t,
            days_from_civil(2026, 10, 20) * 86_400 + 18 * 3600 + 49 * 60 + 14
        );
        assert_eq!(parse_rfc3339("2026-10-20T20:49:14+02:00").unwrap(), t);
        assert_eq!(parse_rfc3339("2026-10-20T18:49:14Z").unwrap(), t);
        assert_eq!(parse_rfc3339("2026-10-20T20:49:14+0200").unwrap(), t);
        assert!(parse_rfc3339("2026-13-20T18:49:14Z").is_none());
        assert!(parse_rfc3339("2026-10-20T18:49:14").is_none());
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(year_of(t), 2026);
        assert_eq!(year_of(0), 1970);
    }

    #[test]
    fn split() {
        let now = parse_rfc3339("2026-10-20T19:00:00Z").unwrap();
        let (t, rest) = split_stamp("2026-10-20T18:49:14.1+00:00 ip50 sshd[1]: x", now, 0).unwrap();
        assert_eq!(rest, "ip50 sshd[1]: x");
        assert_eq!(t, parse_rfc3339("2026-10-20T18:49:14Z").unwrap());
        let (t2, rest) = split_stamp("Oct 20 18:49:14 ip50 sshd[1]: x", now, 0).unwrap();
        assert_eq!((t2, rest), (t, "ip50 sshd[1]: x"));
        let (t3, _) = split_stamp("Oct 20 20:49:14 ip50 sshd[1]: x", now, 7200).unwrap();
        assert_eq!(t3, t);
        // "Dec 31" seen on Jan 1 belongs to last year.
        let jan = parse_rfc3339("2027-01-01T00:10:00Z").unwrap();
        let (t4, _) = split_stamp("Dec 31 23:59:00 h sshd: x", jan, 0).unwrap();
        assert_eq!(t4, parse_rfc3339("2026-12-31T23:59:00Z").unwrap());
        let (t5, _) = split_stamp("Oct  1 01:02:03 h sshd: x", now, 0).unwrap();
        assert_eq!(t5, parse_rfc3339("2026-10-01T01:02:03Z").unwrap());
        assert!(split_stamp("garbage", now, 0).is_none());
        assert!(split_stamp("Foo 1 00:00:00 h", now, 0).is_none());
    }
}
