// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! Tolerant key scanners over JSON-ish text.
//!
//! The desk reads files written by other programs (link.json, survival.json,
//! gateway_state.json, the one session log) and must never fail a snapshot on
//! a half-written or odd file. These scanners find the FIRST occurrence of
//! `"key"` anywhere in the text, like the Oct 10 live binary did, and return
//! an empty value when it is absent or of another type. They are deliberately
//! not a JSON parser; the first-match rule is part of the desk's wire
//! behaviour (for example `name` in link.json is the laptop card's name).

/// Text after `"key"` and its colon, left-trimmed. `None` when absent.
fn after_key<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let i = src.find(&pat)?;
    let rest = &src[i + pat.len()..];
    let colon = rest.find(':')?;
    Some(rest[colon + 1..].trim_start())
}

/// Value of the first `"key": "<string>"`. JSON escapes are decoded
/// (`\n`, `\t`, `\uXXXX` including surrogate pairs). Empty when absent or
/// not a string.
pub fn json_string(src: &str, key: &str) -> String {
    let Some(rest) = after_key(src, key) else {
        return String::new();
    };
    let Some(body) = rest.strip_prefix('"') else {
        return String::new();
    };
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('u') => {
                    let hi = hex4(&mut chars);
                    match hi {
                        Some(hi) if (0xD800..0xDC00).contains(&hi) => {
                            let mut look = chars.clone();
                            let lo = if look.next() == Some('\\') && look.next() == Some('u') {
                                hex4(&mut look)
                            } else {
                                None
                            };
                            match lo {
                                Some(lo) if (0xDC00..0xE000).contains(&lo) => {
                                    chars = look;
                                    let cp = 0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                }
                                _ => out.push('\u{FFFD}'),
                            }
                        }
                        Some(cp) => out.push(char::from_u32(cp).unwrap_or('\u{FFFD}')),
                        None => out.push('\u{FFFD}'),
                    }
                }
                Some(other) => out.push(other),
                None => break,
            },
            c => out.push(c),
        }
    }
    out
}

fn hex4(chars: &mut std::str::Chars<'_>) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        v = v * 16 + chars.next()?.to_digit(16)?;
    }
    Some(v)
}

/// Leading unsigned integer of the first `"key": <number>`. 0 when absent.
/// A fraction is cut (`12.7` -> 12), a negative or non-number is 0.
pub fn json_number(src: &str, key: &str) -> u64 {
    let Some(rest) = after_key(src, key) else {
        return 0;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(0)
}

/// Number text (digits and dots) of the first `"key"`, or `—` when absent.
pub fn json_amount(src: &str, key: &str) -> String {
    let text: String = after_key(src, key)
        .unwrap_or("")
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if text.is_empty() {
        "—".to_string()
    } else {
        text
    }
}

/// `true` only when the first `"key"` holds the literal `true`.
pub fn flag(src: &str, key: &str) -> bool {
    after_key(src, key).is_some_and(|rest| rest.starts_with("true"))
}

/// Strings of the first `"key": [ ... ]` array, in order. Brackets and quotes
/// inside strings do not end the array. Non-string items are skipped.
pub fn json_string_array(src: &str, key: &str) -> Vec<String> {
    let Some(rest) = after_key(src, key) else {
        return Vec::new();
    };
    let Some(body) = rest.strip_prefix('[') else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut cur = String::new();
    let mut inside = false;
    let mut esc = false;
    let mut depth = 0usize;
    for c in body.chars() {
        if inside {
            if esc {
                cur.push(match c {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                if depth == 0 {
                    rows.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                inside = false;
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' => inside = true,
            '[' | '{' => depth += 1,
            ']' | '}' if depth > 0 => depth -= 1,
            ']' => break,
            _ => {}
        }
    }
    rows
}

/// Escape for a JSON string literal. Newlines become spaces and other
/// control characters are dropped, as the live desk did.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_first_match_and_escapes() {
        let src = r#"{"laptop":{"name":"Desk \"A\""},"name":"second"}"#;
        assert_eq!(json_string(src, "name"), "Desk \"A\"");
        assert_eq!(
            json_string(r#"{"t": "a\nb\tc\\d\/e"}"#, "t"),
            "a\nb\tc\\d/e"
        );
        assert_eq!(json_string(r#"{"t":"Caf\u00e9"}"#, "t"), "Café");
        assert_eq!(json_string(r#"{"t":"\ud83d\ude00!"}"#, "t"), "😀!");
        assert_eq!(json_string(r#"{"t":"\ud83dx"}"#, "t"), "\u{FFFD}x");
        assert_eq!(json_string(r#"{"t":"\uZZZZ"}"#, "t"), "\u{FFFD}ZZZ");
        assert_eq!(json_string(r#"{"t":3}"#, "t"), "");
        assert_eq!(json_string(r#"{"x":"y"}"#, "t"), "");
        assert_eq!(json_string(r#"{"t":"unterminated"#, "t"), "unterminated");
        assert_eq!(json_string("", "t"), "");
    }

    #[test]
    fn numbers_amounts_and_flags() {
        let src = r#"{"a": 42, "b":12.7, "c":-3, "d":"9", "e": true, "f":false}"#;
        assert_eq!(json_number(src, "a"), 42);
        assert_eq!(json_number(src, "b"), 12);
        assert_eq!(json_number(src, "c"), 0);
        assert_eq!(json_number(src, "d"), 0);
        assert_eq!(json_number(src, "zz"), 0);
        assert_eq!(json_amount(src, "b"), "12.7");
        assert_eq!(json_amount(src, "zz"), "—");
        assert_eq!(json_amount(src, "d"), "—");
        assert!(flag(src, "e"));
        assert!(!flag(src, "f"));
        assert!(!flag(src, "zz"));
        assert!(!flag(r#"{"france": "unconfigured"}"#, "france"));
    }

    #[test]
    fn string_array_respects_brackets_inside_strings() {
        let src = r#"{"events": ["12:00  [x] up", "b \"q\"", 3, ["nested"], {"k":"v"}, "c"], "z":["no"]}"#;
        assert_eq!(
            json_string_array(src, "events"),
            vec!["12:00  [x] up", "b \"q\"", "c"]
        );
        assert!(json_string_array(r#"{"events": "nope"}"#, "events").is_empty());
        assert!(json_string_array(r#"{"x":1}"#, "events").is_empty());
        assert!(json_string_array(r#"{"events": []}"#, "events").is_empty());
    }

    #[test]
    fn escape_round_trips_through_json_string() {
        for text in ["plain", "q\"uote", "back\\slash", "tab\there", "ünï"] {
            let doc = format!("{{\"k\":\"{}\"}}", escape(text));
            let expect: String = text.chars().filter(|c| !c.is_control()).collect();
            assert_eq!(json_string(&doc, "k"), expect);
        }
        assert_eq!(escape("a\nb\rc\u{1}"), "a b c");
    }
}
