// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Money is integer cents (`i64`). No floats touch a price, a fill or a P&L,
//! so a replay is bit-for-bit reproducible on every target.

/// Parse a non-negative decimal price such as `42.16`, `42.1` or `42` into cents.
pub fn parse_cents(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty price".into());
    }
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s, ""),
    };
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("bad price {s:?}"));
    }
    if frac.len() > 2 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("bad price {s:?} (max two decimals)"));
    }
    let w: i64 = whole.parse().map_err(|_| format!("bad price {s:?}"))?;
    let mut f: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| format!("bad price {s:?}"))?
    };
    if frac.len() == 1 {
        f *= 10;
    }
    w.checked_mul(100)
        .and_then(|c| c.checked_add(f))
        .ok_or_else(|| format!("price out of range {s:?}"))
}

/// Render cents as `-1234.56` style text.
pub fn fmt_cents(c: i64) -> String {
    let sign = if c < 0 { "-" } else { "" };
    let a = c.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}

/// `amount * bps / 10_000`, rounded half away from zero.
pub fn apply_bps(amount: i64, bps: i64) -> i64 {
    let p = amount as i128 * bps as i128;
    let q = if p >= 0 {
        (p + 5_000) / 10_000
    } else {
        (p - 5_000) / 10_000
    };
    q as i64
}

/// Basis points of `num / den`, rounded toward zero; 0 when `den` is 0.
pub fn ratio_bps(num: i64, den: i64) -> i64 {
    if den == 0 {
        0
    } else {
        ((num as i128 * 10_000) / den as i128) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        assert_eq!(parse_cents("42.16"), Ok(4216));
        assert_eq!(parse_cents("42.1"), Ok(4210));
        assert_eq!(parse_cents("42"), Ok(4200));
        assert!(parse_cents("-1").is_err());
        assert!(parse_cents("1.234").is_err());
        assert!(parse_cents("abc").is_err());
        assert!(parse_cents(".5").is_err());
        assert_eq!(fmt_cents(-4216), "-42.16");
        assert_eq!(fmt_cents(5), "0.05");
    }

    #[test]
    fn bps_rounding() {
        assert_eq!(apply_bps(10_000, 5), 5);
        assert_eq!(apply_bps(999, 5), 0);
        assert_eq!(apply_bps(1_000, 5), 1);
        assert_eq!(apply_bps(-1_000, 5), -1);
        assert_eq!(ratio_bps(50, 1_000), 500);
        assert_eq!(ratio_bps(1, 0), 0);
    }
}
