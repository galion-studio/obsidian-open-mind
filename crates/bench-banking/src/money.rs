// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Money in integer minor units (cents), currency codes and IBAN checks.
//! No floats in arithmetic: JSON decimals are converted once at the edge.

use serde_json::Value;

/// Largest amount the bench accepts in one draft (1 000 000.00).
pub const MAX_MINOR: i64 = 100_000_000;

/// JSON number (as Revolut sends it, e.g. `2450.75`) -> minor units.
pub fn json_to_minor(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return i.checked_mul(100);
    }
    let f = v.as_f64()?;
    if !f.is_finite() || f.abs() > 1e12 {
        return None;
    }
    let m = (f * 100.0).round();
    if ((m / 100.0) - f).abs() > 1e-6 {
        return None;
    }
    Some(m as i64)
}

/// Minor units -> JSON number with two decimals.
pub fn minor_to_json(m: i64) -> Value {
    serde_json::from_str(&format_minor(m)).unwrap_or(Value::Null)
}

/// Minor units -> `"-12.30"`.
pub fn format_minor(m: i64) -> String {
    let sign = if m < 0 { "-" } else { "" };
    let a = m.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}

/// Strict user amount: `"12"`, `"12.3"`, `"12.34"`; positive; at most two
/// decimals; at most [`MAX_MINOR`].
pub fn parse_amount(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s, ""),
    };
    let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(frac) || frac.len() > 2 || whole.len() > 9 {
        return Err(format!(
            "amount must be a positive decimal with at most 2 places: {s:?}"
        ));
    }
    let w: i64 = whole.parse().map_err(|_| "bad amount")?;
    let f: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<2}").parse().map_err(|_| "bad amount")?
    };
    let m = w * 100 + f;
    if m <= 0 {
        return Err("amount must be greater than zero".into());
    }
    if m > MAX_MINOR {
        return Err("amount above the bench ceiling".into());
    }
    Ok(m)
}

/// ISO 4217 shape: three ASCII capitals.
pub fn valid_currency(c: &str) -> bool {
    c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase())
}

/// IBAN with the ISO 13616 mod-97 check.
pub fn valid_iban(raw: &str) -> bool {
    let s: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    if !(15..=34).contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return false;
    }
    let b = s.as_bytes();
    if !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
        return false;
    }
    if !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return false;
    }
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u32 = 0;
    for c in rearranged.chars() {
        let v = if c.is_ascii_digit() {
            c as u32 - '0' as u32
        } else {
            c as u32 - 'A' as u32 + 10
        };
        rem = if v >= 10 {
            (rem * 100 + v) % 97
        } else {
            (rem * 10 + v) % 97
        };
    }
    rem == 1
}

/// Normalised IBAN (no spaces, upper case).
pub fn norm_iban(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// `HR12 **** **** 0160` style mask: country + check digits + last 4.
pub fn mask_iban(raw: &str) -> String {
    let s = norm_iban(raw);
    if s.len() < 8 {
        return "****".into();
    }
    format!("{} **** {}", &s[..4], &s[s.len() - 4..])
}

#[cfg(test)]
mod t {
    use super::*;
    use serde_json::json;

    #[test]
    fn amounts() {
        assert_eq!(parse_amount("12.34"), Ok(1234));
        assert_eq!(parse_amount("12.3"), Ok(1230));
        assert_eq!(parse_amount("7"), Ok(700));
        for bad in [
            "12.345", "-5", "abc", "0", "0.00", "", ".5", "1e3", "2000000",
        ] {
            assert!(parse_amount(bad).is_err(), "{bad}");
        }
        assert_eq!(json_to_minor(&json!(2450.75)), Some(245075));
        assert_eq!(json_to_minor(&json!(-12.3)), Some(-1230));
        assert_eq!(json_to_minor(&json!(3)), Some(300));
        assert_eq!(json_to_minor(&json!(1.005)), None);
        assert_eq!(format_minor(-1230), "-12.30");
        assert_eq!(minor_to_json(245075), json!(2450.75));
    }

    #[test]
    fn ibans() {
        assert!(valid_iban("HR1210010051863000160"));
        assert!(valid_iban("DE89 3704 0044 0532 0130 00"));
        assert!(valid_iban("GB29NWBK60161331926819"));
        assert!(!valid_iban("HR1210010051863000161"));
        assert!(!valid_iban("XX00"));
        assert_eq!(mask_iban("HR1210010051863000160"), "HR12 **** 0160");
        assert!(valid_currency("EUR") && !valid_currency("eur") && !valid_currency("EURO"));
    }
}
