// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! JavaScript number and text formatting, so the Rust pages print exactly
//! what the TSX printed: `String(n)`, `n.toFixed(d)`, `Math.round`, and the
//! "drop the word undefined, squeeze whitespace" helpers the rooms used.

use crate::json::Value;

/// `String(n)` for a finite number (Rust's shortest round-trip `Display`
/// matches JS for the ranges a console shows; `-0` prints `0` like JS).
pub fn num(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    format!("{n}")
}

/// `Math.round(n)`: halves go up (towards +infinity), unlike `f64::round`.
pub fn round(n: f64) -> f64 {
    let f = n.floor();
    if n - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// `n.toFixed(d)` (d <= 6): an exact tie picks the larger digit like JS,
/// where Rust's formatter rounds half to even.
pub fn to_fixed(n: f64, d: usize) -> String {
    if !n.is_finite() {
        return num(n);
    }
    // `(-0).toFixed(d)` is "0.00" in JS (but `(-0.001).toFixed(2)` is "-0.00").
    let n = if n == 0.0 { 0.0 } else { n };
    let p = 10f64.powi(d as i32);
    let scaled = n.abs() * p;
    if scaled < 1e15 && scaled.fract() == 0.5 {
        let up = (scaled.floor() + 1.0) / p;
        format!("{:.d$}", up * n.signum(), d = d)
    } else {
        format!("{n:.d$}", d = d)
    }
}

/// A finite JSON number, else `None` (`typeof v === "number" && isFinite`).
pub fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `s.replace(/\s{2,}/g, " ").trim()`.
pub fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0usize;
    let mut first = '\0';
    for c in s.chars() {
        if is_space(c) {
            if run == 0 {
                first = c;
            }
            run += 1;
            continue;
        }
        if run == 1 {
            out.push(first);
        } else if run > 1 {
            out.push(' ');
        }
        run = 0;
        out.push(c);
    }
    if run == 1 {
        out.push(first);
    } else if run > 1 {
        out.push(' ');
    }
    out.trim_matches(is_space).to_string()
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Replace every whole word `w` (JS `\bw\b`, ASCII word chars; case-folded
/// when `fold`) with one space.
pub fn drop_word(s: &str, w: &str, fold: bool) -> String {
    let hay: Vec<char> = s.chars().collect();
    let pat: Vec<char> = w.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < hay.len() {
        let end = i + pat.len();
        let hit = end <= hay.len()
            && hay[i..end].iter().zip(&pat).all(|(a, b)| {
                if fold {
                    a.eq_ignore_ascii_case(b)
                } else {
                    a == b
                }
            })
            && (i == 0 || !word_char(hay[i - 1]))
            && (end == hay.len() || !word_char(hay[end]));
        if hit {
            out.push(' ');
            i = end;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// `text()` of cache.tsx / `shown()` of analytics.tsx for strings: number ->
/// `String(n)`; string -> drop the word `undefined`, squeeze, trim; empty or
/// any other type -> `fallback`.
pub fn text(v: &Value, fallback: &str) -> String {
    if let Some(n) = finite(v) {
        return num(n);
    }
    match v.as_str() {
        Some(s) => {
            let t = squeeze(&drop_word(s, "undefined", false));
            if t.is_empty() {
                fallback.to_string()
            } else {
                t
            }
        }
        None => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn numbers_match_js() {
        assert_eq!(num(3.0), "3");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(12.5), "12.5");
        assert_eq!(num(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(num(109107266.0), "109107266");
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49), 0.0);
    }

    #[test]
    fn to_fixed_matches_js() {
        // values checked against node: (x).toFixed(d)
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(0.25, 1), "0.3");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(109.107266, 1), "109.1");
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(-1.5, 2), "-1.50");
        assert_eq!(to_fixed(2.0, 0), "2");
        assert_eq!(to_fixed(f64::NAN, 2), "NaN");
    }

    #[test]
    fn text_helpers() {
        let v =
            parse(br#"{"a":"  undefined  x  undefinedness ","b":7,"c":"","d":null,"e":"a\tb"}"#)
                .unwrap();
        assert_eq!(text(v.get("a"), "-"), "x undefinedness");
        assert_eq!(text(v.get("b"), "-"), "7");
        assert_eq!(text(v.get("c"), "-"), "-");
        assert_eq!(text(v.get("d"), "-"), "-");
        assert_eq!(text(v.get("e"), "-"), "a\tb");
        assert_eq!(text(v.get("zz"), "0"), "0");
        assert_eq!(drop_word("Undefined x", "undefined", true), "  x");
        assert_eq!(squeeze(" a  \n b "), "a b");
    }
}
