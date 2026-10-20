// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! fail2ban-style durations: `600`, `10m`, `1h`, `1d 8h`, `1w`.

/// Parse a duration into seconds. Units: s, m, h, d, w (also the long
/// forms `sec`, `min`, `hour`, `day`, `week`, with optional plural `s`).
/// A bare number is seconds. Negative or empty input is an error: a
/// permanent ban (fail2ban `-1`) is deliberately not supported.
pub fn parse(s: &str) -> Result<u64, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty duration".into());
    }
    let mut total: u64 = 0;
    let mut rest = t;
    let mut any = false;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return Err(format!("bad duration {s:?}"));
        }
        let n: u64 = rest[..digits]
            .parse()
            .map_err(|_| format!("bad number in {s:?}"))?;
        rest = rest[digits..].trim_start();
        let ulen = rest.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
        let unit = &rest[..ulen];
        rest = &rest[ulen..];
        let mult = match unit.to_ascii_lowercase().as_str() {
            "" | "s" | "sec" | "secs" | "second" | "seconds" => 1,
            "m" | "min" | "mins" | "minute" | "minutes" => 60,
            "h" | "hour" | "hours" => 3600,
            "d" | "day" | "days" => 86_400,
            "w" | "week" | "weeks" => 604_800,
            other => return Err(format!("unknown unit {other:?} in {s:?}")),
        };
        total = n
            .checked_mul(mult)
            .and_then(|v| total.checked_add(v))
            .ok_or_else(|| format!("duration overflow in {s:?}"))?;
        any = true;
    }
    if !any {
        return Err(format!("bad duration {s:?}"));
    }
    Ok(total)
}

/// Format seconds the way fail2ban prints them: `1d 8h`, `2h`, `10m`, `45s`.
pub fn format(secs: u64) -> String {
    if secs == 0 {
        return "0s".into();
    }
    let parts = [
        (604_800, "w"),
        (86_400, "d"),
        (3600, "h"),
        (60, "m"),
        (1, "s"),
    ];
    let mut left = secs;
    let mut out = Vec::new();
    for (n, u) in parts {
        if left >= n {
            out.push(format!("{}{u}", left / n));
            left %= n;
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fail2ban_forms() {
        assert_eq!(parse("600").unwrap(), 600);
        assert_eq!(parse("10m").unwrap(), 600);
        assert_eq!(parse("1h").unwrap(), 3600);
        assert_eq!(parse("1d 8h").unwrap(), 115_200);
        assert_eq!(parse("1w").unwrap(), 604_800);
        assert_eq!(parse("2 hours").unwrap(), 7200);
        assert!(parse("-1").is_err());
        assert!(parse("").is_err());
        assert!(parse("1x").is_err());
        assert!(parse("99999999999999999999w").is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(format(115_200), "1d 8h");
        assert_eq!(format(7200), "2h");
        assert_eq!(format(61), "1m 1s");
        assert_eq!(format(0), "0s");
    }
}
