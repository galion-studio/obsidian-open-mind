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
//!
//! Script mode ([`lex_opts`] with `script: true`, used only for a script file
//! that lives in the workspace and runs inside the terminal sandbox, behind
//! `HERMES_RS_GUARD_SCRIPT_MODE=1`) additionally accepts the ordinary script
//! forms: `$NAME` / `${NAME}` / simple `${NAME:-x}` `${NAME%x}` forms (the word
//! is marked [`Word::dynamic`]), `$( ... )` command substitution (the inner
//! text is returned in [`Segment::subs`] so the caller checks it with the same
//! deny list), `$(( ... ))` arithmetic over plain characters, here-documents
//! (a body with an unquoted delimiter may not contain `$(` or backticks),
//! here-strings, and `name()` function headers. Backticks, process
//! substitution, subshells, ANSI-C quoting, indirect `${!x}` and brace
//! expansion stay refused in every mode.

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
    /// Script mode only: part of the word is a run-time expansion.
    pub dynamic: bool,
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
    /// Script mode only: the inner text of every `$( ... )` in this segment.
    pub subs: Vec<String>,
}

/// Lexer options.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LexOpts {
    /// Accept the script forms listed in the module docs.
    pub script: bool,
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

/// A pending here-document on the current line.
struct HereDoc {
    delim: String,
    quoted: bool,
    strip_tabs: bool,
}

struct Lx {
    segs: Vec<Segment>,
    cur: Segment,
    word: Option<Word>,
    pending: Option<RedirKind>,
    /// next word is the target of a `>&` / `<&` (a number means fd dup)
    pending_dup: bool,
    /// script mode: the next word is here-string data, not an argument
    data_next: bool,
    heredocs: Vec<HereDoc>,
    /// script mode (a lone `[` / `[[` is the test command, not a glob)
    script: bool,
}

