// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `safety` check: every `unsafe` block, `unsafe impl`, `unsafe trait` and
//! `unsafe extern` block carries a `// SAFETY:` comment, and every
//! `unsafe fn` documents its contract (`# Safety` doc section or a
//! `SAFETY` comment).
//!
//! The rule mirrors clippy's `undocumented_unsafe_blocks` and
//! `missing_safety_doc` (same accepted places for the comment) but is
//! std-only and runs over the whole tree in milliseconds, without a build.
//! Accepted places, nearest first:
//!
//! 1. the same line (`unsafe { f() } // SAFETY: ...`);
//! 2. the comment/attribute lines directly above the line holding `unsafe`;
//! 3. the comment lines above the start of the statement, when `unsafe`
//!    sits on a continuation line of a longer expression (up to
//!    [`MAX_STMT_LINES`] lines up).
//!
//! The word `SAFETY` matches case-sensitively (so `// SAFETY (all probes):`
//! counts); prose such as "for safety" does not. Code inside string
//! literals, char literals and comments is ignored, so the scanner can read
//! its own source.

/// How many code lines a statement may span above the `unsafe` line.
pub const MAX_STMT_LINES: usize = 6;

/// What follows the `unsafe` keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Block,
    Fn,
    Impl,
    Trait,
    Extern,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Block => "unsafe block",
            Kind::Fn => "unsafe fn",
            Kind::Impl => "unsafe impl",
            Kind::Trait => "unsafe trait",
            Kind::Extern => "unsafe extern block",
        }
    }
}

/// One `unsafe` site without the required comment (1-based line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub line: usize,
    pub kind: Kind,
}

/// Per line: the code with literals/comments blanked, and the comment text.
struct Lexed {
    code: String,
    comment: String,
}

