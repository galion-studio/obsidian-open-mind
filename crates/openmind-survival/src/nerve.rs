// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! B25: guardian report as nerve event lines (one JSON object per line).
//! Std only, so JSON is written by hand with proper escaping.

use crate::ProbeResult;

/// Escape a string for a JSON string literal (without the quotes).
pub fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// `{"ts","source","kind":"probe","name","ok","detail"}` for one result.
pub fn nerve_line(source: &str, r: &ProbeResult) -> String {
    format!(
        "{{\"ts\":\"{}\",\"source\":\"{}\",\"kind\":\"probe\",\"name\":\"{}\",\"ok\":{},\"detail\":\"{}\"}}",
        json_escape(&r.ts),
        json_escape(source),
        json_escape(&r.name),
        r.ok,
        json_escape(&r.detail)
    )
}

/// Append nerve lines to a JSONL file (creates parent dirs).
pub fn append_nerve(path: &std::path::Path, source: &str, rs: &[ProbeResult]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let mut buf = String::new();
    for r in rs {
        buf.push_str(&nerve_line(source, r));
        buf.push('\n');
    }
    f.write_all(buf.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(json_escape("a\"b\\c\nd\u{1}"), "a\\\"b\\\\c\\nd\\u0001");
        let mut r = ProbeResult::new("one", false, "connect: \"refused\"");
        r.ts = "2026-10-18T21:00:00+02:00".into();
        assert_eq!(
            nerve_line("guardian", &r),
            "{\"ts\":\"2026-10-18T21:00:00+02:00\",\"source\":\"guardian\",\"kind\":\"probe\",\"name\":\"one\",\"ok\":false,\"detail\":\"connect: \\\"refused\\\"\"}"
        );
    }
}
