// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Port of `scripts/heal-dry.py`: match an error text against the guardian
//! healer patterns (same order as `AutoHealer._initialize_fix_patterns`) and
//! return a dry-run plan. Never edits. Hand-written matchers, no regex crate:
//! every upstream pattern stays inside one line, so matching is per line on a
//! lowercased copy (the Python used `re.IGNORECASE`).

/// Words that make the healer refuse outright (heal-dry.py `PROTECTED`).
pub const PROTECTED: &[&str] = &["gateway", "whisper", "vision", "keep.sh", "restart"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryPlan {
    /// Pattern name, or `none`.
    pub matched: &'static str,
    /// Always false: this is a plan, not an edit.
    pub edit: bool,
    /// `protected`, `not auto-fixable`, or empty.
    pub refuse: &'static str,
}

impl DryPlan {
    /// Same JSON shape and spacing as heal-dry.py prints.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"match\": \"{}\", \"edit\": false, \"refuse\": \"{}\"}}",
            self.matched, self.refuse
        )
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_quote(c: char) -> bool {
    c == '\'' || c == '"'
}

/// `prefix(.+)`: prefix followed by at least one more char on the line.
fn prefix_any(line: &str, prefix: &str) -> bool {
    line.find(prefix)
        .is_some_and(|i| line.len() > i + prefix.len())
}

/// `prefix['"](\w+)['"]` (+ optional tail requirement after the close quote).
fn prefix_quoted_word(line: &str, prefix: &str, tail: Option<&str>) -> bool {
    let mut from = 0;
    while let Some(off) = line[from..].find(prefix) {
        let start = from + off + prefix.len();
        let mut it = line[start..].char_indices();
        if let Some((_, q)) = it.next() {
            if is_quote(q) {
                let mut n = 0;
                for (ix, c) in it {
                    if is_word(c) {
                        n += 1;
                        continue;
                    }
                    if is_quote(c) && n > 0 {
                        let rest = &line[start + ix + c.len_utf8()..];
                        if tail.is_none_or(|t| rest.contains(t)) {
                            return true;
                        }
                    }
                    break;
                }
            }
        }
        from = from + off + 1;
        while !line.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// `prefix['"](.+)['"]`: open quote, at least one char, a later quote.
fn prefix_quoted_any(line: &str, prefix: &str) -> bool {
    let mut from = 0;
    while let Some(off) = line[from..].find(prefix) {
        let start = from + off + prefix.len();
        let rest = &line[start..];
        if let Some(q) = rest.chars().next() {
            if is_quote(q) {
                let body = &rest[q.len_utf8()..];
                let mut chars = body.char_indices();
                if chars.next().is_some() && chars.any(|(ix, c)| ix > 0 && is_quote(c)) {
                    return true;
                }
            }
        }
        from = from + off + 1;
        while !line.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// `(a1|a2).*(b1|b2|...)` on one line.
fn any_then_any(line: &str, firsts: &[&str], seconds: &[&str]) -> bool {
    firsts.iter().any(|a| {
        line.find(a)
            .is_some_and(|i| seconds.iter().any(|b| line[i + a.len()..].contains(b)))
    })
}

type Matcher = fn(&str) -> bool;

const PATTERNS: &[(&str, Matcher, bool)] = &[
    (
        "ModuleNotFoundError",
        |l| prefix_quoted_word(l, "modulenotfounderror: no module named ", None),
        false,
    ),
    (
        "ImportError_no_module",
        |l| prefix_quoted_word(l, "importerror: no module named ", None),
        false,
    ),
    (
        "SyntaxError_general",
        |l| prefix_any(l, "syntaxerror: "),
        false,
    ),
    (
        "IndentationError",
        |l| prefix_any(l, "indentationerror: "),
        false,
    ),
    (
        "FileNotFoundError",
        |l| {
            prefix_quoted_any(
                l,
                "filenotfounderror: [errno 2] no such file or directory: ",
            )
        },
        false,
    ),
    (
        "PermissionError",
        |l| prefix_quoted_any(l, "permissionerror: [errno 13] permission denied: "),
        false,
    ),
    (
        "init_file_missing",
        |l| prefix_quoted_word(l, "no module named ", Some("__init__")),
        false,
    ),
    (
        "TypeError_argument",
        |l| {
            let p = "typeerror: ";
            l.find(p).is_some_and(|i| {
                l[i + p.len()..]
                    .match_indices(" argument")
                    .any(|(j, _)| j >= 1)
            })
        },
        false,
    ),
    (
        "JSONDecodeError",
        |l| prefix_any(l, "jsondecodeerror: "),
        false,
    ),
    (
        "DatabaseConnectionError",
        |l| {
            any_then_any(
                l,
                &["operationalerror", "connectionrefusederror"],
                &["database", "postgres", "mysql", "sqlite"],
            )
        },
        true,
    ),
    (
        "RedisConnectionError",
        |l| any_then_any(l, &["connectionerror", "rediserror"], &["redis"]),
        true,
    ),
];

/// heal-dry.py `main()` as a pure function.
pub fn dry_plan(text: &str) -> DryPlan {
    let low = text.to_lowercase();
    let mut matched = "none";
    let mut flagged = false;
    'outer: for (name, m, f) in PATTERNS {
        for line in low.lines() {
            if m(line) {
                matched = name;
                flagged = *f;
                break 'outer;
            }
        }
    }
    let refuse = if PROTECTED.iter().any(|t| low.contains(t)) {
        "protected"
    } else if flagged {
        "not auto-fixable"
    } else {
        ""
    };
    DryPlan {
        matched,
        edit: false,
        refuse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases = [
            (
                "ModuleNotFoundError: No module named 'psutil'",
                "ModuleNotFoundError",
                "",
            ),
            (
                "importerror: no module named \"yaml\"",
                "ImportError_no_module",
                "",
            ),
            ("SyntaxError: invalid syntax", "SyntaxError_general", ""),
            (
                "IndentationError: unexpected indent",
                "IndentationError",
                "",
            ),
            (
                "FileNotFoundError: [Errno 2] No such file or directory: '/x/y'",
                "FileNotFoundError",
                "",
            ),
            (
                "PermissionError: [Errno 13] Permission denied: '/root/a'",
                "PermissionError",
                "",
            ),
            (
                "No module named 'pkg' because __init__ missing",
                "init_file_missing",
                "",
            ),
            (
                "TypeError: f() missing 1 required positional argument",
                "TypeError_argument",
                "",
            ),
            (
                "json.decoder.JSONDecodeError: Expecting value",
                "JSONDecodeError",
                "",
            ),
            (
                "sqlite3.OperationalError: unable to open database file",
                "DatabaseConnectionError",
                "not auto-fixable",
            ),
            (
                "ConnectionError to redis:6379",
                "RedisConnectionError",
                "not auto-fixable",
            ),
            ("all good", "none", ""),
            (
                "SyntaxError: in gateway config",
                "SyntaxError_general",
                "protected",
            ),
            ("please restart it", "none", "protected"),
            ("SyntaxError: ", "none", ""),
            ("ModuleNotFoundError: No module named ''", "none", ""),
            (
                "FileNotFoundError: [Errno 2] No such file or directory: ''",
                "none",
                "",
            ),
        ];
        for (text, m, r) in cases {
            let p = dry_plan(text);
            assert_eq!(p.matched, m, "{text}");
            assert_eq!(p.refuse, r, "{text}");
            assert!(!p.edit);
        }
    }

    #[test]
    fn order_and_json() {
        // Module pattern wins over init_file_missing (earlier in the list).
        let p = dry_plan("ModuleNotFoundError: No module named 'a' __init__");
        assert_eq!(p.matched, "ModuleNotFoundError");
        assert_eq!(
            dry_plan("x").to_json(),
            "{\"match\": \"none\", \"edit\": false, \"refuse\": \"\"}"
        );
        // Multi-line: patterns do not cross lines.
        assert_eq!(dry_plan("TypeError: \n argument").matched, "none");
    }
}
