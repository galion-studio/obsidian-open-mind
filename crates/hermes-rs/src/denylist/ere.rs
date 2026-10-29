// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! A small, dependency-free regular expression engine for the prior deny
//! list. The prior list is POSIX ERE (`grep -E`) plus the Python
//! `command-guard` floor (Python `re`). This engine covers exactly the
//! syntax those rules use: literals, `.`, bracket classes (ranges, negation,
//! `[:space:]` / `[:alpha:]` / `[:digit:]` / `[:alnum:]`), `\s \S \d \D \w \W
//! \b \B`, groups `( )` / `(?: )`, alternation, `* + ? {m} {m,} {m,n}`,
//! anchors `^ $`, and the flags case-insensitive, dot-matches-newline and
//! multi-line.
//!
//! Matching is a Thompson NFA (Pike VM): linear in the input, no
//! backtracking, so a hostile command can not make a rule hang. No new crate
//! is pulled into the 2 MiB hermes-gateway binary.

#[derive(Debug, Clone, Copy, Default)]
pub struct Flags {
    pub icase: bool,
    pub dotall: bool,
    pub multiline: bool,
}

#[derive(Debug, Clone)]
enum Item {
    Range(char, char),
    Space,
    Digit,
    Word,
    Alpha,
    NotSpace,
}

#[derive(Debug, Clone)]
struct Class {
    neg: bool,
    items: Vec<Item>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Class {
    fn one(&self, c: char) -> bool {
        self.items.iter().any(|it| match *it {
            Item::Range(a, b) => a <= c && c <= b,
            Item::Space => c.is_whitespace(),
            Item::NotSpace => !c.is_whitespace(),
            Item::Digit => c.is_ascii_digit(),
            Item::Word => is_word(c),
            Item::Alpha => c.is_alphabetic(),
        })
    }
    fn matches(&self, c: char, icase: bool) -> bool {
        let hit = self.one(c)
            || (icase
                && (c.to_lowercase().any(|l| self.one(l))
                    || c.to_uppercase().any(|u| self.one(u))));
        hit != self.neg
    }
}

#[derive(Debug, Clone)]
enum Node {
    Empty,
    Char(char),
    Class(Class),
    Dot,
    Start,
    End,
    WordB(bool),
    Cat(Vec<Node>),
    Alt(Vec<Node>),
    Rep(Box<Node>, u32, Option<u32>),
}

struct Parser<'a> {
    s: Vec<char>,
    i: usize,
    _src: &'a str,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn alt(&mut self) -> Result<Node, String> {
        let mut arms = vec![self.cat()?];
        while self.eat('|') {
            arms.push(self.cat()?);
        }
        Ok(if arms.len() == 1 {
            arms.pop().unwrap()
        } else {
            Node::Alt(arms)
        })
    }
    fn cat(&mut self) -> Result<Node, String> {
        let mut v = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            v.push(self.quant(atom)?);
        }
        Ok(match v.len() {
            0 => Node::Empty,
            1 => v.pop().unwrap(),
            _ => Node::Cat(v),
        })
    }
    fn num(&mut self) -> Option<u32> {
        let st = self.i;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.i += 1;
        }
        if st == self.i {
            return None;
        }
        self.s[st..self.i].iter().collect::<String>().parse().ok()
    }
    fn quant(&mut self, mut atom: Node) -> Result<Node, String> {
        loop {
            let (lo, hi) = match self.peek() {
                Some('*') => (0, None),
                Some('+') => (1, None),
                Some('?') => (0, Some(1)),
                Some('{') => {
                    let save = self.i;
                    self.i += 1;
                    match self.num() {
                        Some(m) => {
                            let hi = if self.eat(',') { self.num() } else { Some(m) };
                            if !self.eat('}') {
                                return Err("bad {m,n}".into());
                            }
                            if hi.map(|h| h > 1000 || h < m).unwrap_or(m > 1000) {
                                return Err("repeat too large".into());
                            }
                            self.i -= 1; // re-consume '}' below
                            (m, hi)
                        }
                        None => {
                            self.i = save;
                            return Ok(atom);
                        }
                    }
                }
                _ => return Ok(atom),
            };
            self.i += 1;
            atom = Node::Rep(Box::new(atom), lo, hi);
        }
    }
    fn esc(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("trailing backslash")?;
        self.i += 1;
        let cls = |it: Item, neg: bool| {
            Node::Class(Class {
                neg,
                items: vec![it],
            })
        };
        Ok(match c {
            's' => cls(Item::Space, false),
            'S' => cls(Item::Space, true),
            'd' => cls(Item::Digit, false),
            'D' => cls(Item::Digit, true),
            'w' => cls(Item::Word, false),
            'W' => cls(Item::Word, true),
            'b' => Node::WordB(true),
            'B' => Node::WordB(false),
            'n' => Node::Char('\n'),
            't' => Node::Char('\t'),
            other => Node::Char(other),
        })
    }
    fn atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end")?;
        self.i += 1;
        match c {
            '(' => {
                if self.peek() == Some('?') {
                    self.i += 1;
                    if !self.eat(':') {
                        return Err("only (?: ) groups are supported".into());
                    }
                }
                let n = self.alt()?;
                if !self.eat(')') {
                    return Err("missing )".into());
                }
                Ok(n)
            }
            '[' => self.class(),
            '.' => Ok(Node::Dot),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '\\' => self.esc(),
            '*' | '+' | '?' => Err("quantifier with nothing to repeat".into()),
            other => Ok(Node::Char(other)),
        }
    }
    fn class(&mut self) -> Result<Node, String> {
        let mut cl = Class {
            neg: self.eat('^'),
            items: Vec::new(),
        };
        let mut first = true;
        loop {
            let c = self.peek().ok_or("missing ]")?;
            if c == ']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            if c == '[' && self.s.get(self.i + 1) == Some(&':') {
                let rest: String = self.s[self.i..].iter().take(12).collect();
                let named = [
                    ("[:space:]", Item::Space),
                    ("[:digit:]", Item::Digit),
                    ("[:alpha:]", Item::Alpha),
                    ("[:alnum:]", Item::Word),
                ];
                if let Some((n, it)) = named.iter().find(|(n, _)| rest.starts_with(n)) {
                    self.i += n.chars().count();
                    cl.items.push(it.clone());
                    continue;
                }
            }
            self.i += 1;
            let lo = if c == '\\' {
                let e = self.peek().ok_or("bad escape in class")?;
                self.i += 1;
                match e {
                    's' => {
                        cl.items.push(Item::Space);
                        continue;
                    }
                    'S' => {
                        cl.items.push(Item::NotSpace);
                        continue;
                    }
                    'd' => {
                        cl.items.push(Item::Digit);
                        continue;
                    }
                    'w' => {
                        cl.items.push(Item::Word);
                        continue;
                    }
                    'n' => '\n',
                    't' => '\t',
                    o => o,
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.s.get(self.i + 1).is_some_and(|&n| n != ']') {
                self.i += 1;
                let mut hi = self.peek().ok_or("bad range")?;
                self.i += 1;
                if hi == '\\' {
                    hi = self.peek().ok_or("bad range")?;
                    self.i += 1;
                }
                cl.items.push(Item::Range(lo, hi));
            } else {
                cl.items.push(Item::Range(lo, lo));
            }
        }
        Ok(Node::Class(cl))
    }
}

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Class(Class),
    Dot,
    Start,
    End,
    WordB(bool),
    Split(usize, usize),
    Jmp(usize),
    Match,
}

