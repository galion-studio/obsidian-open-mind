// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Std-only JSON output helpers for crates that build JSON by hand.
//!
//! [`push_ascii_escaped`] is the one place that knows CPython's
//! `ensure_ascii=True` rule (every non-ASCII code point as `\uXXXX`, astral
//! code points as a surrogate pair, lower-case hex). The serde `jsonout`
//! formatter (crate galion-jsonout) uses it too.

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
}
