// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! A conservative POSIX-shell lexer for the deny list (layer S1/S2/S5).
//!
//! It does not try to run or fully parse shell. It removes quoting and
//! escapes the way the shell would (so `r""m`, `\rm`, `'rm'` all become
//! `rm`), splits on every control operator (`; & && || | |&` newline), and
//! REFUSES everything whose meaning depends on run-time state: command and
//! process substitution, parameter / arithmetic expansion, ANSI-C quoting,
//! here-documents, subshells and brace expansion. What is left is a list of
//! simple commands whose words are exactly what the program will receive.

use super::Deny;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Word {
    /// The word after quote and escape removal.
    pub text: String,
    /// Unquoted glob metacharacter (`* ? [`) present.
    pub glob: bool,
    /// Any part of the word was quoted or escaped.
    pub quoted: bool,
    /// Unquoted leading `~`.
    pub tilde: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RedirKind {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Redir {
    pub kind: RedirKind,
    pub target: Word,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Segment {
    pub words: Vec<Word>,
    pub redirs: Vec<Redir>,
}

fn deny(rule: &str, why: impl Into<String>) -> Deny {
    Deny {
        rule: rule.to_string(),
        reason: why.into(),
    }
}

/// Unicode format / invisible characters and look-alike ASCII (full-width
/// forms): never legitimate in a command, a classic filter-evasion trick.
pub fn suspicious_char(c: char) -> bool {
    let u = c as u32;
    matches!(u,
        0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180B..=0x180F
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0x3164 | 0xFE00..=0xFE0F
        | 0xFEFF | 0xFF01..=0xFF5E | 0xFFA0 | 0xFFF0..=0xFFFB | 0xE0000..=0xE0FFF
        | 0x1D400..=0x1D7FF)
        || (c.is_control() && c != '\n' && c != '\t' && c != '\r')
}

struct Lx {
    segs: Vec<Segment>,
    cur: Segment,
    word: Option<Word>,
    pending: Option<RedirKind>,
    /// next word is the target of a `>&` / `<&` (a number means fd dup)
    pending_dup: bool,
}

impl Lx {
    fn w(&mut self) -> &mut Word {
        self.word.get_or_insert_with(Word::default)
    }
    fn end_word(&mut self) {
        if let Some(w) = self.word.take() {
            if let Some(k) = self.pending.take() {
                let dup = std::mem::take(&mut self.pending_dup);
                if dup && (w.text.chars().all(|c| c.is_ascii_digit()) || w.text == "-") {
                    return;
                }
                self.cur.redirs.push(Redir { kind: k, target: w });
            } else {
                self.cur.words.push(w);
            }
        }
    }
    fn end_seg(&mut self) -> Result<(), Deny> {
        self.end_word();
        if self.pending.is_some() {
            return Err(deny("S1-syntax", "redirection with no target"));
        }
        let s = std::mem::take(&mut self.cur);
        if !s.words.is_empty() || !s.redirs.is_empty() {
            self.segs.push(s);
        }
        Ok(())
    }
}

fn expansion_after_dollar(n: Option<char>) -> Option<&'static str> {
    match n {
        Some('(') => Some("command substitution $( ) / arithmetic $(( ))"),
        Some('{') => Some("parameter expansion ${...}"),
        Some('[') => Some("arithmetic $[...]"),
        Some('\'') => Some("ANSI-C quoting $'...' (hides \\x escapes)"),
        Some('"') => Some("locale quoting $\"...\""),
        Some(c) if c.is_ascii_alphanumeric() || "_@*#?$!-".contains(c) => {
            Some("variable expansion $NAME")
        }
        _ => None,
    }
}

pub fn lex(src: &str) -> Result<Vec<Segment>, Deny> {
    let s: Vec<char> = src.chars().collect();
    let mut lx = Lx {
        segs: Vec::new(),
        cur: Segment::default(),
        word: None,
        pending: None,
        pending_dup: false,
    };
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        let next = s.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\r' => {
                lx.end_word();
                i += 1;
            }
            '\n' | ';' => {
                lx.end_seg()?;
                i += 1;
            }
            '#' if lx.word.is_none() => {
                // comment to end of line
                while i < s.len() && s[i] != '\n' {
                    i += 1;
                }
            }
            '&' | '|' => {
                if c == '&' && next == Some('>') {
                    // &> / &>> : stdout+stderr to a file
                    lx.end_word();
                    i += 2;
                    if s.get(i) == Some(&'>') {
                        i += 1;
                    }
                    lx.pending = Some(RedirKind::Write);
                    continue;
                }
                lx.end_seg()?;
                i += 1;
                if next == Some(c) || (c == '|' && next == Some('&')) {
                    i += 1;
                }
            }
            '(' | ')' => {
                return Err(deny(
                    "S1-subshell",
                    "subshells, groups and function bodies ( ) are not allowed",
                ))
            }
            '`' => return Err(deny("S1-subst", "command substitution with backticks")),
            '<' | '>' => {
                // a pure-digit word just before is an fd number
                if let Some(w) = &lx.word {
                    if !w.quoted && !w.text.is_empty() && w.text.chars().all(|d| d.is_ascii_digit())
                    {
                        lx.word = None;
                    } else {
                        lx.end_word();
                    }
                }
                if lx.pending.is_some() {
                    return Err(deny("S1-syntax", "two redirections in a row"));
                }
                if next == Some('(') {
                    return Err(deny("S1-subst", "process substitution <( ) / >( )"));
                }
                if c == '<' && next == Some('<') {
                    return Err(deny(
                        "S1-heredoc",
                        "here-documents and here-strings (<< / <<<) hide input",
                    ));
                }
                let kind = if c == '<' && next != Some('>') {
                    RedirKind::Read
                } else {
                    RedirKind::Write
                };
                i += 1;
                if let Some(n) = s.get(i).copied() {
                    if (c == '>' && (n == '>' || n == '|')) || (c == '<' && n == '>') {
                        i += 1;
                    } else if n == '&' {
                        i += 1;
                        lx.pending_dup = true;
                    }
                }
                lx.pending = Some(kind);
            }
            '\\' => {
                match next {
                    Some('\n') => {} // line continuation
                    Some(n) => {
                        let w = lx.w();
                        w.text.push(n);
                        w.quoted = true;
                    }
                    None => return Err(deny("S1-syntax", "trailing backslash")),
                }
                i += 2;
            }
            '\'' => {
                let w = lx.w();
                w.quoted = true;
                i += 1;
                loop {
                    match s.get(i) {
                        None => return Err(deny("S1-syntax", "unterminated single quote")),
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            lx.w().text.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '"' => {
                lx.w().quoted = true;
                i += 1;
                loop {
                    match s.get(i).copied() {
                        None => return Err(deny("S1-syntax", "unterminated double quote")),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('`') => {
                            return Err(deny("S1-subst", "command substitution with backticks"))
                        }
                        Some('$') => {
                            if let Some(why) = expansion_after_dollar(s.get(i + 1).copied()) {
                                return Err(deny(
                                    "S5-expand",
                                    format!("{why} is resolved at run time"),
                                ));
                            }
                            lx.w().text.push('$');
                            i += 1;
                        }
                        Some('\\') => {
                            match s.get(i + 1).copied() {
                                Some('\n') => {}
                                Some(n @ ('$' | '`' | '"' | '\\')) => lx.w().text.push(n),
                                Some(n) => {
                                    lx.w().text.push('\\');
                                    lx.w().text.push(n);
                                }
                                None => return Err(deny("S1-syntax", "unterminated double quote")),
                            }
                            i += 2;
                        }
                        Some(ch) => {
                            lx.w().text.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '$' => {
                if let Some(why) = expansion_after_dollar(next) {
                    let rule = if matches!(next, Some('(') | Some('[')) {
                        "S1-subst"
                    } else if matches!(next, Some('\'') | Some('"')) {
                        "S3-ansi-c"
                    } else {
                        "S5-expand"
                    };
                    return Err(deny(rule, format!("{why} is resolved at run time")));
                }
                lx.w().text.push('$');
                i += 1;
            }
            _ => {
                if !c.is_ascii() {
                    return Err(deny(
                        "S2-unicode",
                        "non-ASCII character outside quotes (look-alike / invisible command text)",
                    ));
                }
                let starting = lx.word.is_none();
                let w = lx.w();
                if c == '~' && starting {
                    w.tilde = true;
                }
                if matches!(c, '*' | '?' | '[') {
                    w.glob = true;
                }
                if c == '{' {
                    // brace expansion: {a,b} or {1..9} in an unquoted word
                    let rest: String = s[i..]
                        .iter()
                        .take_while(|&&x| !x.is_whitespace() && !";&|<>".contains(x))
                        .collect();
                    if let Some(end) = rest.find('}') {
                        let inner = &rest[1..end];
                        if inner.contains(',') || inner.contains("..") {
                            return Err(deny("S2-brace", "brace expansion {a,b} / {1..9}"));
                        }
                    }
                }
                w.text.push(c);
                i += 1;
            }
        }
    }
    lx.end_seg()?;
    Ok(lx.segs)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn words(s: &str) -> Vec<Vec<String>> {
        lex(s)
            .unwrap()
            .into_iter()
            .map(|g| g.words.into_iter().map(|w| w.text).collect())
            .collect()
    }
    #[test]
    fn dequotes_and_splits() {
        assert_eq!(words(r#"r""m -rf "/""#), vec![vec!["rm", "-rf", "/"]]);
        assert_eq!(
            words(r"\rm a;b&&c||d|e&f"),
            vec![
                vec!["rm", "a"],
                vec!["b"],
                vec!["c"],
                vec!["d"],
                vec!["e"],
                vec!["f"]
            ]
        );
        assert_eq!(words("r\\\nm x"), vec![vec!["rm", "x"]]);
        assert_eq!(words("ls # rm -rf /"), vec![vec!["ls"]]);
        let g = lex("echo hi > out.txt 2>&1").unwrap();
        assert_eq!(g[0].redirs.len(), 1);
        assert_eq!(g[0].redirs[0].target.text, "out.txt");
    }
    #[test]
    fn refuses_runtime_forms() {
        for bad in [
            "$(id)",
            "`id`",
            "${HOME}",
            "$HOME",
            "<(id)",
            "cat <<EOF",
            "$'\\x72m'",
            "(id)",
            "{a,b}",
            "echo \"$(id)\"",
        ] {
            assert!(lex(bad).is_err(), "{bad}");
        }
        assert!(lex("echo '$HOME'").is_ok());
        assert!(lex("rg 'foo$'").is_ok());
    }
}
