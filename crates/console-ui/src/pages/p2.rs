// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Helpers shared by the console-port-2 rooms (`context`, `gateway`,
//! `guardian`, `hermes`): JS-like number/text formatting, Zagreb clock
//! stamps and a `JSON.stringify(v, null, 2)` printer. std only.

use super::tidy;
use crate::json::{write_str, Value};

/// JS `String(n)` for the numbers the API sends (integers without `.0`).
pub fn js_num(n: f64) -> String {
    if !n.is_finite() {
        return "NaN".into();
    }
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// TSX `text(value, fallback)`: numbers as `String(n)`, strings without the
/// word `undefined` and squeezed, anything else (or empty) is the fallback.
pub fn text(v: &Value, fallback: &str) -> String {
    let out = match v {
        Value::Num(n) if n.is_finite() => js_num(*n),
        Value::Str(s) => tidy(s),
        _ => String::new(),
    };
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

/// TSX `num(value)`: a finite number or 0.
pub fn num(v: &Value) -> f64 {
    v.as_f64().filter(|n| n.is_finite()).unwrap_or(0.0)
}

/// JS `n.toFixed(1)`.
pub fn fixed1(n: f64) -> String {
    format!("{n:.1}")
}

/// TSX `compact(n)`: 0, 1.2k, 3.4M.
pub fn compact(n: f64) -> String {
    if n == 0.0 {
        "0".into()
    } else if n >= 1_000_000.0 {
        format!("{}M", fixed1(n / 1_000_000.0))
    } else if n >= 1_000.0 {
        format!("{}k", fixed1(n / 1_000.0))
    } else {
        js_num(n)
    }
}

/// TSX `gb(n)`: "idle" or "1.2 GB".
pub fn gb(n: f64) -> String {
    if n == 0.0 {
        "idle".into()
    } else {
        format!("{} GB", fixed1(n / 1_000_000_000.0))
    }
}

/// Days since 1970-01-01 for a civil date (H. Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date for days since 1970-01-01.
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
fn last_sunday_0100_utc(year: i64, month: i64) -> i64 {
    let next = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    let last = next - 1;
    // 1970-01-01 was a Thursday (weekday 4, Sunday = 0).
    let wd = (last + 4).rem_euclid(7);
    (last - wd) * 86_400 + 3_600
}

/// Europe/Zagreb UTC offset in hours at a Unix time (CET +1, CEST +2).
pub fn zagreb_offset(secs: i64) -> i64 {
    let (y, _, _) = civil_from_days(secs.div_euclid(86_400));
    if secs >= last_sunday_0100_utc(y, 3) && secs < last_sunday_0100_utc(y, 10) {
        2
    } else {
        1
    }
}

/// Zagreb wall clock `(y, m, d, hh, mm, offset_hours)` for Unix seconds.
pub fn zagreb(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let off = zagreb_offset(secs);
    let local = secs + off * 3_600;
    let (y, m, d) = civil_from_days(local.div_euclid(86_400));
    let s = local.rem_euclid(86_400);
    (y, m, d, s / 3_600, (s % 3_600) / 60, off)
}

/// TSX `stamp(n)` (`MM-DD HH:MM`), on the Zagreb clock instead of the
/// browser's; empty for 0.
pub fn stamp(n: f64) -> String {
    if n == 0.0 || !n.is_finite() {
        return String::new();
    }
    let (_, m, d, hh, mm, _) = zagreb(n.floor() as i64);
    format!("{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// The zone label for a stamp (`CEST` / `CET`).
pub fn zone_label(n: f64) -> &'static str {
    if zagreb_offset(n.floor() as i64) == 2 {
        "CEST"
    } else {
        "CET"
    }
}

/// `JSON.stringify(v, null, 2)`.
pub fn pretty(v: &Value) -> String {
    let mut out = String::new();
    pretty_into(&mut out, v, 0);
    out
}

fn pretty_into(out: &mut String, v: &Value, depth: usize) {
    let pad = |out: &mut String, d: usize| {
        out.push('\n');
        for _ in 0..d {
            out.push_str("  ");
        }
    };
    match v {
        Value::Arr(a) if !a.is_empty() => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, depth + 1);
                pretty_into(out, x, depth + 1);
            }
            pad(out, depth);
            out.push(']');
        }
        Value::Obj(m) if !m.is_empty() => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                pad(out, depth + 1);
                write_str(out, k);
                out.push_str(": ");
                pretty_into(out, x, depth + 1);
            }
            pad(out, depth);
            out.push('}');
        }
        other => out.push_str(&other.to_json()),
    }
}

/// Cut `s` to at most `max` chars, saying how much was left out.
pub fn clip(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}\n… ({} more characters not shown)", n - max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn numbers_like_js() {
        assert_eq!(compact(0.0), "0");
        assert_eq!(compact(999.0), "999");
        assert_eq!(compact(1_500.0), "1.5k");
        assert_eq!(compact(109_107_266.0), "109.1M");
        assert_eq!(gb(0.0), "idle");
        assert_eq!(gb(8_200_000_000.0), "8.2 GB");
        assert_eq!(js_num(65536.0), "65536");
        assert_eq!(js_num(1.5), "1.5");
        assert_eq!(text(&Value::Str("a undefined  b".into()), "x"), "a b");
        assert_eq!(text(&Value::Null, "x"), "x");
        assert_eq!(text(&Value::Num(12.0), "x"), "12");
    }

    #[test]
    fn zagreb_clock_and_dst() {
        // 2026-10-18 16:26:45 UTC -> 18:26 CEST.
        assert_eq!(stamp(1_792_340_805.0), "10-18 18:26");
        assert_eq!(zone_label(1_792_340_805.0), "CEST");
        // DST ends 2026-10-25 01:00 UTC: 00:59 UTC is CEST, 01:00 UTC is CET.
        let end = last_sunday_0100_utc(2026, 10);
        assert_eq!(end, 1_792_890_000);
        assert_eq!(zagreb_offset(end - 1), 2);
        assert_eq!(zagreb_offset(end), 1);
        assert_eq!(zagreb(end), (2026, 10, 25, 2, 0, 1));
        // DST starts 2026-03-29 01:00 UTC.
        let start = last_sunday_0100_utc(2026, 3);
        assert_eq!(zagreb(start - 60), (2026, 3, 29, 1, 59, 1));
        assert_eq!(zagreb(start), (2026, 3, 29, 3, 0, 2));
        assert_eq!(stamp(0.0), "");
        assert_eq!(zagreb(0), (1970, 1, 1, 1, 0, 1));
    }

    #[test]
    fn pretty_matches_json_stringify() {
        let v = parse(br#"{"a":1,"b":[true,null,"x\"y"],"c":{},"d":[],"e":{"f":2.5}}"#).unwrap();
        assert_eq!(
            pretty(&v),
            "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null,\n    \"x\\\"y\"\n  ],\n  \"c\": {},\n  \"d\": [],\n  \"e\": {\n    \"f\": 2.5\n  }\n}"
        );
        assert_eq!(clip("abcdef", 3), "abc\n… (3 more characters not shown)");
        assert_eq!(clip("abc", 3), "abc");
    }
}