impl Lx {
    fn w(&mut self) -> &mut Word {
        self.word.get_or_insert_with(Word::default)
    }
    fn end_word(&mut self) {
        if let Some(mut w) = self.word.take() {
            if self.script && !w.quoted && (w.text == "[" || w.text == "[[") {
                w.glob = false;
            }
            if std::mem::take(&mut self.data_next) {
                return;
            }
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
        if self.data_next {
            return Err(deny("S1-syntax", "here-string with no data"));
        }
        let s = std::mem::take(&mut self.cur);
        if !s.words.is_empty() || !s.redirs.is_empty() || !s.subs.is_empty() {
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

fn is_ident(t: &str) -> bool {
    let mut c = t.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// The index just past the `)` that closes the `(` at `open`, skipping quoted
/// text and escapes; `None` if unbalanced.
fn matching_paren(s: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < s.len() {
        match s[i] {
            '\\' => i += 1,
            '\'' => {
                i += 1;
                while i < s.len() && s[i] != '\'' {
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < s.len() && s[i] != '"' {
                    if s[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Script mode: one `$` expansion starting at `i` (`s[i] == '$'`). Appends a
/// placeholder to the current word, marks it dynamic, records `$( )` inner
/// text, and returns the index after the expansion. `Ok(None)` = a literal `$`.
fn script_dollar(s: &[char], i: usize, lx: &mut Lx, in_dq: bool) -> Result<Option<usize>, Deny> {
    let next = s.get(i + 1).copied();
    match next {
        Some('(') if s.get(i + 2) == Some(&'(') => {
            let end =
                matching_paren(s, i + 1).ok_or_else(|| deny("S1-syntax", "unterminated $(( ))"))?;
            let inner: String = s[i + 3..end.saturating_sub(2).max(i + 3)].iter().collect();
            if s.get(end - 2) != Some(&')')
                || !inner
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || " \t_+-*/%<>=!&|?:,()^~$".contains(c))
                || inner.contains("$(")
            {
                return Err(deny(
                    "S1-subst",
                    "arithmetic $(( )) with anything but plain names, numbers and operators",
                ));
            }
            let w = lx.w();
            w.text.push_str("$_");
            w.dynamic = true;
            Ok(Some(end))
        }
        Some('(') => {
            let end = matching_paren(s, i + 1)
                .ok_or_else(|| deny("S1-syntax", "unterminated command substitution $( )"))?;
            let inner: String = s[i + 2..end - 1].iter().collect();
            lx.cur.subs.push(inner);
            let w = lx.w();
            w.text.push_str("$_");
            w.dynamic = true;
            Ok(Some(end))
        }
        Some('{') => {
            let close = s[i + 2..]
                .iter()
                .position(|&c| c == '}')
                .map(|p| i + 2 + p)
                .ok_or_else(|| deny("S1-syntax", "unterminated ${...}"))?;
            let inner: String = s[i + 2..close].iter().collect();
            let name_end = inner
                .char_indices()
                .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
                .map(|(p, _)| p)
                .unwrap_or(inner.len());
            let (name, op) = inner.split_at(name_end);
            let name_ok = is_ident(name)
                || (name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()))
                || (name.is_empty() && op.starts_with('#') && is_ident(&op[1..]));
            let op_ok = op.is_empty()
                || (name.is_empty() && op.starts_with('#'))
                || ([":-", ":=", ":+", "-", "=", "+", "##", "#", "%%", "%"]
                    .iter()
                    .any(|p| op.starts_with(p))
                    && !op.contains(['$', '`', '(', ')', '{', '/', '\\', '\'', '"']));
            if !(name_ok && op_ok) {
                return Err(deny(
                    "S5-expand",
                    format!("parameter expansion ${{{inner}}} is not a plain script form"),
                ));
            }
            let w = lx.w();
            w.text.push('$');
            w.text.push_str(&inner.replace('/', "_"));
            w.dynamic = true;
            Ok(Some(close + 1))
        }
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            let mut j = i + 1;
            while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == '_') {
                j += 1;
            }
            let name: String = s[i..j].iter().collect();
            let w = lx.w();
            w.text.push_str(&name);
            w.dynamic = true;
            Ok(Some(j))
        }
        Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
            let w = lx.w();
            w.text.push('$');
            w.text.push(c);
            w.dynamic = true;
            Ok(Some(i + 2))
        }
        Some('[') => Err(deny(
            "S1-subst",
            "arithmetic $[...] is resolved at run time",
        )),
        Some('\'') | Some('"') if !in_dq => Err(deny(
            "S3-ansi-c",
            "ANSI-C / locale quoting is resolved at run time",
        )),
        _ => Ok(None),
    }
}

/// Script mode: read a here-document delimiter starting at `i`; returns the
/// delimiter, whether it was quoted, and the index after it.
fn heredoc_delim(s: &[char], mut i: usize) -> Result<(String, bool, usize), Deny> {
    while i < s.len() && (s[i] == ' ' || s[i] == '\t') {
        i += 1;
    }
    let mut d = String::new();
    let mut quoted = false;
    while i < s.len() && !s[i].is_whitespace() && !";&|<>()".contains(s[i]) {
        match s[i] {
            '\'' | '"' => {
                let q = s[i];
                quoted = true;
                i += 1;
                while i < s.len() && s[i] != q {
                    d.push(s[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                quoted = true;
                if let Some(&n) = s.get(i + 1) {
                    d.push(n);
                }
                i += 2;
            }
            '$' | '`' => {
                return Err(deny(
                    "S1-heredoc",
                    "here-document delimiter with an expansion",
                ))
            }
            c => {
                d.push(c);
                i += 1;
            }
        }
    }
    if d.is_empty() {
        return Err(deny("S1-heredoc", "here-document with no delimiter"));
    }
    Ok((d, quoted, i))
}

/// Script mode: skip the bodies of the pending here-documents; `i` is the
/// index just after the newline that ended their command line.
fn skip_heredoc_bodies(s: &[char], mut i: usize, docs: Vec<HereDoc>) -> Result<usize, Deny> {
    for h in docs {
        loop {
            if i >= s.len() {
                return Err(deny(
                    "S1-heredoc",
                    format!("here-document {} is never closed", h.delim),
                ));
            }
            let end = s[i..].iter().position(|&c| c == '\n').map(|p| i + p);
            let line: String = s[i..end.unwrap_or(s.len())].iter().collect();
            let cmp = if h.strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            i = end.map(|e| e + 1).unwrap_or(s.len());
            if cmp.trim_end_matches('\r') == h.delim {
                break;
            }
            if !h.quoted && (line.contains('`') || line.contains("$(")) {
                return Err(deny(
                    "S1-heredoc",
                    "a here-document with an unquoted delimiter runs $( ) or backticks; quote the delimiter (<<'EOF')",
                ));
            }
        }
    }
    Ok(i)
}

/// The strict lexer (the terminal command line and all non-script checks).
pub fn lex(src: &str) -> Result<Vec<Segment>, Deny> {
    lex_opts(src, LexOpts::default())
}

pub fn lex_opts(src: &str, opts: LexOpts) -> Result<Vec<Segment>, Deny> {
    let s: Vec<char> = src.chars().collect();
    let mut lx = Lx {
        segs: Vec::new(),
        cur: Segment::default(),
        word: None,
        pending: None,
        pending_dup: false,
        data_next: false,
        heredocs: Vec::new(),
        script: opts.script,
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
                if c == '\n' && !lx.heredocs.is_empty() {
                    let docs = std::mem::take(&mut lx.heredocs);
                    i = skip_heredoc_bodies(&s, i, docs)?;
                }
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
            '(' if opts.script
                && next == Some(')')
                && (lx
                    .word
                    .as_ref()
                    .is_some_and(|w| !w.quoted && is_ident(&w.text))
                    || (lx.word.is_none()
                        && lx.cur.words.len() == 1
                        && is_ident(&lx.cur.words[0].text))) =>
            {
                // function header `name()` / `name ()`
                lx.end_word();
                lx.cur.words.push(Word {
                    text: "()".into(),
                    ..Default::default()
                });
                i += 2;
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
                    if !opts.script {
                        return Err(deny(
                            "S1-heredoc",
                            "here-documents and here-strings (<< / <<<) hide input",
                        ));
                    }
                    if s.get(i + 2) == Some(&'<') {
                        // here-string: the next word is stdin data
                        lx.data_next = true;
                        i += 3;
                        continue;
                    }
                    let mut j = i + 2;
                    let strip_tabs = s.get(j) == Some(&'-');
                    if strip_tabs {
                        j += 1;
                    }
                    let (delim, quoted, after) = heredoc_delim(&s, j)?;
                    lx.heredocs.push(HereDoc {
                        delim,
                        quoted,
                        strip_tabs,
                    });
                    i = after;
                    continue;
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
                            if opts.script {
                                if let Some(after) = script_dollar(&s, i, &mut lx, true)? {
                                    i = after;
                                    continue;
                                }
                            } else if let Some(why) = expansion_after_dollar(s.get(i + 1).copied())
                            {
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
                if opts.script {
                    if let Some(after) = script_dollar(&s, i, &mut lx, false)? {
                        i = after;
                        continue;
                    }
                } else if let Some(why) = expansion_after_dollar(next) {
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
    if !lx.heredocs.is_empty() {
        return Err(deny("S1-heredoc", "here-document with no body"));
    }
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