/// A compiled rule.
#[derive(Debug, Clone)]
pub struct Regex {
    prog: Vec<Inst>,
    flags: Flags,
    pub source: String,
}

const MAX_PROG: usize = 20_000;

fn emit(n: &Node, p: &mut Vec<Inst>) -> Result<(), String> {
    if p.len() > MAX_PROG {
        return Err("pattern too large".into());
    }
    match n {
        Node::Empty => {}
        Node::Char(c) => p.push(Inst::Char(*c)),
        Node::Class(c) => p.push(Inst::Class(c.clone())),
        Node::Dot => p.push(Inst::Dot),
        Node::Start => p.push(Inst::Start),
        Node::End => p.push(Inst::End),
        Node::WordB(b) => p.push(Inst::WordB(*b)),
        Node::Cat(v) => {
            for x in v {
                emit(x, p)?;
            }
        }
        Node::Alt(arms) => {
            let mut jmps = Vec::new();
            for (k, arm) in arms.iter().enumerate() {
                if k + 1 < arms.len() {
                    let split = p.len();
                    p.push(Inst::Split(split + 1, 0));
                    emit(arm, p)?;
                    jmps.push(p.len());
                    p.push(Inst::Jmp(0));
                    let next = p.len();
                    p[split] = Inst::Split(split + 1, next);
                } else {
                    emit(arm, p)?;
                }
            }
            let end = p.len();
            for j in jmps {
                p[j] = Inst::Jmp(end);
            }
        }
        Node::Rep(x, lo, hi) => {
            for _ in 0..*lo {
                emit(x, p)?;
            }
            match hi {
                None => {
                    let l = p.len();
                    p.push(Inst::Split(l + 1, 0));
                    emit(x, p)?;
                    p.push(Inst::Jmp(l));
                    let out = p.len();
                    p[l] = Inst::Split(l + 1, out);
                }
                Some(h) => {
                    let mut holes = Vec::new();
                    for _ in *lo..*h {
                        let s = p.len();
                        p.push(Inst::Split(s + 1, 0));
                        holes.push(s);
                        emit(x, p)?;
                    }
                    let out = p.len();
                    for s in holes {
                        p[s] = Inst::Split(s + 1, out);
                    }
                }
            }
        }
    }
    Ok(())
}

