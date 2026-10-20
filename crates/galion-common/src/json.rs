// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Std-only JSON output helpers for crates that build JSON by hand.
//!
//! [`push_ascii_escaped`] is the one place that knows CPython's
//! `ensure_ascii=True` rule (every non-ASCII code point as `\uXXXX`, astral
//! code points as a surrogate pair, lower-case hex). The serde `jsonout`
//! formatter (feature `serde-json`) uses it too.

use std::fmt::Write as _;

/// Append `s` as a quoted JSON string. Escapes `"`, `\` and control
/// characters; leaves non-ASCII as UTF-8.
pub fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        push_char(out, c, false);
    }
    out.push('"');
}

/// `s` as a quoted JSON string (see [`push_str`]).
pub fn string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    push_str(&mut o, s);
    o
}

/// Append `s` as a quoted JSON string the way CPython `json.dumps` writes it
/// with `ensure_ascii=True` (the default).
pub fn push_str_ascii(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        push_char(out, c, true);
    }
    out.push('"');
}

/// `s` as CPython `json.dumps(s)` would write it.
pub fn string_ascii(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    push_str_ascii(&mut o, s);
    o
}

fn push_char(out: &mut String, c: char, ascii: bool) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{08}' => out.push_str("\\b"),
        '\u{0c}' => out.push_str("\\f"),
        c if (c as u32) < 0x20 => {
            let _ = write!(out, "\\u{:04x}", c as u32);
        }
        c if ascii && !c.is_ascii() => push_ascii_escaped(out, c),
        c => out.push(c),
    }
}

/// Append one non-ASCII code point as `\uXXXX` (or a surrogate pair).
pub fn push_ascii_escaped(out: &mut String, c: char) {
    let cp = c as u32;
    if cp <= 0xFFFF {
        let _ = write!(out, "\\u{cp:04x}");
    } else {
        let v = cp - 0x1_0000;
        let hi = 0xD800 + (v >> 10);
        let lo = 0xDC00 + (v & 0x3FF);
        let _ = write!(out, "\\u{hi:04x}\\u{lo:04x}");
    }
}

/// CPython `repr(float)` (what `json.dumps` writes for a float).
///
/// Shortest round-trip digits (Rust `{:e}` and CPython both pick the
/// shortest, closest decimal), laid out the CPython way: positional when the
/// decimal exponent `d` (value = 0.DIGITS x 10^d) satisfies `-4 < d <= 16`,
/// with `.0` on integral values; otherwise `De[+-]XX` with a sign and at
/// least two exponent digits (`1e+16`, `1e-05`). serde_json's own float
/// writer (1.0.154) differs for small values (`1.5e-7`, `0.00001`). Non-finite values (which JSON
/// cannot hold and serde_json writes as `null`) give `NaN` / `Infinity` /
/// `-Infinity` like CPython.
pub fn float_repr(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let sci = format!("{:e}", v.abs());
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let decpt = exp + 1;
    let mut out = String::with_capacity(digits.len() + 8);
    if v < 0.0 {
        out.push('-');
    }
    if decpt <= -4 || decpt > 16 {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(
            out,
            "e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.unsigned_abs()
        );
    } else if decpt <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', decpt.unsigned_abs() as usize));
        out.push_str(&digits);
    } else {
        let point = decpt as usize;
        if point >= digits.len() {
            out.push_str(&digits);
            out.extend(std::iter::repeat_n('0', point - digits.len()));
            out.push_str(".0");
        } else {
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(string("a\"b\\c\n\u{1}"), "\"a\\\"b\\\\c\\n\\u0001\"");
        assert_eq!(string("é"), "\"é\"");
        assert_eq!(string_ascii("é😀"), "\"\\u00e9\\ud83d\\ude00\"");
        assert_eq!(string_ascii("\t\u{8}\u{c}\r"), "\"\\t\\b\\f\\r\"");
    }

    /// Expected strings are CPython 3 `repr(float)` outputs.
    #[test]
    fn float_repr_matches_cpython() {
        let cases: [(f64, &str); 20] = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-2.5, "-2.5"),
            (0.1, "0.1"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (2.5e-10, "2.5e-10"),
            (123456789.0, "123456789.0"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.2345678901234568e17, "1.2345678901234568e+17"),
            (1e22, "1e+22"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
            (8.888000000937703e46, "8.888000000937703e+46"),
            (f64::INFINITY, "Infinity"),
            (f64::NAN, "NaN"),
        ];
        for (v, want) in cases {
            assert_eq!(float_repr(v), want, "{v:e}");
        }
    }
}