/// Blank out string/char literals and comments, keep line structure.
fn lex(text: &str) -> Vec<Lexed> {
    let mut out = Vec::new();
    let mut in_block = 0usize; // nested /* */ depth
    let mut in_str: Option<usize> = None; // Some(hashes) for raw, usize::MAX for normal
    for line in text.lines() {
        let b: Vec<char> = line.chars().collect();
        let mut code = String::with_capacity(line.len());
        let mut comment = String::new();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if in_block > 0 {
                if c == '*' && b.get(i + 1) == Some(&'/') {
                    in_block -= 1;
                    i += 2;
                } else if c == '/' && b.get(i + 1) == Some(&'*') {
                    in_block += 1;
                    i += 2;
                } else {
                    comment.push(c);
                    i += 1;
                }
                continue;
            }
            if let Some(h) = in_str {
                if h == usize::MAX {
                    if c == '\\' {
                        i += 2;
                        continue;
                    }
                    if c == '"' {
                        in_str = None;
                        code.push('"');
                    }
                    i += 1;
                } else {
                    let closes = c == '"' && (0..h).all(|k| b.get(i + 1 + k) == Some(&'#'));
                    if closes {
                        in_str = None;
                        code.push('"');
                        i += 1 + h;
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            match c {
                '/' if b.get(i + 1) == Some(&'/') => {
                    comment.extend(&b[i..]);
                    break;
                }
                '/' if b.get(i + 1) == Some(&'*') => {
                    in_block = 1;
                    i += 2;
                }
                '"' => {
                    in_str = Some(usize::MAX);
                    code.push('"');
                    i += 1;
                }
                'r' if !prev_ident(&b, i) && raw_start(&b, i + 1).is_some() => {
                    let h = raw_start(&b, i + 1).unwrap_or(0);
                    in_str = Some(h);
                    code.push('"');
                    i += 2 + h;
                }
                '\'' => {
                    // char literal ('x', '\n', '\'', '\u{..}') vs lifetime ('a)
                    if b.get(i + 1) == Some(&'\\') {
                        let mut j = i + 2;
                        while j < b.len() && b[j] != '\'' {
                            j += 1;
                        }
                        // skip the escaped char itself when it is a quote
                        if j == i + 2 && b.get(j) == Some(&'\'') {
                            j += 1;
                            while j < b.len() && b[j] != '\'' {
                                j += 1;
                            }
                        }
                        code.push_str("' '");
                        i = j + 1;
                    } else if b.get(i + 2) == Some(&'\'') {
                        code.push_str("' '");
                        i += 3;
                    } else {
                        code.push('\'');
                        i += 1;
                    }
                }
                _ => {
                    code.push(c);
                    i += 1;
                }
            }
        }
        if in_block > 0 && comment.is_empty() {
            comment.push(' ');
        }
        out.push(Lexed { code, comment });
    }
    out
}

fn prev_ident(b: &[char], i: usize) -> bool {
    i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_')
}

/// `#*"` after an `r` (also `br`): number of hashes.
fn raw_start(b: &[char], mut j: usize) -> Option<usize> {
    let mut h = 0;
    while b.get(j) == Some(&'#') {
        h += 1;
        j += 1;
    }
    (b.get(j) == Some(&'"')).then_some(h)
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Positions (line index, byte offset) of the word `unsafe` in code.
fn unsafe_words(lx: &[Lexed]) -> Vec<(usize, usize)> {
    const W: &str = concat!("un", "safe");
    let mut v = Vec::new();
    for (n, l) in lx.iter().enumerate() {
        let s = &l.code;
        let mut from = 0;
        while let Some(off) = s[from..].find(W) {
            let at = from + off;
            let before = s[..at].chars().next_back().is_some_and(is_ident);
            let after = s[at + W.len()..].chars().next().is_some_and(is_ident);
            if !before && !after {
                v.push((n, at + W.len()));
            }
            from = at + W.len();
        }
    }
    v
}

/// The next few code tokens after (line, offset), across lines.
fn next_tokens(lx: &[Lexed], line: usize, off: usize, want: usize) -> Vec<String> {
    let mut toks = Vec::new();
    let mut n = line;
    let mut start = off;
    while n < lx.len() && n < line + 4 && toks.len() < want {
        let s = &lx[n].code[start..];
        let mut cur = String::new();
        for c in s.chars() {
            if is_ident(c) {
                cur.push(c);
                continue;
            }
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            if !c.is_whitespace() {
                toks.push(c.to_string());
            }
            if toks.len() >= want {
                break;
            }
        }
        if !cur.is_empty() {
            toks.push(cur);
        }
        n += 1;
        start = 0;
    }
    toks.truncate(want);
    toks
}

fn classify(t: &[String]) -> Option<Kind> {
    let first = t.first()?.as_str();
    match first {
        "{" => Some(Kind::Block),
        "impl" => Some(Kind::Impl),
        "trait" => Some(Kind::Trait),
        // `unsafe fn name` is an item; `unsafe fn(` is a pointer type.
        "fn" => (t.get(1).map(String::as_str) != Some("(")).then_some(Kind::Fn),
        "extern" => {
            // unsafe extern "C" { .. }  vs  unsafe extern "C" fn ..
            let rest: Vec<&str> = t[1..].iter().map(String::as_str).collect();
            let after_abi = if rest.first() == Some(&"\"") {
                rest.iter().skip(1).position(|s| *s == "\"").map(|p| p + 2)
            } else {
                Some(0)
            };
            match after_abi.and_then(|k| rest.get(k)) {
                Some(&"{") => Some(Kind::Extern),
                Some(&"fn") => {
                    let k = after_abi.unwrap_or(0);
                    (rest.get(k + 1) != Some(&"(")).then_some(Kind::Fn)
                }
                _ => None,
            }
        }
        // `#[unsafe(no_mangle)]`, `unsafe<..>` etc.: not a site.
        _ => None,
    }
}

fn has_word(s: &str) -> bool {
    s.contains("SAFETY")
}

fn comment_only(l: &Lexed) -> bool {
    l.code.trim().is_empty() && !l.comment.trim().is_empty()
}

fn attr_only(l: &Lexed) -> bool {
    let c = l.code.trim();
    c.starts_with("#[") || c.starts_with("#![")
}

/// Comment block directly above `line` (comments and attributes, no blanks).
fn block_above(lx: &[Lexed], line: usize, fn_doc: bool) -> bool {
    let mut n = line;
    while n > 0 {
        n -= 1;
        let l = &lx[n];
        if comment_only(l) {
            if has_word(&l.comment) || (fn_doc && l.comment.contains("# Safety")) {
                return true;
            }
        } else if attr_only(l) {
            if has_word(&l.comment) {
                return true;
            }
        } else {
            return false;
        }
    }
    false
}

/// Does code line `n` end a statement or open/close a block?
fn ends_stmt(l: &Lexed) -> bool {
    let c = l.code.trim_end();
    c.is_empty() || c.ends_with(';') || c.ends_with('{') || c.ends_with('}')
}

fn documented(lx: &[Lexed], line: usize, kind: Kind) -> bool {
    if has_word(&lx[line].comment) {
        return true;
    }
    let fn_doc = kind == Kind::Fn;
    if block_above(lx, line, fn_doc) {
        return true;
    }
    if kind != Kind::Block {
        return false;
    }
    // `unsafe` on a continuation line: climb to the statement start.
    // The `unsafe` line itself opened with code before it (e.g. `let x =`)
    // counts as the start.
    let mut n = line;
    for _ in 0..MAX_STMT_LINES {
        if n == 0 {
            return false;
        }
        let prev = &lx[n - 1];
        if comment_only(prev) || attr_only(prev) || ends_stmt(prev) {
            return false;
        }
        n -= 1;
        if has_word(&lx[n].comment) || block_above(lx, n, false) {
            return true;
        }
    }
    false
}

/// Every undocumented `unsafe` site in a Rust source text.
pub fn missing(text: &str, allow_marker: &str) -> Vec<Missing> {
    let lx = lex(text);
    let raw: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (line, off) in unsafe_words(&lx) {
        if raw.get(line).is_some_and(|l| l.contains(allow_marker)) {
            continue;
        }
        let toks = next_tokens(&lx, line, off, 8);
        let Some(kind) = classify(&toks) else {
            continue;
        };
        if !documented(&lx, line, kind) {
            out.push(Missing {
                line: line + 1,
                kind,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = "repo-check: allow";
    // Build the keyword at runtime-ish so this file reads clean to itself.
    fn u() -> &'static str {
        concat!("un", "safe")
    }

    fn lines(src: &str) -> Vec<usize> {
        missing(&src.replace("UNS", u()), M)
            .into_iter()
            .map(|m| m.line)
            .collect()
    }

    #[test]
    fn plain_block_needs_comment() {
        assert_eq!(lines("fn a() {\n    UNS { f() };\n}\n"), vec![2]);
        assert!(
            lines("fn a() {\n    // SAFETY: f has no preconditions.\n    UNS { f() };\n}\n")
                .is_empty()
        );
        assert!(lines("fn a() {\n    UNS { f() }; // SAFETY: none\n}\n").is_empty());
    }

    #[test]
    fn blank_line_breaks_the_link() {
        assert_eq!(lines("// SAFETY: x\n\nUNS { f() };\n"), vec![3]);
    }

    #[test]
    fn attributes_between_comment_and_block_are_fine() {
        assert!(lines("// SAFETY: x\n#[cfg(unix)]\nUNS {\n f()\n}\n").is_empty());
    }

    #[test]
    fn statement_continuation() {
        let ok = "// SAFETY: x\nlet r = call(\n    a,\n    UNS { f() },\n);\n";
        assert!(lines(ok).is_empty());
        let bad = "foo();\nlet r = call(\n    a,\n    UNS { f() },\n);\n";
        assert_eq!(lines(bad), vec![4]);
    }

    #[test]
    fn nested_block_in_if_needs_own_comment() {
        // A comment above the `if` does not cover a block in its body.
        assert_eq!(
            lines("// SAFETY: x\nif c {\n    UNS { f() };\n}\n"),
            vec![3]
        );
    }

    #[test]
    fn unsafe_fn_takes_safety_doc() {
        assert_eq!(lines("pub UNS fn g() {}\n"), vec![1]);
        assert!(
            lines("/// Does g.\n///\n/// # Safety\n/// x must be valid.\npub UNS fn g() {}\n")
                .is_empty()
        );
        assert!(lines("// SAFETY: caller upholds x.\nUNS fn g() {}\n").is_empty());
    }

    #[test]
    fn impl_trait_extern() {
        assert_eq!(lines("UNS impl Send for X {}\n"), vec![1]);
        assert!(
            lines("// SAFETY: X owns no thread-bound data.\nUNS impl Send for X {}\n").is_empty()
        );
        assert_eq!(lines("pub UNS trait T {}\n"), vec![1]);
        assert_eq!(lines("UNS extern \"C\" {\n    fn f();\n}\n"), vec![1]);
    }

    #[test]
    fn non_sites_are_ignored() {
        // fn-pointer types, attributes, literals and comments
        assert!(lines("type F = UNS fn(i32);\n").is_empty());
        assert!(lines("type F = UNS extern \"C\" fn(i32);\n").is_empty());
        assert!(lines("#[UNS(no_mangle)]\npub extern \"C\" fn f() {}\n").is_empty());
        assert!(lines("let s = \"UNS { x }\";\n").is_empty());
        assert!(lines("let s = r#\"UNS { \"x\" }\"#;\n").is_empty());
        assert!(lines("// UNS { x }\n/* UNS { y } */\n").is_empty());
        assert!(lines("let c = '\"'; let d = UNS_ok;\n").is_empty());
        assert!(lines("fn f<'a>(x: &'a str) { let not_UNS = 1; }\n").is_empty());
    }

    #[test]
    fn block_on_next_line_and_allow_marker() {
        assert_eq!(lines("let x = UNS\n{\n f()\n};\n"), vec![1]);
        assert!(lines(&format!("UNS {{ f() }}; // {M}\n")).is_empty());
    }

    #[test]
    fn prose_safety_does_not_count() {
        assert_eq!(lines("// for safety reasons\nUNS { f() };\n"), vec![2]);
        assert!(lines("// SAFETY (all probes): fine\nUNS { f() };\n").is_empty());
    }

    #[test]
    fn multiline_string_hides_unsafe() {
        assert!(lines("let s = \"a\nUNS { b }\nc\";\n").is_empty());
    }
}