impl Regex {
    pub fn new(src: &str, flags: Flags) -> Result<Regex, String> {
        let mut ps = Parser {
            s: src.chars().collect(),
            i: 0,
            _src: src,
        };
        let n = ps.alt()?;
        if ps.i != ps.s.len() {
            return Err(format!("unbalanced ) at {}", ps.i));
        }
        let mut prog = Vec::new();
        emit(&n, &mut prog)?;
        prog.push(Inst::Match);
        Ok(Regex {
            prog,
            flags,
            source: src.to_string(),
        })
    }

    fn assert_ok(&self, inst: &Inst, t: &[char], i: usize) -> bool {
        let prev = if i == 0 { None } else { Some(t[i - 1]) };
        let cur = t.get(i).copied();
        match inst {
            Inst::Start => i == 0 || (self.flags.multiline && prev == Some('\n')),
            Inst::End => i == t.len() || (self.flags.multiline && cur == Some('\n')),
            Inst::WordB(want) => {
                let a = prev.is_some_and(is_word);
                let b = cur.is_some_and(is_word);
                (a != b) == *want
            }
            _ => true,
        }
    }

    fn add(&self, list: &mut Vec<usize>, seen: &mut [bool], pc: usize, t: &[char], i: usize) {
        // Iterative epsilon closure (explicit stack: no recursion depth limit).
        let mut stack = vec![pc];
        while let Some(pc) = stack.pop() {
            if seen[pc] {
                continue;
            }
            seen[pc] = true;
            match &self.prog[pc] {
                Inst::Jmp(a) => stack.push(*a),
                Inst::Split(a, b) => {
                    stack.push(*b);
                    stack.push(*a);
                }
                inst @ (Inst::Start | Inst::End | Inst::WordB(_)) => {
                    if self.assert_ok(inst, t, i) {
                        stack.push(pc + 1);
                    }
                }
                _ => list.push(pc),
            }
        }
    }

    /// True when the rule matches anywhere in `text` (like `re.search`).
    pub fn is_match(&self, text: &str) -> bool {
        let t: Vec<char> = text.chars().collect();
        let n = self.prog.len();
        let mut clist: Vec<usize> = Vec::new();
        let mut nlist: Vec<usize> = Vec::new();
        let mut seen = vec![false; n];
        for i in 0..=t.len() {
            // unanchored search: a new thread may start at every position
            self.add(&mut clist, &mut seen, 0, &t, i);
            if clist.iter().any(|&pc| matches!(self.prog[pc], Inst::Match)) {
                return true;
            }
            if i == t.len() {
                break;
            }
            let c = t[i];
            seen.iter_mut().for_each(|s| *s = false);
            nlist.clear();
            for &pc in &clist {
                let ok = match &self.prog[pc] {
                    Inst::Char(x) => {
                        *x == c || (self.flags.icase && x.to_lowercase().eq(c.to_lowercase()))
                    }
                    Inst::Class(cl) => cl.matches(c, self.flags.icase),
                    Inst::Dot => self.flags.dotall || c != '\n',
                    _ => false,
                };
                if ok {
                    self.add(&mut nlist, &mut seen, pc + 1, &t, i + 1);
                }
            }
            std::mem::swap(&mut clist, &mut nlist);
            // `seen` now marks the threads at i+1; keep it so the start
            // thread added next round does not duplicate them.
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(p: &str, t: &str) -> bool {
        Regex::new(p, Flags::default()).unwrap().is_match(t)
    }
    #[test]
    fn basics() {
        assert!(m("abc", "xxabcxx"));
        assert!(!m("^abc", "xabc"));
        assert!(m("a(b|c)+d", "abcbd"));
        assert!(m("x{2,3}y", "axxy"));
        assert!(!m("^x{2,3}y", "xy"));
        assert!(m(r"\bkill\s+-9\b", "sudo kill -9 1"));
        assert!(!m(r"\bkill\s+-9\b", "skill -9 1"));
        assert!(m("[^/[:space:]\"']+", "abc"));
        assert!(m("[a-z-]+$", "--no-x"));
        assert!(m(r"rm[[:space:]]+-rf", "rm  -rf"));
        assert!(m("(^|[;&|[:space:]])rm", "a;rm"));
        assert!(!m("(^|[;&|[:space:]])rm", "/bin/rm"));
        let ci = Flags {
            icase: true,
            ..Default::default()
        };
        assert!(Regex::new("openrouter_api_key", ci)
            .unwrap()
            .is_match("echo $OPENROUTER_API_KEY"));
        assert!(m(":\\(\\)[[:space:]]*\\{", ":(){ :|:& };:"));
        assert!(m("a.{0,3}b", "a12b"));
        assert!(!m("a.{0,3}b", "a1234b"));
        assert!(!m("a.b", "a\nb"));
        let dotall = Flags {
            dotall: true,
            ..Default::default()
        };
        assert!(Regex::new("a.b", dotall).unwrap().is_match("a\nb"));
    }
    #[test]
    fn hostile_input_is_linear() {
        let r = Regex::new("(a+)+$", Flags::default()).unwrap();
        let s = "a".repeat(20_000) + "!";
        let t0 = std::time::Instant::now();
        assert!(!r.is_match(&s));
        assert!(t0.elapsed().as_secs() < 5);
    }
}
