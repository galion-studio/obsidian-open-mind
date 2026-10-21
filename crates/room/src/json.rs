// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The page as JSON: `{"objects":[{"kind","name","path","note"}, ...]}`.
//! Hand-written to keep the crate dependency-free.

use crate::index::Obj;
use std::fmt::Write as _;

/// Escapes text for a JSON string body. Quotes and backslashes are escaped,
/// newlines become spaces, and other control characters are dropped, so the
/// output is always valid JSON on one line.
#[must_use]
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Serialises the page.
#[must_use]
pub fn to_json(rows: &[Obj]) -> String {
    let mut out = String::from("{\"objects\":[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"kind\":\"{}\",\"name\":\"{}\",\"path\":\"{}\",\"note\":\"{}\"}}",
            esc(row.kind),
            esc(&row.name),
            esc(&row.path),
            esc(&row.note)
        );
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::{esc, to_json};
    use crate::index::Obj;

    #[test]
    fn escaping() {
        assert_eq!(esc("plain"), "plain");
        assert_eq!(esc("a\"b"), "a\\\"b");
        assert_eq!(esc("a\\b"), "a\\\\b");
        assert_eq!(esc("a\nb\rc"), "a b c");
        assert_eq!(esc("a\tb\u{0}c\u{7f}"), "abc");
        assert_eq!(esc("\u{17e}upa \u{1f600}"), "\u{17e}upa \u{1f600}");
    }

    #[test]
    fn page_shape() {
        assert_eq!(to_json(&[]), "{\"objects\":[]}");
        let rows = [
            Obj {
                kind: "doc",
                name: "a\"b".into(),
                path: "/x/a.md".into(),
                note: "line\none".into(),
            },
            Obj {
                kind: "live",
                name: "room".into(),
                path: "room".into(),
                note: String::new(),
            },
        ];
        assert_eq!(
            to_json(&rows),
            "{\"objects\":[{\"kind\":\"doc\",\"name\":\"a\\\"b\",\"path\":\"/x/a.md\",\"note\":\"line one\"},{\"kind\":\"live\",\"name\":\"room\",\"path\":\"room\",\"note\":\"\"}]}"
        );
    }
}
