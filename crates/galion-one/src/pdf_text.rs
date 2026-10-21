// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// First-page text of a PDF in pure Rust (std only, no unsafe, no SIMD, no
// assembly, no crates). Replaces the `pdftotext -f 1 -l 1` subprocess for
// the "they included a file" note (main.rs `pdf_clip`). Written from the
// PDF 1.7 specification (ISO 32000-1, Adobe's free copy:
// https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf).
//
// What it reads:
// * objects found by scanning for `N G obj` (no reliance on the xref table,
//   so broken or rebuilt files still work; later copies win, which is what an
//   incremental update means), plus objects packed in /ObjStm streams;
// * filters FlateDecode (own inflate.rs), LZWDecode, ASCIIHexDecode,
//   ASCII85Decode, RunLengthDecode; image filters are never needed for text;
// * the first leaf of the page tree, its (inherited) /Resources, /Contents
//   (one stream or an array) and Form XObjects drawn with `Do`;
// * text operators Tj, TJ, ', " with line breaks from Td/TD/T*/Tm and word
//   gaps from TJ offsets;
// * fonts: /ToUnicode CMaps (bfchar, bfrange, codespace ranges; this is what
//   Chrome/Skia, LibreOffice, Ghostscript, Word and LaTeX write), else the
//   simple-font /Encoding (WinAnsi, Standard, MacRoman upper half
//   approximated, /Differences with common glyph names).
//
// What it does not do (the caller may fall back to pdftotext, see host.rs):
// encrypted files (/Encrypt), CID fonts without /ToUnicode, reading order by
// position (text comes in content-stream order, which for the 480-character
// clip is what matters), OCR of scanned pages.
//
// Hostile input: every index is checked, recursion is depth-limited, decoded
// bytes, operators and output characters are all capped (`Limits`).

use crate::inflate;
use std::cell::{Cell, OnceCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Max decoded size of one stream.
    pub max_stream: usize,
    /// Max decoded bytes over the whole run (all streams).
    pub max_total: usize,
    /// Max content-stream tokens interpreted.
    pub max_ops: usize,
    /// Stop once this many characters of text were produced.
    pub max_chars: usize,
}

pub const DEFAULT_LIMITS: Limits = Limits {
    max_stream: 16 << 20,
    max_total: 32 << 20,
    max_ops: 2_000_000,
    max_chars: 4000,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Text(String),
    NotPdf,
    Encrypted,
    NoPage,
    NoText,
}

type Dict = Vec<(Vec<u8>, Obj)>;

#[derive(Clone, Debug)]
enum Obj {
    Null,
    Bool,
    Int(i64),
    Real(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Arr(Vec<Obj>),
    Dict(Dict),
    Ref(u32),
    /// dict, start and end of the raw bytes in the file, object number
    Stream(Dict, usize, usize, u32),
    Op(Vec<u8>),
}

fn dget<'a>(d: &'a Dict, key: &str) -> Option<&'a Obj> {
    d.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v)
}

impl Obj {
    fn num(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }
    fn name(&self) -> Option<&[u8]> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }
    fn dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) | Obj::Stream(d, _, _, _) => Some(d),
            _ => None,
        }
    }
}

fn is_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}
fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}
fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

struct Lex<'a> {
    s: &'a [u8],
    p: usize,
    /// Recognise `N G R` references (object syntax, not content streams).
    refs: bool,
}

const MAX_DEPTH: usize = 48;

impl<'a> Lex<'a> {
    fn new(s: &'a [u8], p: usize, refs: bool) -> Self {
        Lex { s, p, refs }
    }

    fn skip_ws(&mut self) {
        loop {
            while self.p < self.s.len() && is_ws(self.s[self.p]) {
                self.p += 1;
            }
            if self.p < self.s.len() && self.s[self.p] == b'%' {
                while self.p < self.s.len() && self.s[self.p] != b'\n' && self.s[self.p] != b'\r' {
                    self.p += 1;
                }
            } else {
                return;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let start = self.p;
        while self.p < self.s.len() && !is_ws(self.s[self.p]) && !is_delim(self.s[self.p]) {
            self.p += 1;
        }
        &self.s[start..self.p]
    }

    fn uint_at(&self, mut p: usize) -> Option<(u64, usize)> {
        let start = p;
        let mut v: u64 = 0;
        while p < self.s.len() && self.s[p].is_ascii_digit() && p - start < 12 {
            v = v * 10 + u64::from(self.s[p] - b'0');
            p += 1;
        }
        (p > start).then_some((v, p))
    }

    fn token(&mut self, depth: usize) -> Option<Obj> {
        self.skip_ws();
        let c = *self.s.get(self.p)?;
        match c {
            b'/' => {
                self.p += 1;
                let raw = self.word();
                let mut name = Vec::with_capacity(raw.len());
                let mut i = 0;
                while i < raw.len() {
                    if raw[i] == b'#' && i + 2 < raw.len() {
                        if let (Some(h), Some(l)) = (hexval(raw[i + 1]), hexval(raw[i + 2])) {
                            name.push(h * 16 + l);
                            i += 3;
                            continue;
                        }
                    }
                    name.push(raw[i]);
                    i += 1;
                }
                Some(Obj::Name(name))
            }
            b'(' => Some(Obj::Str(self.literal())),
            b'<' if self.s.get(self.p + 1) == Some(&b'<') => {
                self.p += 2;
                if depth > MAX_DEPTH {
                    return Some(Obj::Null);
                }
                let mut d = Dict::new();
                loop {
                    self.skip_ws();
                    if self.p >= self.s.len() {
                        break;
                    }
                    if self.s[self.p] == b'>' {
                        self.p += if self.s.get(self.p + 1) == Some(&b'>') {
                            2
                        } else {
                            1
                        };
                        break;
                    }
                    let before = self.p;
                    let Some(k) = self.token(depth + 1) else {
                        break;
                    };
                    let Obj::Name(k) = k else {
                        if self.p == before {
                            self.p += 1;
                        }
                        continue;
                    };
                    self.skip_ws();
                    if self.s.get(self.p) == Some(&b'>') {
                        continue;
                    }
                    let Some(v) = self.token(depth + 1) else {
                        break;
                    };
                    if d.len() < 4096 {
                        d.push((k, v));
                    }
                }
                Some(Obj::Dict(d))
            }
            b'<' => {
                self.p += 1;
                let mut out = Vec::new();
                let mut hi: Option<u8> = None;
                while self.p < self.s.len() && self.s[self.p] != b'>' {
                    if let Some(v) = hexval(self.s[self.p]) {
                        match hi.take() {
                            Some(h) => out.push(h * 16 + v),
                            None => hi = Some(v),
                        }
                    }
                    self.p += 1;
                }
                if let Some(h) = hi {
                    out.push(h * 16);
                }
                self.p += 1;
                Some(Obj::Str(out))
            }
            b'[' => {
                self.p += 1;
                if depth > MAX_DEPTH {
                    return Some(Obj::Null);
                }
                let mut a = Vec::new();
                loop {
                    self.skip_ws();
                    match self.s.get(self.p) {
                        None => break,
                        Some(b']') => {
                            self.p += 1;
                            break;
                        }
                        _ => {}
                    }
                    let before = self.p;
                    match self.token(depth + 1) {
                        Some(o) => {
                            if a.len() < 1 << 16 {
                                a.push(o)
                            }
                        }
                        None => break,
                    }
                    if self.p == before {
                        self.p += 1;
                    }
                }
                Some(Obj::Arr(a))
            }
            b')' | b'>' | b']' | b'{' | b'}' => {
                self.p += 1;
                Some(Obj::Op(vec![c]))
            }
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                let w = self.word();
                let text = std::str::from_utf8(w).unwrap_or("");
                if !w.contains(&b'.') {
                    let v: i64 = text.parse().unwrap_or(0);
                    if self.refs && v >= 0 && w[0].is_ascii_digit() {
                        let mut q = self.p;
                        while q < self.s.len() && is_ws(self.s[q]) {
                            q += 1;
                        }
                        if let Some((_, q2)) = self.uint_at(q) {
                            let mut q = q2;
                            while q < self.s.len() && is_ws(self.s[q]) {
                                q += 1;
                            }
                            if self.s.get(q) == Some(&b'R')
                                && self.s.get(q + 1).is_none_or(|b| is_ws(*b) || is_delim(*b))
                                && q2 > self.p
                                && v <= u32::MAX as i64
                            {
                                self.p = q + 1;
                                return Some(Obj::Ref(v as u32));
                            }
                        }
                    }
                    Some(Obj::Int(v))
                } else {
                    // "-.5", "4.", and junk like "1.2.3" (-> 0, as poppler).
                    Some(Obj::Real(text.parse().unwrap_or(0.0)))
                }
            }
            _ => {
                let w = self.word();
                if w.is_empty() {
                    self.p += 1;
                    return Some(Obj::Op(vec![c]));
                }
                Some(match w {
                    b"true" | b"false" => Obj::Bool,
                    b"null" => Obj::Null,
                    _ => Obj::Op(w.to_vec()),
                })
            }
        }
    }

    fn literal(&mut self) -> Vec<u8> {
        self.p += 1;
        let mut depth = 1u32;
        let mut out = Vec::new();
        while self.p < self.s.len() {
            let c = self.s[self.p];
            self.p += 1;
            match c {
                b'\\' => {
                    let Some(&e) = self.s.get(self.p) else { break };
                    self.p += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\r' => {
                            if self.s.get(self.p) == Some(&b'\n') {
                                self.p += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.s.get(self.p) {
                                    Some(&d @ b'0'..=b'7') => {
                                        v = v * 8 + u32::from(d - b'0');
                                        self.p += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        out
    }
}

/// Decoded object streams, and object number -> (stream index, offset).
type ObjStms = (Vec<Vec<u8>>, HashMap<u32, (usize, usize)>);

struct Doc<'a> {
    data: &'a [u8],
    /// object number -> byte offset just after `obj`
    offsets: HashMap<u32, usize>,
    gens: HashMap<u32, u16>,
    /// RC4 file key when the file is encrypted with an empty user password
    key: Option<Vec<u8>>,
    objstm: OnceCell<ObjStms>,
    budget: Cell<usize>,
    limits: Limits,
}

impl<'a> Doc<'a> {
    fn new(data: &'a [u8], limits: Limits) -> Self {
        let mut offsets = HashMap::new();
        let mut gens = HashMap::new();
        let mut i = 0;
        while let Some(k) = find(data, b"obj", i) {
            i = k + 3;
            if data.get(k + 3).is_some_and(|b| !is_ws(*b) && !is_delim(*b)) {
                continue;
            }
            // walk back: ws* gen ws+ num, num preceded by ws/delim/start
            let mut q = k;
            while q > 0 && is_ws(data[q - 1]) {
                q -= 1;
            }
            let gen_end = q;
            while q > 0 && data[q - 1].is_ascii_digit() {
                q -= 1;
            }
            if q == gen_end || gen_end - q > 5 {
                continue;
            }
            let gen_start = q;
            while q > 0 && is_ws(data[q - 1]) {
                q -= 1;
            }
            if q == gen_start {
                continue;
            }
            let num_end = q;
            while q > 0 && data[q - 1].is_ascii_digit() {
                q -= 1;
            }
            if q == num_end || num_end - q > 10 {
                continue;
            }
            if q > 0 && !is_ws(data[q - 1]) && !is_delim(data[q - 1]) {
                continue;
            }
            let Ok(num) = std::str::from_utf8(&data[q..num_end])
                .unwrap_or("x")
                .parse::<u32>()
            else {
                continue;
            };
            if offsets.len() < 2_000_000 || offsets.contains_key(&num) {
                offsets.insert(num, k + 3);
                let gen = std::str::from_utf8(&data[gen_start..gen_end])
                    .ok()
                    .and_then(|g| g.parse::<u16>().ok())
                    .unwrap_or(0);
                gens.insert(num, gen);
            }
        }
        Doc {
            data,
            offsets,
            gens,
            key: None,
            objstm: OnceCell::new(),
            budget: Cell::new(limits.max_total),
            limits,
        }
    }

    fn parse_at(&self, off: usize, with_stream: bool, num: u32) -> Option<Obj> {
        let mut lx = Lex::new(self.data, off, true);
        let o = lx.token(0)?;
        let Obj::Dict(d) = o else { return Some(o) };
        lx.skip_ws();
        if !self.data[lx.p.min(self.data.len())..].starts_with(b"stream") {
            return Some(Obj::Dict(d));
        }
        if !with_stream {
            return Some(Obj::Stream(d, 0, 0, num));
        }
        let mut start = lx.p + 6;
        if self.data.get(start) == Some(&b'\r') {
            start += 1;
        }
        if self.data.get(start) == Some(&b'\n') {
            start += 1;
        }
        let len = match dget(&d, "Length") {
            Some(Obj::Int(n)) => usize::try_from(*n).ok(),
            Some(Obj::Ref(r)) => self
                .offsets
                .get(r)
                .and_then(|o| self.parse_at(*o, false, 0))
                .and_then(|o| match o {
                    Obj::Int(n) => usize::try_from(n).ok(),
                    _ => None,
                }),
            _ => None,
        };
        let good = len.and_then(|n| {
            let end = start.checked_add(n)?;
            let mut q = end;
            while q < self.data.len() && is_ws(self.data[q]) {
                q += 1;
            }
            (end <= self.data.len() && self.data[q..].starts_with(b"endstream")).then_some(end)
        });
        let end = match good {
            Some(e) => e,
            None => {
                let mut e = find(self.data, b"endstream", start).unwrap_or(self.data.len());
                if e > start && self.data[e - 1] == b'\n' {
                    e -= 1;
                }
                if e > start && self.data[e - 1] == b'\r' {
                    e -= 1;
                }
                e
            }
        };
        Some(Obj::Stream(d, start, end.max(start), num))
    }

    fn get(&self, num: u32) -> Option<Obj> {
        if let Some(off) = self.offsets.get(&num) {
            return self.parse_at(*off, true, num);
        }
        let (bufs, map) = self.objstm.get_or_init(|| self.load_objstms());
        let (i, off) = map.get(&num)?;
        Lex::new(bufs.get(*i)?, *off, true).token(0)
    }

    fn load_objstms(&self) -> ObjStms {
        let mut bufs = Vec::new();
        let mut map = HashMap::new();
        let mut nums: Vec<(usize, u32)> = self.offsets.iter().map(|(n, o)| (*o, *n)).collect();
        nums.sort_unstable();
        for (off, onum) in nums {
            let Some(Obj::Stream(d, _, _, _)) = self.parse_at(off, false, onum) else {
                continue;
            };
            if dget(&d, "Type").and_then(Obj::name) != Some(b"ObjStm") {
                continue;
            }
            let Some(o @ Obj::Stream(..)) = self.parse_at(off, true, onum) else {
                continue;
            };
            let Some(data) = self.stream_bytes(&o) else {
                continue;
            };
            let first = dget(&d, "First").and_then(Obj::num).unwrap_or(0.0) as usize;
            let n = dget(&d, "N").and_then(Obj::num).unwrap_or(0.0) as usize;
            let mut lx = Lex::new(&data, 0, false);
            let idx = bufs.len();
            for _ in 0..n.min(100_000) {
                let (Some(Obj::Int(num)), Some(Obj::Int(rel))) = (lx.token(0), lx.token(0)) else {
                    break;
                };
                if let (Ok(num), Ok(rel)) = (u32::try_from(num), usize::try_from(rel)) {
                    if !self.offsets.contains_key(&num) {
                        map.insert(num, (idx, first.saturating_add(rel)));
                    }
                }
            }
            bufs.push(data);
        }
        (bufs, map)
    }

    fn deref(&self, o: &Obj) -> Obj {
        let mut cur = o.clone();
        for _ in 0..8 {
            match cur {
                Obj::Ref(n) => cur = self.get(n).unwrap_or(Obj::Null),
                _ => return cur,
            }
        }
        Obj::Null
    }

    fn take_budget(&self, n: usize) -> bool {
        let b = self.budget.get();
        if n > b {
            self.budget.set(0);
            false
        } else {
            self.budget.set(b - n);
            true
        }
    }

    /// Decoded bytes of a stream object, `None` for filters we cannot (or
    /// need not) decode.
    fn stream_bytes(&self, o: &Obj) -> Option<Vec<u8>> {
        let Obj::Stream(d, start, end, num) = o else {
            return None;
        };
        let mut raw = std::borrow::Cow::Borrowed(self.data.get(*start..*end)?);
        if let Some(key) = &self.key {
            if *num != 0 && dget(d, "Type").and_then(Obj::name) != Some(b"XRef") {
                let gen = self.gens.get(num).copied().unwrap_or(0);
                let k = crate::pdf_crypt::object_key(key, *num, gen);
                raw = std::borrow::Cow::Owned(crate::pdf_crypt::rc4(&k, &raw));
            }
        }
        let filters: Vec<Obj> = match dget(d, "Filter").map(|f| self.deref(f)) {
            None | Some(Obj::Null) => Vec::new(),
            Some(Obj::Arr(a)) => a,
            Some(other) => vec![other],
        };
        let parms: Vec<Obj> = match dget(d, "DecodeParms").map(|f| self.deref(f)) {
            Some(Obj::Arr(a)) => a.iter().map(|x| self.deref(x)).collect(),
            Some(x @ Obj::Dict(_)) => vec![x],
            _ => Vec::new(),
        };
        let mut cur = raw.to_vec();
        for (i, f) in filters.iter().enumerate() {
            let cap = self.limits.max_stream.min(self.budget.get());
            let pred = parms
                .get(i)
                .and_then(Obj::dict)
                .and_then(|p| dget(p, "Predictor"))
                .and_then(Obj::num)
                .unwrap_or(1.0);
            if pred > 1.0 {
                return None; // only xref/image streams use predictors
            }
            let early = parms
                .get(i)
                .and_then(Obj::dict)
                .and_then(|p| dget(p, "EarlyChange"))
                .and_then(Obj::num)
                .unwrap_or(1.0);
            cur = match f.name()? {
                b"FlateDecode" | b"Fl" => {
                    let (out, res) = inflate::inflate_zlib(&cur, cap);
                    if res.is_err() && out.is_empty() {
                        return None;
                    }
                    out
                }
                b"LZWDecode" | b"LZW" => lzw(&cur, cap, early != 0.0),
                b"ASCIIHexDecode" | b"AHx" => ascii_hex(&cur),
                b"ASCII85Decode" | b"A85" => ascii85(&cur),
                b"RunLengthDecode" | b"RL" => run_length(&cur, cap),
                _ => return None,
            };
            cur.truncate(cap);
            if !self.take_budget(cur.len()) {
                return None;
            }
        }
        if filters.is_empty() && !self.take_budget(cur.len()) {
            return None;
        }
        Some(cur)
    }

    fn trailers(&self) -> Vec<Dict> {
        let mut dicts: Vec<Dict> = Vec::new();
        // Every `trailer` (linearized and updated files have several; the
        // last one wins for /Root, any one with /Encrypt counts).
        let mut at = Vec::new();
        let mut from = 0;
        while let Some(t) = find(self.data, b"trailer", from) {
            at.push(t);
            from = t + 7;
        }
        for t in at.into_iter().rev().take(64) {
            if let Some(Obj::Dict(d)) = Lex::new(self.data, t + 7, true).token(0) {
                dicts.push(d);
            }
        }
        // Cross-reference stream dictionaries (PDF 1.5+) carry the trailer.
        let mut xrefs: Vec<(usize, Dict)> = Vec::new();
        for (num, off) in &self.offsets {
            if let Some(Obj::Stream(d, _, _, _)) = self.parse_at(*off, false, *num) {
                if dget(&d, "Type").and_then(Obj::name) == Some(b"XRef") {
                    xrefs.push((*off, d));
                }
            }
        }
        xrefs.sort_by_key(|x| std::cmp::Reverse(x.0));
        dicts.extend(xrefs.into_iter().map(|x| x.1));
        dicts
    }

    /// Set the RC4 key from /Encrypt. Err when the file needs a password
    /// or uses a handler we do not implement (AES, public-key).
    fn unlock(&mut self, dicts: &[Dict]) -> Result<(), ()> {
        let Some(enc) = dicts.iter().find_map(|d| dget(d, "Encrypt")) else {
            return Ok(());
        };
        let Obj::Dict(e) = self.deref(enc) else {
            return Err(());
        };
        let int = |k: &str, def: i64| dget(&e, k).and_then(Obj::num).map_or(def, |v| v as i64);
        let s = |k: &str| match dget(&e, k) {
            Some(Obj::Str(v)) => v.clone(),
            _ => Vec::new(),
        };
        if dget(&e, "Filter").and_then(Obj::name) != Some(b"Standard")
            || !(1..=2).contains(&int("V", 0))
        {
            return Err(());
        }
        let id0 = dicts
            .iter()
            .find_map(|d| match dget(d, "ID").map(|i| self.deref(i)) {
                Some(Obj::Arr(a)) => match a.first() {
                    Some(Obj::Str(s)) => Some(s.clone()),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default();
        let key = crate::pdf_crypt::empty_password_key(
            int("R", 0),
            int("Length", 40),
            &s("O"),
            &s("U"),
            int("P", 0),
            &id0,
        )
        .ok_or(())?;
        self.key = Some(key);
        Ok(())
    }

    fn root(&self, dicts: &[Dict]) -> Option<Dict> {
        let mut root = None;
        for d in dicts {
            if root.is_none() {
                if let Some(Obj::Dict(r)) = dget(d, "Root").map(|r| self.deref(r)) {
                    root = Some(r);
                }
            }
        }
        if root.is_none() {
            let mut offs: Vec<usize> = self.offsets.values().copied().collect();
            offs.sort_unstable();
            for off in offs.into_iter().rev() {
                if let Some(Obj::Dict(d)) = self.parse_at(off, false, 0) {
                    if dget(&d, "Type").and_then(Obj::name) == Some(b"Catalog") {
                        root = Some(d);
                        break;
                    }
                }
            }
        }
        root
    }

    /// First leaf of the page tree and its inherited /Resources.
    fn first_page(&self, root: &Dict) -> Option<(Dict, Option<Dict>)> {
        let pages = self.deref(dget(root, "Pages")?);
        let mut seen = HashSet::new();
        self.leaf(&pages, None, &mut seen, 0)
    }

    fn leaf(
        &self,
        node: &Obj,
        res: Option<Dict>,
        seen: &mut HashSet<usize>,
        depth: usize,
    ) -> Option<(Dict, Option<Dict>)> {
        if depth > 64 {
            return None;
        }
        let d = node.dict()?;
        let res = match dget(d, "Resources").map(|r| self.deref(r)) {
            Some(Obj::Dict(r)) => Some(r),
            _ => res,
        };
        let Some(kids) = dget(d, "Kids") else {
            let is_pages = dget(d, "Type").and_then(Obj::name) == Some(b"Pages");
            return (!is_pages).then(|| (d.clone(), res));
        };
        let Obj::Arr(kids) = self.deref(kids) else {
            return None;
        };
        for kid in kids.iter().take(10_000) {
            if let Obj::Ref(n) = kid {
                if !seen.insert(*n as usize) {
                    continue;
                }
            }
            let k = self.deref(kid);
            if let Some(found) = self.leaf(&k, res.clone(), seen, depth + 1) {
                return Some(found);
            }
        }
        None
    }
}

fn lzw(data: &[u8], cap: usize, early: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut table: Vec<Vec<u8>> = (0..=255u8).map(|b| vec![b]).collect();
    table.push(Vec::new());
    table.push(Vec::new());
    let mut width = 9u32;
    let (mut acc, mut nacc, mut pos) = (0u32, 0u32, 0usize);
    let mut prev: Option<Vec<u8>> = None;
    loop {
        while nacc < width {
            let Some(&b) = data.get(pos) else { return out };
            pos += 1;
            acc = (acc << 8) | u32::from(b);
            nacc += 8;
        }
        let code = ((acc >> (nacc - width)) & ((1 << width) - 1)) as usize;
        nacc -= width;
        acc &= (1u32 << nacc).wrapping_sub(1);
        if code == 256 {
            table.truncate(258);
            width = 9;
            prev = None;
            continue;
        }
        if code == 257 {
            return out;
        }
        let entry = if code < table.len() && code != 256 && code != 257 {
            table[code].clone()
        } else if code == table.len() {
            match &prev {
                Some(p) if !p.is_empty() => {
                    let mut e = p.clone();
                    e.push(p[0]);
                    e
                }
                _ => return out,
            }
        } else {
            return out;
        };
        if out.len() + entry.len() > cap {
            return out;
        }
        out.extend_from_slice(&entry);
        if let Some(p) = prev.take() {
            if table.len() < 4096 && !entry.is_empty() {
                let mut n = p;
                n.push(entry[0]);
                table.push(n);
            }
        }
        prev = Some(entry);
        let next = table.len() + usize::from(early);
        width = if next >= 2048 {
            12
        } else if next >= 1024 {
            11
        } else if next >= 512 {
            10
        } else {
            9
        };
    }
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut hi = None;
    for &b in data {
        if b == b'>' {
            break;
        }
        if let Some(v) = hexval(b) {
            match hi.take() {
                Some(h) => out.push(h * 16 + v),
                None => hi = Some(v),
            }
        }
    }
    if let Some(h) = hi {
        out.push(h * 16);
    }
    out
}

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut n = 0;
    let mut i = 0;
    if data.starts_with(b"<~") {
        i = 2;
    }
    while i < data.len() {
        let b = data[i];
        i += 1;
        if b == b'~' {
            break;
        }
        if is_ws(b) {
            continue;
        }
        if b == b'z' && n == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&b) {
            continue;
        }
        group[n] = b - b'!';
        n += 1;
        if n == 5 {
            let v = group.iter().fold(0u64, |a, d| a * 85 + u64::from(*d));
            out.extend_from_slice(&(v as u32).to_be_bytes());
            n = 0;
        }
    }
    if n > 1 {
        for g in group.iter_mut().skip(n) {
            *g = 84;
        }
        let v = group.iter().fold(0u64, |a, d| a * 85 + u64::from(*d));
        out.extend_from_slice(&(v as u32).to_be_bytes()[..n - 1]);
    }
    out
}

fn run_length(data: &[u8], cap: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() && out.len() < cap {
        let len = data[i];
        i += 1;
        match len {
            128 => break,
            0..=127 => {
                let n = usize::from(len) + 1;
                let end = (i + n).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                let Some(&b) = data.get(i) else { break };
                i += 1;
                out.extend(std::iter::repeat_n(b, 257 - usize::from(len)));
            }
        }
    }
    out
}

// ---- fonts -------------------------------------------------------------

#[derive(Default)]
struct CMap {
    /// (bytes, low, high) codespace ranges
    space: Vec<(usize, u32, u32)>,
    single: HashMap<(usize, u32), String>,
    /// (bytes, low, high, first destination, or per-code list)
    ranges: Vec<(usize, u32, u32, RangeDst)>,
}

enum RangeDst {
    Base(Vec<u16>),
    List(Vec<String>),
}

/// A PDF text string: UTF-16BE or UTF-8 with a BOM, else PDFDocEncoding
/// (Latin-1 for the printable range).
fn text_string(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xfe, 0xff]) {
        return utf16(rest);
    }
    if let Some(rest) = b.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    b.iter().map(|&c| char::from(c)).collect()
}

fn be(code: &[u8]) -> u32 {
    code.iter()
        .take(4)
        .fold(0u32, |a, b| (a << 8) | u32::from(*b))
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from(c[0])
            }
        })
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{fffd}'))
        .collect()
}

impl CMap {
    fn parse(data: &[u8]) -> CMap {
        let mut cm = CMap::default();
        let mut lx = Lex::new(data, 0, false);
        let mut ops: Vec<Obj> = Vec::new();
        let mut n = 0usize;
        while let Some(t) = lx.token(0) {
            n += 1;
            if n > 2_000_000 {
                break;
            }
            let Obj::Op(op) = t else {
                if ops.len() < 300_000 {
                    ops.push(t);
                }
                continue;
            };
            match op.as_slice() {
                b"endcodespacerange" => {
                    for p in ops.chunks(2) {
                        if let [Obj::Str(lo), Obj::Str(hi)] = p {
                            if (1..=4).contains(&lo.len()) && lo.len() == hi.len() {
                                cm.space.push((lo.len(), be(lo), be(hi)));
                            }
                        }
                    }
                }
                b"endbfchar" => {
                    for p in ops.chunks(2) {
                        if let [Obj::Str(src), dst] = p {
                            if !(1..=4).contains(&src.len()) {
                                continue;
                            }
                            let s = match dst {
                                Obj::Str(d) => utf16(d),
                                Obj::Name(nm) => {
                                    glyph_char(nm).map(String::from).unwrap_or_default()
                                }
                                _ => continue,
                            };
                            cm.single.insert((src.len(), be(src)), s);
                        }
                    }
                }
                b"endbfrange" => {
                    for p in ops.chunks(3) {
                        if let [Obj::Str(lo), Obj::Str(hi), dst] = p {
                            if !(1..=4).contains(&lo.len()) || cm.ranges.len() > 100_000 {
                                continue;
                            }
                            let (l, h) = (be(lo), be(hi));
                            if h < l {
                                continue;
                            }
                            let d = match dst {
                                Obj::Str(d) => RangeDst::Base(
                                    d.chunks(2)
                                        .map(|c| {
                                            if c.len() == 2 {
                                                u16::from_be_bytes([c[0], c[1]])
                                            } else {
                                                u16::from(c[0])
                                            }
                                        })
                                        .collect(),
                                ),
                                Obj::Arr(a) => RangeDst::List(
                                    a.iter()
                                        .map(|x| match x {
                                            Obj::Str(s) => utf16(s),
                                            _ => String::new(),
                                        })
                                        .collect(),
                                ),
                                _ => continue,
                            };
                            cm.ranges.push((lo.len(), l, h, d));
                        }
                    }
                }
                _ => {}
            }
            ops.clear();
        }
        cm
    }

    fn lookup(&self, len: usize, code: u32) -> Option<String> {
        if let Some(s) = self.single.get(&(len, code)) {
            return Some(s.clone());
        }
        for (l, lo, hi, d) in &self.ranges {
            if *l == len && (*lo..=*hi).contains(&code) {
                let k = code - lo;
                return match d {
                    RangeDst::List(v) => v.get(k as usize).cloned(),
                    RangeDst::Base(units) => {
                        let mut u = units.clone();
                        let last = u.last_mut()?;
                        *last = last.wrapping_add(k as u16);
                        Some(
                            char::decode_utf16(u)
                                .map(|r| r.unwrap_or('\u{fffd}'))
                                .collect(),
                        )
                    }
                };
            }
        }
        None
    }
}

struct Font {
    tounicode: Option<CMap>,
    /// Type0 (composite) font: 2-byte codes unless a codespace says otherwise
    wide: bool,
    simple: [char; 256],
    /// glyph widths in 1/1000 em by code (simple) or CID (Type0 Identity)
    widths: HashMap<u32, f64>,
    default_width: f64,
    /// false when the font gave no widths at all (then gaps are guessed)
    known_widths: bool,
}

/// What a shown string did: total width (1/1000 em), glyphs, single-byte
/// spaces (for Tw).
struct Shown {
    width: f64,
    glyphs: usize,
    spaces: usize,
}

impl Font {
    fn code_len(&self, rest: &[u8]) -> usize {
        // Simple fonts always use 1-byte codes, whatever a ToUnicode
        // codespace claims (Acrobat writes <0000><FFFF> for TrueType).
        if !self.wide {
            return 1;
        }
        if let Some(cm) = &self.tounicode {
            if !cm.space.is_empty() {
                for len in 1..=4.min(rest.len()) {
                    let c = be(&rest[..len]);
                    if cm
                        .space
                        .iter()
                        .any(|(l, lo, hi)| *l == len && (*lo..=*hi).contains(&c))
                    {
                        return len;
                    }
                }
            }
        }
        2.min(rest.len())
    }

    fn decode(&self, s: &[u8], out: &mut Out) -> Shown {
        let mut shown = Shown {
            width: 0.0,
            glyphs: 0,
            spaces: 0,
        };
        let mut i = 0;
        while i < s.len() {
            let n = self.code_len(&s[i..]).max(1);
            let code = be(&s[i..i + n]);
            i += n;
            shown.glyphs += 1;
            if n == 1 && code == 32 {
                shown.spaces += 1;
            }
            shown.width += self
                .widths
                .get(&code)
                .copied()
                .unwrap_or(self.default_width);
            let mapped = self.tounicode.as_ref().and_then(|cm| cm.lookup(n, code));
            match mapped {
                Some(t) => out.text(&t),
                None if !self.wide && code < 256 => {
                    let c = self.simple[code as usize];
                    if c != '\0' {
                        let mut b = [0u8; 4];
                        out.text(c.encode_utf8(&mut b));
                    }
                }
                None => {}
            }
        }
        shown
    }
}

const WIN_HIGH: [u16; 32] = [
    0x20ac, 0, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017d, 0, 0, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc,
    0x2122, 0x0161, 0x203a, 0x0153, 0, 0x017e, 0x0178,
];

fn base_encoding(name: Option<&[u8]>) -> [char; 256] {
    let mut t = ['\0'; 256];
    for (i, c) in t.iter_mut().enumerate().take(0x7f).skip(0x20) {
        *c = char::from(i as u8);
    }
    match name {
        Some(b"StandardEncoding") => {
            t[0x27] = '\u{2019}';
            t[0x60] = '\u{2018}';
            for (code, name) in [
                (0xa1u8, "exclamdown"),
                (0xa4, "fraction"),
                (0xa9, "quotesingle"),
                (0xaa, "quotedblleft"),
                (0xae, "fi"),
                (0xaf, "fl"),
                (0xb1, "endash"),
                (0xb7, "bullet"),
                (0xb9, "quotedblbase"),
                (0xba, "quotedblright"),
                (0xbc, "ellipsis"),
                (0xd0, "emdash"),
                (0xe1, "AE"),
                (0xf1, "ae"),
                (0xf5, "dotlessi"),
                (0xf8, "oslash"),
                (0xfa, "oe"),
                (0xfb, "germandbls"),
            ] {
                if let Some(c) = glyph_char(name.as_bytes()) {
                    t[code as usize] = c;
                }
            }
        }
        _ => {
            // WinAnsi (also used, approximately, for MacRoman and unknown).
            for (i, u) in WIN_HIGH.iter().enumerate() {
                if *u != 0 {
                    t[0x80 + i] = char::from_u32(u32::from(*u)).unwrap_or('\0');
                }
            }
            for (i, c) in t.iter_mut().enumerate().skip(0xa0) {
                *c = char::from_u32(i as u32).unwrap_or('\0');
            }
            t[0xa0] = ' ';
            t[0xad] = '-';
        }
    }
    t
}

/// Unicode for an Adobe glyph name: `uniXXXX`, `uXXXX[XX]`, single-letter
/// names, and the common Latin names (enough for English and Croatian).
fn glyph_char(name: &[u8]) -> Option<char> {
    let n = std::str::from_utf8(name).ok()?;
    let base = n.split('.').next().unwrap_or(n);
    if base.len() == 1 {
        return base.chars().next();
    }
    if let Some(h) = base.strip_prefix("uni") {
        if h.len() >= 4 {
            return u32::from_str_radix(&h[..4], 16)
                .ok()
                .and_then(char::from_u32);
        }
    }
    if let Some(h) = base.strip_prefix('u') {
        if (4..=6).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return u32::from_str_radix(h, 16).ok().and_then(char::from_u32);
        }
    }
    const NAMES: &[(&str, char)] = &[
        ("space", ' '),
        ("exclam", '!'),
        ("quotedbl", '"'),
        ("numbersign", '#'),
        ("dollar", '$'),
        ("percent", '%'),
        ("ampersand", '&'),
        ("quotesingle", '\''),
        ("quoteright", '\u{2019}'),
        ("quoteleft", '\u{2018}'),
        ("parenleft", '('),
        ("parenright", ')'),
        ("asterisk", '*'),
        ("plus", '+'),
        ("comma", ','),
        ("hyphen", '-'),
        ("minus", '\u{2212}'),
        ("period", '.'),
        ("slash", '/'),
        ("zero", '0'),
        ("one", '1'),
        ("two", '2'),
        ("three", '3'),
        ("four", '4'),
        ("five", '5'),
        ("six", '6'),
        ("seven", '7'),
        ("eight", '8'),
        ("nine", '9'),
        ("colon", ':'),
        ("semicolon", ';'),
        ("less", '<'),
        ("equal", '='),
        ("greater", '>'),
        ("question", '?'),
        ("at", '@'),
        ("bracketleft", '['),
        ("backslash", '\\'),
        ("bracketright", ']'),
        ("asciicircum", '^'),
        ("underscore", '_'),
        ("grave", '`'),
        ("braceleft", '{'),
        ("bar", '|'),
        ("braceright", '}'),
        ("asciitilde", '~'),
        ("endash", '\u{2013}'),
        ("emdash", '\u{2014}'),
        ("quotedblleft", '\u{201c}'),
        ("quotedblright", '\u{201d}'),
        ("quotedblbase", '\u{201e}'),
        ("quotesinglbase", '\u{201a}'),
        ("bullet", '\u{2022}'),
        ("ellipsis", '\u{2026}'),
        ("fi", '\u{fb01}'),
        ("fl", '\u{fb02}'),
        ("ff", '\u{fb00}'),
        ("ffi", '\u{fb03}'),
        ("ffl", '\u{fb04}'),
        ("exclamdown", '\u{a1}'),
        ("fraction", '\u{2044}'),
        ("dotlessi", '\u{131}'),
        ("germandbls", '\u{df}'),
        ("AE", '\u{c6}'),
        ("ae", '\u{e6}'),
        ("oslash", '\u{f8}'),
        ("oe", '\u{153}'),
        ("OE", '\u{152}'),
        ("Ccaron", '\u{10c}'),
        ("ccaron", '\u{10d}'),
        ("Cacute", '\u{106}'),
        ("cacute", '\u{107}'),
        ("Scaron", '\u{160}'),
        ("scaron", '\u{161}'),
        ("Zcaron", '\u{17d}'),
        ("zcaron", '\u{17e}'),
        ("Dcroat", '\u{110}'),
        ("dcroat", '\u{111}'),
        ("Eacute", '\u{c9}'),
        ("eacute", '\u{e9}'),
        ("Adieresis", '\u{c4}'),
        ("adieresis", '\u{e4}'),
        ("Odieresis", '\u{d6}'),
        ("odieresis", '\u{f6}'),
        ("Udieresis", '\u{dc}'),
        ("udieresis", '\u{fc}'),
        ("degree", '\u{b0}'),
        ("copyright", '\u{a9}'),
        ("registered", '\u{ae}'),
        ("trademark", '\u{2122}'),
        ("Euro", '\u{20ac}'),
        ("section", '\u{a7}'),
        ("paragraph", '\u{b6}'),
        ("periodcentered", '\u{b7}'),
        ("nbspace", ' '),
    ];
    NAMES.iter().find(|(k, _)| *k == base).map(|(_, c)| *c)
}

impl<'a> Doc<'a> {
    fn font(&self, fd: &Obj) -> Font {
        let empty = Dict::new();
        let d = fd.dict().unwrap_or(&empty);
        let subtype = dget(d, "Subtype").and_then(Obj::name).unwrap_or(b"");
        let wide = subtype == b"Type0";
        let tounicode = dget(d, "ToUnicode")
            .map(|t| self.deref(t))
            .and_then(|s| self.stream_bytes(&s))
            .map(|b| CMap::parse(&b));
        let enc = dget(d, "Encoding").map(|e| self.deref(e));
        let mut simple = match &enc {
            Some(Obj::Name(n)) => base_encoding(Some(n)),
            Some(Obj::Dict(ed)) => base_encoding(dget(ed, "BaseEncoding").and_then(Obj::name)),
            _ => base_encoding(None),
        };
        if let Some(Obj::Dict(ed)) = &enc {
            if let Some(Obj::Arr(diffs)) = dget(ed, "Differences").map(|x| self.deref(x)) {
                let mut code = 0usize;
                for item in diffs {
                    match item {
                        Obj::Int(c) => code = c.clamp(0, 255) as usize,
                        Obj::Name(n) => {
                            if code < 256 {
                                simple[code] = glyph_char(&n).unwrap_or('\0');
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut widths = HashMap::new();
        let mut default_width = if wide { 1000.0 } else { 0.0 };
        let mut known_widths = false;
        if wide {
            let desc = match dget(d, "DescendantFonts").map(|x| self.deref(x)) {
                Some(Obj::Arr(a)) => a.first().map(|x| self.deref(x)),
                _ => None,
            };
            if let Some(Obj::Dict(cid)) = desc {
                if let Some(dw) = dget(&cid, "DW")
                    .map(|x| self.deref(x))
                    .and_then(|x| x.num())
                {
                    default_width = dw;
                }
                if let Some(Obj::Arr(w)) = dget(&cid, "W").map(|x| self.deref(x)) {
                    known_widths = true;
                    let mut k = 0;
                    while k < w.len() && widths.len() < 200_000 {
                        let first = w[k].num().unwrap_or(0.0).max(0.0) as u32;
                        match w.get(k + 1).map(|x| self.deref(x)) {
                            Some(Obj::Arr(list)) => {
                                for (j, x) in list.iter().enumerate() {
                                    widths.insert(first + j as u32, x.num().unwrap_or(0.0));
                                }
                                k += 2;
                            }
                            Some(last) => {
                                let last = last.num().unwrap_or(0.0).max(0.0) as u32;
                                let v = w.get(k + 2).and_then(Obj::num).unwrap_or(0.0);
                                for c in first..=last.min(first.saturating_add(65_535)) {
                                    widths.insert(c, v);
                                }
                                k += 3;
                            }
                            None => break,
                        }
                    }
                }
            }
        } else if let Some(Obj::Arr(w)) = dget(d, "Widths").map(|x| self.deref(x)) {
            known_widths = true;
            let first = dget(d, "FirstChar")
                .and_then(Obj::num)
                .unwrap_or(0.0)
                .max(0.0) as u32;
            for (j, x) in w.iter().enumerate().take(256) {
                widths.insert(first + j as u32, self.deref(x).num().unwrap_or(0.0));
            }
            if let Some(Obj::Dict(fd)) = dget(d, "FontDescriptor").map(|x| self.deref(x)) {
                if let Some(mw) = dget(&fd, "MissingWidth").and_then(Obj::num) {
                    default_width = mw;
                }
            }
        }
        if !known_widths {
            default_width = 500.0;
        }
        Font {
            tounicode,
            wide,
            simple,
            widths,
            default_width,
            known_widths,
        }
    }
}

// ---- text output -------------------------------------------------------

struct Out {
    s: String,
    chars: usize,
    max: usize,
    /// characters that came from text (not from gaps)
    shown: usize,
}

impl Out {
    fn full(&self) -> bool {
        self.chars >= self.max
    }
    fn text(&mut self, t: &str) {
        for c in t.chars() {
            if self.full() {
                return;
            }
            let c = match c {
                '\u{0}'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{fffd}' => continue,
                '\t' | '\u{a0}' => ' ',
                '\u{fb00}' => {
                    self.text("ff");
                    continue;
                }
                '\u{fb01}' => {
                    self.text("fi");
                    continue;
                }
                '\u{fb02}' => {
                    self.text("fl");
                    continue;
                }
                '\u{fb03}' => {
                    self.text("ffi");
                    continue;
                }
                '\u{fb04}' => {
                    self.text("ffl");
                    continue;
                }
                c => c,
            };
            if c == ' ' && (self.s.is_empty() || self.s.ends_with([' ', '\n'])) {
                continue;
            }
            self.s.push(c);
            self.chars += 1;
            self.shown += 1;
        }
    }
    fn gap(&mut self, c: char) {
        if self.s.is_empty() {
            return;
        }
        if c == '\n' && self.s.ends_with(' ') {
            self.s.pop();
            self.chars -= 1;
        }
        if !self.s.ends_with([' ', '\n']) {
            self.s.push(c);
            self.chars += 1;
        }
    }
}

struct Run<'d, 'a> {
    doc: &'d Doc<'a>,
    out: Out,
    ops: usize,
    forms: HashSet<u32>,
    /// user-space pen position after the last shown glyph, and the font
    /// size there (user units)
    last: Option<(f64, f64, f64)>,
}

/// Text state that matters for positions (PDF 1.7 section 9.3).
#[derive(Clone, Copy)]
struct TextState {
    tm: [f64; 6],
    /// line start, in text space
    lx: f64,
    ly: f64,
    /// pen x, in text space (relative to the line start's origin)
    px: f64,
    fs: f64,
    tc: f64,
    tw: f64,
    th: f64,
    tl: f64,
}

impl TextState {
    fn new() -> Self {
        TextState {
            tm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            lx: 0.0,
            ly: 0.0,
            px: 0.0,
            fs: 0.0,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            tl: 0.0,
        }
    }
    fn user(&self, x: f64, y: f64, ctm: &[f64; 6]) -> (f64, f64) {
        let m = &self.tm;
        let (tx, ty) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        (
            ctm[0] * tx + ctm[2] * ty + ctm[4],
            ctm[1] * tx + ctm[3] * ty + ctm[5],
        )
    }
    fn size(&self, ctm: &[f64; 6]) -> f64 {
        let m = &self.tm;
        let sy = (m[2] * ctm[0] + m[3] * ctm[2]).hypot(m[2] * ctm[1] + m[3] * ctm[3]);
        (self.fs * sy).abs()
    }
    fn newline(&mut self, tx: f64, ty: f64) {
        self.lx += tx;
        self.ly += ty;
        self.px = self.lx;
    }
}

fn mul(a: &[f64; 6], b: &[f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

impl Run<'_, '_> {
    /// Run a Form XObject (also used for annotation appearances).
    fn form(&mut self, f: &Obj, res: Option<&Dict>, depth: usize, ctm: &[f64; 6]) {
        let Obj::Stream(fd, ..) = f else { return };
        if dget(fd, "Subtype").and_then(Obj::name) != Some(b"Form") {
            return;
        }
        let Some(bytes) = self.doc.stream_bytes(f) else {
            return;
        };
        let own = match dget(fd, "Resources").map(|r| self.doc.deref(r)) {
            Some(Obj::Dict(r)) => Some(r),
            _ => None,
        };
        let m = match dget(fd, "Matrix").map(|r| self.doc.deref(r)) {
            Some(Obj::Arr(a)) if a.len() == 6 => {
                let v: Vec<f64> = a.iter().map(|x| x.num().unwrap_or(0.0)).collect();
                [v[0], v[1], v[2], v[3], v[4], v[5]]
            }
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        self.content(&bytes, own.as_ref().or(res), depth + 1, mul(&m, ctm));
    }

    /// Visible annotation appearances (filled-in form fields, free text),
    /// as pdftotext shows them.
    fn annotations(&mut self, page: &Dict, res: Option<&Dict>) {
        let Some(Obj::Arr(annots)) = dget(page, "Annots").map(|a| self.doc.deref(a)) else {
            return;
        };
        for a in annots.iter().take(500) {
            if self.out.full() {
                return;
            }
            let Obj::Dict(ad) = self.doc.deref(a) else {
                continue;
            };
            let before = self.out.shown;
            self.annotation(&ad, res);
            // A text or choice field whose appearance stream is empty (the
            // viewer is asked to build it, /NeedAppearances): show /V.
            let field = dget(&ad, "FT").and_then(Obj::name);
            let hidden = dget(&ad, "F").and_then(Obj::num).unwrap_or(0.0) as i64 & 2 != 0;
            if self.out.shown == before && !hidden && matches!(field, Some(b"Tx" | b"Ch")) {
                if let Some(Obj::Str(v)) = dget(&ad, "V") {
                    let v = match (&self.doc.key, a) {
                        (Some(key), Obj::Ref(n)) => {
                            let gen = self.doc.gens.get(n).copied().unwrap_or(0);
                            crate::pdf_crypt::rc4(&crate::pdf_crypt::object_key(key, *n, gen), v)
                        }
                        _ => v.clone(),
                    };
                    self.out.gap('\n');
                    self.out.text(&text_string(&v));
                }
            }
        }
    }

    fn annotation(&mut self, ad: &Dict, res: Option<&Dict>) {
        let flags = dget(ad, "F").and_then(Obj::num).unwrap_or(0.0) as i64;
        if flags & 2 != 0 {
            return; // hidden
        }
        let Some(Obj::Dict(ap)) = dget(ad, "AP").map(|x| self.doc.deref(x)) else {
            return;
        };
        let normal = match dget(&ap, "N").map(|x| self.doc.deref(x)) {
            Some(s @ Obj::Stream(..)) => s,
            Some(Obj::Dict(states)) => {
                let Some(state) = dget(ad, "AS").and_then(Obj::name) else {
                    return;
                };
                let Ok(state) = std::str::from_utf8(state) else {
                    return;
                };
                match dget(&states, state).map(|x| self.doc.deref(x)) {
                    Some(s @ Obj::Stream(..)) => s,
                    _ => return,
                }
            }
            _ => return,
        };
        let r = match dget(ad, "Rect").map(|x| self.doc.deref(x)) {
            Some(Obj::Arr(v)) if v.len() == 4 => {
                let n: Vec<f64> = v.iter().map(|x| x.num().unwrap_or(0.0)).collect();
                [1.0, 0.0, 0.0, 1.0, n[0].min(n[2]), n[1].min(n[3])]
            }
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        self.out.gap('\n');
        self.form(&normal, res, 0, &r);
    }

    /// Decide line break / word gap from where this glyph run starts versus
    /// where the previous one ended, then show it and move the pen.
    fn show(&mut self, s: &[u8], font: &Font, ts: &mut TextState, ctm: &[f64; 6]) {
        let (ux, uy) = ts.user(ts.px, ts.ly, ctm);
        let size = ts.size(ctm).max(0.01);
        if let Some((lx, ly, lsize)) = self.last {
            let em = size.max(lsize);
            if (uy - ly).abs() > 0.5 * em {
                self.out.gap('\n');
            } else {
                let dx = ux - lx;
                let wide_gap = if font.known_widths { 0.15 } else { 1.0 };
                if dx > wide_gap * em || dx < -em {
                    self.out.gap(' ');
                }
            }
        }
        let shown = font.decode(s, &mut self.out);
        let adv = (shown.width / 1000.0 * ts.fs
            + shown.glyphs as f64 * ts.tc
            + shown.spaces as f64 * ts.tw)
            * ts.th;
        ts.px += adv;
        let (ex, ey) = ts.user(ts.px, ts.ly, ctm);
        self.last = Some((ex, ey, size));
    }

    fn content(&mut self, data: &[u8], res: Option<&Dict>, depth: usize, ctm0: [f64; 6]) {
        if depth > 8 {
            return;
        }
        let mut fonts: HashMap<Vec<u8>, Rc<Font>> = HashMap::new();
        let font_dict = res.and_then(|r| dget(r, "Font")).map(|f| self.doc.deref(f));
        let xobjects = res
            .and_then(|r| dget(r, "XObject"))
            .map(|f| self.doc.deref(f));
        let mut font: Option<Rc<Font>> = None;
        let mut lx = Lex::new(data, 0, false);
        let mut stack: Vec<Obj> = Vec::new();
        let mut ts = TextState::new();
        let mut ctm = ctm0;
        let mut saved: Vec<([f64; 6], TextState, Option<Rc<Font>>)> = Vec::new();
        while let Some(tok) = lx.token(0) {
            if self.ops == 0 || self.out.full() {
                return;
            }
            self.ops -= 1;
            let Obj::Op(op) = tok else {
                if stack.len() < 64 {
                    stack.push(tok);
                }
                continue;
            };
            let num = |i: usize| -> f64 {
                stack
                    .len()
                    .checked_sub(i)
                    .and_then(|k| stack.get(k))
                    .and_then(Obj::num)
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.0)
            };
            let six = || [num(6), num(5), num(4), num(3), num(2), num(1)];
            match op.as_slice() {
                b"q" => {
                    if saved.len() < 64 {
                        saved.push((ctm, ts, font.clone()));
                    }
                }
                b"Q" => {
                    if let Some((c, t, f)) = saved.pop() {
                        ctm = c;
                        // Text position is not part of the graphics state;
                        // keep the matrices, restore font/size/spacing.
                        ts = TextState {
                            tm: ts.tm,
                            lx: ts.lx,
                            ly: ts.ly,
                            px: ts.px,
                            ..t
                        };
                        font = f;
                    }
                }
                b"cm" => ctm = mul(&six(), &ctm),
                b"BT" => {
                    ts.tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                    ts.lx = 0.0;
                    ts.ly = 0.0;
                    ts.px = 0.0;
                }
                b"Tc" => ts.tc = num(1),
                b"Tw" => ts.tw = num(1),
                b"Tz" => ts.th = num(1) / 100.0,
                b"TL" => ts.tl = num(1),
                b"Tf" => {
                    ts.fs = num(1);
                    let name = stack
                        .len()
                        .checked_sub(2)
                        .and_then(|k| stack.get(k))
                        .and_then(Obj::name)
                        .map(<[u8]>::to_vec);
                    font = name.and_then(|n| {
                        if let Some(f) = fonts.get(&n) {
                            return Some(f.clone());
                        }
                        let fd = font_dict
                            .as_ref()
                            .and_then(Obj::dict)
                            .and_then(|fd| dget(fd, std::str::from_utf8(&n).ok()?))
                            .map(|f| self.doc.deref(f))?;
                        let f = Rc::new(self.doc.font(&fd));
                        if fonts.len() < 256 {
                            fonts.insert(n, f.clone());
                        }
                        Some(f)
                    });
                }
                b"Td" => ts.newline(num(2), num(1)),
                b"TD" => {
                    ts.tl = -num(1);
                    ts.newline(num(2), num(1));
                }
                b"Tm" => {
                    ts.tm = six();
                    ts.lx = 0.0;
                    ts.ly = 0.0;
                    ts.px = 0.0;
                }
                b"T*" => ts.newline(0.0, -ts.tl),
                b"Tj" | b"'" | b"\"" => {
                    if op == b"\"" {
                        ts.tw = num(3);
                        ts.tc = num(2);
                    }
                    if op != b"Tj" {
                        ts.newline(0.0, -ts.tl);
                    }
                    if let (Some(Obj::Str(s)), Some(f)) = (stack.last(), font.clone()) {
                        self.show(s, &f, &mut ts, &ctm);
                    }
                }
                b"TJ" => {
                    if let (Some(Obj::Arr(items)), Some(f)) = (stack.last(), font.clone()) {
                        for it in items {
                            match it {
                                Obj::Str(s) => self.show(s, &f, &mut ts, &ctm),
                                other => {
                                    let n = other.num().filter(|v| v.is_finite()).unwrap_or(0.0);
                                    ts.px -= n / 1000.0 * ts.fs * ts.th;
                                    if !f.known_widths && n < -150.0 {
                                        self.out.gap(' ');
                                    }
                                }
                            }
                        }
                    }
                }
                b"Do" => {
                    let name = stack.last().and_then(Obj::name).map(<[u8]>::to_vec);
                    let Some(name) = name else {
                        stack.clear();
                        continue;
                    };
                    let entry = xobjects
                        .as_ref()
                        .and_then(Obj::dict)
                        .and_then(|x| dget(x, std::str::from_utf8(&name).ok()?))
                        .cloned();
                    if let Some(Obj::Ref(n)) = &entry {
                        if !self.forms.insert(*n) {
                            stack.clear();
                            continue;
                        }
                    }
                    if let Some(form) = entry.map(|e| self.doc.deref(&e)) {
                        self.form(&form, res, depth, &ctm);
                    }
                }
                b"BI" => {
                    // Inline image: skip to "ID", then raw bytes up to " EI".
                    let mut found = false;
                    while let Some(t) = lx.token(0) {
                        if matches!(&t, Obj::Op(o) if o == b"ID") {
                            found = true;
                            break;
                        }
                    }
                    if found {
                        let mut q = lx.p + 1;
                        loop {
                            match find(data, b"EI", q) {
                                Some(k)
                                    if is_ws(data[k - 1])
                                        && data.get(k + 2).is_none_or(|b| is_ws(*b)) =>
                                {
                                    lx.p = k + 2;
                                    break;
                                }
                                Some(k) => q = k + 2,
                                None => {
                                    lx.p = data.len();
                                    break;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            stack.clear();
        }
    }
}

fn page_content(doc: &Doc, page: &Dict) -> Vec<u8> {
    let mut content = Vec::new();
    match dget(page, "Contents").map(|c| doc.deref(c)) {
        Some(s @ Obj::Stream(..)) => content = doc.stream_bytes(&s).unwrap_or_default(),
        Some(Obj::Arr(parts)) => {
            for part in parts.iter().take(10_000) {
                if let Some(b) = doc.stream_bytes(&doc.deref(part)) {
                    content.extend_from_slice(&b);
                    content.push(b'\n');
                }
            }
        }
        _ => {}
    }
    content
}

fn open<'a>(bytes: &'a [u8], limits: Limits) -> Result<(Doc<'a>, Dict, Option<Dict>), Outcome> {
    let head = &bytes[..bytes.len().min(1024)];
    if find(head, b"%PDF-", 0).is_none() {
        return Err(Outcome::NotPdf);
    }
    let mut doc = Doc::new(bytes, limits);
    let dicts = doc.trailers();
    if doc.unlock(&dicts).is_err() {
        return Err(Outcome::Encrypted);
    }
    let Some(root) = doc.root(&dicts) else {
        return Err(Outcome::NoPage);
    };
    let Some((page, res)) = doc.first_page(&root) else {
        return Err(Outcome::NoPage);
    };
    Ok((doc, page, res))
}

/// The decoded content stream(s) of the first page (for `galion-one
/// pdf-text --content`, a debugging aid).
pub fn first_page_content(bytes: &[u8], limits: Limits) -> Result<Vec<u8>, Outcome> {
    let (doc, page, _) = open(bytes, limits)?;
    Ok(page_content(&doc, &page))
}

/// Text of the first page, in content-stream order, at most
/// `limits.max_chars` characters.
pub fn first_page_text(bytes: &[u8], limits: Limits) -> Outcome {
    let (doc, page, res) = match open(bytes, limits) {
        Ok(x) => x,
        Err(o) => return o,
    };
    let content = page_content(&doc, &page);
    let mut run = Run {
        doc: &doc,
        out: Out {
            s: String::new(),
            chars: 0,
            max: limits.max_chars,
            shown: 0,
        },
        ops: limits.max_ops,
        forms: HashSet::new(),
        last: None,
    };
    run.content(&content, res.as_ref(), 0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    run.annotations(&page, res.as_ref());
    let text = run.out.s.trim().to_string();
    if text.is_empty() {
        Outcome::NoText
    } else {
        Outcome::Text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: Limits = Limits {
        max_chars: 1_000_000,
        ..DEFAULT_LIMITS
    };

    fn text(bytes: &[u8]) -> String {
        match first_page_text(bytes, BIG) {
            Outcome::Text(t) => t,
            other => panic!("no text: {other:?}"),
        }
    }

    fn words(s: &str) -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A PDF from numbered object bodies (1-based), with a classic xref.
    fn pdf(objs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for o in offs {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
        let mut v = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        v.extend_from_slice(data);
        v.extend_from_slice(b"\nendstream");
        v
    }

    fn s(x: &str) -> Vec<u8> {
        x.as_bytes().to_vec()
    }

    /// Catalog, Pages, Page with font F1 = obj 5 and content = obj 4.
    fn one_page(content: Vec<u8>, font: Vec<u8>, extra: &[Vec<u8>]) -> Vec<u8> {
        let mut objs = vec![
            s("<< /Type /Catalog /Pages 2 0 R >>"),
            s("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            s("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /X1 6 0 R >> >> /Contents 4 0 R >>"),
            content,
            font,
        ];
        objs.extend_from_slice(extra);
        pdf(&objs)
    }

    const HELV: &str =
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

    fn fixture(name: &str) -> Vec<u8> {
        let p = format!("{}/tests/fixtures/pdf/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
    }

    const EXPECT: &str = "Galion One PDF test Dobar dan, Zagreb! first page text. Second line: 12345 hermes nerve cortex.";

    #[test]
    fn ghostscript_fixtures_plain_objstm_and_rc4() {
        for name in [
            "gs-plain.pdf",
            "gs-objstm.pdf",
            "gs-rc4-r2.pdf",
            "gs-rc4-r3.pdf",
        ] {
            let t = text(&fixture(name));
            assert_eq!(words(&t), EXPECT, "{name}");
            assert_eq!(t.lines().count(), 3, "{name}: {t:?}");
            assert!(!t.contains("PAGE TWO"), "{name}");
        }
        assert_eq!(
            first_page_text(&fixture("gs-userpw.pdf"), BIG),
            Outcome::Encrypted
        );
    }

    #[test]
    fn type0_font_with_tounicode_bfchar_and_bfrange() {
        let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
            1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
            2 beginbfchar <0001> <0044> <0002> <010D> endbfchar\n\
            2 beginbfrange <0010> <0012> <0061> <0020> <0022> [<0161> <017E> <0111>] endbfrange\n\
            endcmap CMapName currentdict /CMap defineresource pop end end";
        // "D" "č" "a" "b" "c" " "? no: codes 0001 0002 0010 0011 0012 0020 0021 0022
        let content = stream(
            "",
            b"BT /F1 12 Tf 72 700 Td <00010002001000110012> Tj <002000210022> Tj ET",
        );
        let font = s("<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [7 0 R] /ToUnicode 8 0 R >>");
        let extra = [
            s("<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] >>"),
            s("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X /DW 500 >>"),
            stream("", cmap.as_bytes()),
        ];
        let t = text(&one_page(content, font, &extra));
        assert_eq!(t, "Dčabcšžđ");
    }

    #[test]
    fn simple_font_differences_and_word_gaps_from_widths() {
        let font = s("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /FirstChar 65 /Widths [500 500 500] /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /Ccaron /scaron /uni0111] >> >>");
        // Each glyph is 0.5 em = 6 units at 12 pt. "AB" then a jump of 20
        // units (a gap) then "C"; then a 1-unit kern (no gap) and "A".
        let content = stream("", b"BT /F1 12 Tf 1 0 0 1 72 700 Tm (AB) Tj 32 0 Td (C) Tj 7 0 Td (A) Tj 0 -14 Td (\\101\\102) Tj ET");
        let t = text(&one_page(content, font, &[s("null")]));
        assert_eq!(t, "\u{10c}\u{161} \u{111}\u{10c}\n\u{10c}\u{161}");
    }

    #[test]
    fn tj_offsets_literal_escapes_and_quote_operators() {
        let content = stream(
            "",
            b"BT /F1 10 Tf 12 TL 50 700 Td [(Hel) -20 (lo) -400 (world)] TJ (sec\\(ond\\) line) ' (\\124hird) ' ET",
        );
        let t = text(&one_page(content, s(HELV), &[s("null")]));
        // Helvetica has no /Widths here, so gaps come from TJ offsets only.
        assert_eq!(t, "Hello world\nsec(ond) line\nThird");
    }

    #[test]
    fn filters_flate_stored_hex_a85_runlength_lzw() {
        let raw = b"BT /F1 12 Tf 72 700 Td (Filtered text) Tj ET";
        // zlib header + one stored DEFLATE block (no compressor needed).
        let mut flate = vec![0x78, 0x01, 0x01];
        flate.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        flate.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        flate.extend_from_slice(raw);
        let hex: String = raw.iter().map(|b| format!("{b:02X}")).collect::<String>() + ">";
        let mut rl = Vec::new();
        for chunk in raw.chunks(100) {
            rl.push((chunk.len() - 1) as u8);
            rl.extend_from_slice(chunk);
        }
        rl.push(128);
        let a85 = a85_encode(raw);
        let lz = lzw_encode(raw);
        for (filter, data) in [
            ("/Filter /FlateDecode", flate.clone()),
            ("/Filter /ASCIIHexDecode", hex.clone().into_bytes()),
            ("/Filter /ASCII85Decode", a85.clone()),
            ("/Filter /RunLengthDecode", rl.clone()),
            ("/Filter /LZWDecode", lz.clone()),
            ("/Filter [/ASCIIHexDecode /FlateDecode]", {
                let h: String = flate.iter().map(|b| format!("{b:02x}")).collect();
                h.into_bytes()
            }),
        ] {
            let t = text(&one_page(stream(filter, &data), s(HELV), &[s("null")]));
            assert_eq!(t, "Filtered text", "{filter}");
        }
    }

    #[test]
    fn lzw_pdf_spec_example_and_long_roundtrip() {
        // ISO 32000-1, 7.4.4.2 example: "-----A---B" -> 80 0B 60 50 22 0C 0C 85 01
        let out = lzw(
            &[0x80, 0x0b, 0x60, 0x50, 0x22, 0x0c, 0x0c, 0x85, 0x01],
            1000,
            true,
        );
        assert_eq!(out, b"-----A---B");
        // Long input crosses the 9->10->11->12 bit widths and a table reset.
        let mut x: u32 = 7;
        let data: Vec<u8> = (0..60_000)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                b"ab cd\n"[(x >> 16) as usize % 6]
            })
            .collect();
        assert_eq!(lzw(&lzw_encode(&data), 1 << 20, true), data);
    }

    #[test]
    fn ascii85_known_vector() {
        // "Man " -> 9jqo^ (the classic example)
        assert_eq!(ascii85(b"9jqo^~>"), b"Man ");
        assert_eq!(ascii85(b"<~z~>"), [0, 0, 0, 0]);
        assert_eq!(ascii85(&a85_encode(b"hello world")), b"hello world");
    }

    #[test]
    fn form_xobject_inherited_resources_nested_page_tree() {
        let objs = vec![
            s("<< /Type /Catalog /Pages 2 0 R >>"),
            s("<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 5 0 R >> /XObject << /X1 6 0 R >> >> >>"),
            s("<< /Type /Pages /Parent 2 0 R /Kids [9 0 R 4 0 R] /Count 1 >>"),
            s("<< /Type /Page /Parent 3 0 R /Contents 7 0 R >>"),
            s(HELV),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 100 100]", b"BT /F1 12 Tf 0 0 Td (inside the form) Tj ET"),
            stream("", b"BT /F1 12 Tf 72 700 Td (before) Tj ET q 1 0 0 1 72 600 cm /X1 Do Q"),
            s("null"),
            s("<< /Type /Pages /Kids [] /Count 0 >>"),
        ];
        assert_eq!(text(&pdf(&objs)), "before\ninside the form");
    }

    #[test]
    fn incremental_update_later_object_wins_and_no_xref_needed() {
        let mut v = one_page(
            stream("", b"BT /F1 12 Tf 72 700 Td (old) Tj ET"),
            s(HELV),
            &[s("null")],
        );
        v.extend_from_slice(b"4 0 obj\n");
        v.extend_from_slice(&stream("", b"BT /F1 12 Tf 72 700 Td (new) Tj ET"));
        v.extend_from_slice(b"\nendobj\n");
        assert_eq!(text(&v), "new");
        // No xref, no trailer: the catalog is found by /Type.
        let bare = b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
            2 0 obj << /Type /Pages /Kids [3 0 R] >> endobj\n\
            3 0 obj << /Type /Page /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >> endobj\n\
            4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n\
            5 0 obj << /Length 999 >> stream\nBT /F1 9 Tf (bare file) Tj ET\nendstream endobj\n";
        assert_eq!(text(bare), "bare file");
    }

    #[test]
    fn inline_image_bytes_are_skipped() {
        let mut c = b"BT /F1 12 Tf 72 700 Td (A) Tj ET BI /W 4 /H 1 /BPC 8 /CS /G ID ".to_vec();
        c.extend_from_slice(b"(X) Tj\x00\xff");
        c.extend_from_slice(b" EI BT /F1 12 Tf 72 680 Td (B) Tj ET");
        assert_eq!(
            text(&one_page(stream("", &c), s(HELV), &[s("null")])),
            "A\nB"
        );
    }

    #[test]
    fn annotation_appearance_and_empty_field_value() {
        let objs = vec![
            s("<< /Type /Catalog /Pages 2 0 R >>"),
            s("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            s("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R /Annots [6 0 R 7 0 R 9 0 R] >>"),
            stream("", b"BT /F1 12 Tf 72 700 Td (Name:) Tj ET"),
            s(HELV),
            s("<< /Type /Annot /Subtype /Widget /FT /Tx /Rect [100 690 200 710] /V <FEFF0041006C006900630065> /AP << /N 8 0 R >> >>"),
            s("<< /Type /Annot /Subtype /FreeText /Rect [100 600 200 620] /F 2 /AP << /N 10 0 R >> >>"),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 100 20]", b"/Tx BMC EMC"),
            s("<< /Type /Annot /Subtype /FreeText /Rect [100 500 200 520] /AP << /N 10 0 R >> >>"),
            stream("/Type /XObject /Subtype /Form /BBox [0 0 100 20] /Resources << /Font << /F1 5 0 R >> >>", b"BT /F1 10 Tf 2 5 Td (a note) Tj ET"),
        ];
        // Field value shown (empty appearance), hidden annot skipped, the
        // visible FreeText appearance shown.
        assert_eq!(text(&pdf(&objs)), "Name:\nAlice\na note");
    }

    #[test]
    fn hostile_structures_end_quickly() {
        // Page tree cycle.
        let cyc = pdf(&[
            s("<< /Type /Catalog /Pages 2 0 R >>"),
            s("<< /Type /Pages /Kids [2 0 R 3 0 R] >>"),
            s("<< /Type /Pages /Kids [2 0 R] >>"),
        ]);
        assert_eq!(first_page_text(&cyc, BIG), Outcome::NoPage);
        // Form that draws itself.
        let selfdraw = one_page(
            stream("", b"/X1 Do BT /F1 12 Tf (ok) Tj ET"),
            s(HELV),
            &[stream(
                "/Subtype /Form /Resources << /XObject << /X1 6 0 R >> >>",
                b"/X1 Do",
            )],
        );
        assert_eq!(text(&selfdraw), "ok");
        // 200k nested arrays in content and in an object: no stack overflow.
        let mut deep = b"BT /F1 12 Tf ".to_vec();
        deep.extend(std::iter::repeat_n(b'[', 200_000));
        deep.extend_from_slice(b" (deep) Tj ET");
        let _ = first_page_text(&one_page(stream("", &deep), s(HELV), &[s("null")]), BIG);
        let mut deep_obj = b"%PDF-1.4\n1 0 obj ".to_vec();
        deep_obj.extend(std::iter::repeat_n(b'<', 200_000));
        let _ = first_page_text(&deep_obj, BIG);
        // max_chars and max_ops caps.
        let mut many = b"BT /F1 12 Tf ".to_vec();
        for _ in 0..50_000 {
            many.extend_from_slice(b"(abcdefghij) Tj ");
        }
        many.extend_from_slice(b"ET");
        let doc = one_page(stream("", &many), s(HELV), &[s("null")]);
        let Outcome::Text(t) = first_page_text(&doc, DEFAULT_LIMITS) else {
            panic!()
        };
        assert_eq!(t.chars().count(), DEFAULT_LIMITS.max_chars);
        let small = Limits {
            max_ops: 100,
            ..BIG
        };
        let Outcome::Text(t) = first_page_text(&doc, small) else {
            panic!()
        };
        assert!(t.len() < 1000);
        // Not a PDF at all.
        assert_eq!(first_page_text(b"GIF89a", BIG), Outcome::NotPdf);
        assert_eq!(first_page_text(b"%PDF-1.4\n", BIG), Outcome::NoPage);
    }

    #[test]
    fn mutated_inputs_never_panic() {
        let mut seeds: Vec<Vec<u8>> = [
            "gs-plain.pdf",
            "gs-objstm.pdf",
            "gs-rc4-r3.pdf",
            "gs-userpw.pdf",
        ]
        .iter()
        .map(|n| fixture(n))
        .collect();
        seeds.push(one_page(
            stream("", b"BT /F1 12 Tf 72 700 Td [(a) -300 (b)] TJ ET /X1 Do"),
            s("<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /ToUnicode 7 0 R /DescendantFonts [<< /W [1 [500 600] 5 9 300] >>] >>"),
            &[
                stream("/Subtype /Form", b"BT /F1 3 Tf <0001> Tj ET"),
                stream("", b"begincodespacerange <00> <FF> endcodespacerange 1 beginbfrange <00> <FF> <0041> endbfrange"),
            ],
        ));
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        let mut rnd = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let limits = Limits {
            max_stream: 1 << 20,
            max_total: 4 << 20,
            max_ops: 100_000,
            max_chars: 2000,
        };
        let start = std::time::Instant::now();
        let mut runs = 0;
        for seed in &seeds {
            for _ in 0..1500 {
                let mut m = seed.clone();
                for _ in 0..1 + rnd() % 8 {
                    let len = m.len().max(1);
                    match rnd() % 5 {
                        0 => {
                            let i = rnd() as usize % len;
                            if i < m.len() {
                                m[i] = rnd() as u8;
                            }
                        }
                        1 => m.truncate(rnd() as usize % len),
                        2 => {
                            let i = rnd() as usize % len;
                            let junk: &[u8] = [
                                &b"<<"[..],
                                b">>",
                                b"[",
                                b"(",
                                b"0 0 R",
                                b"stream",
                                b"obj",
                                b"\xff\xfe",
                            ][rnd() as usize % 8];
                            let i = i.min(m.len());
                            m.splice(i..i, junk.iter().copied());
                        }
                        3 => {
                            let i = rnd() as usize % len;
                            let j = (i + rnd() as usize % 64).min(m.len());
                            if i < j {
                                m.drain(i..j);
                            }
                        }
                        _ => {
                            let i = rnd() as usize % len;
                            let digit = b'0' + (rnd() % 10) as u8;
                            if i < m.len() && m[i].is_ascii_digit() {
                                m[i] = digit;
                            }
                        }
                    }
                }
                let _ = first_page_text(&m, limits);
                runs += 1;
            }
        }
        eprintln!("{runs} mutated PDFs in {:?}", start.elapsed());
        assert!(start.elapsed() < std::time::Duration::from_secs(120));
    }

    fn a85_encode(data: &[u8]) -> Vec<u8> {
        let mut out = b"<~".to_vec();
        for chunk in data.chunks(4) {
            let mut b = [0u8; 4];
            b[..chunk.len()].copy_from_slice(chunk);
            let mut v = u32::from_be_bytes(b);
            let mut d = [0u8; 5];
            for k in (0..5).rev() {
                d[k] = (v % 85) as u8 + b'!';
                v /= 85;
            }
            out.extend_from_slice(&d[..chunk.len() + 1]);
        }
        out.extend_from_slice(b"~>");
        out
    }

    /// Test-only LZW encoder (PDF flavour, EarlyChange 1), so the decoder
    /// can be exercised across code widths.
    fn lzw_encode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let (mut acc, mut n) = (0u64, 0u32);
        let mut emit = |code: u32, width: u32, out: &mut Vec<u8>| {
            acc = (acc << width) | u64::from(code);
            n += width;
            while n >= 8 {
                out.push((acc >> (n - 8)) as u8);
                n -= 8;
            }
        };
        let mut dict: HashMap<Vec<u8>, u32> = HashMap::new();
        let reset = |d: &mut HashMap<Vec<u8>, u32>| {
            d.clear();
            for b in 0..=255u8 {
                d.insert(vec![b], u32::from(b));
            }
        };
        reset(&mut dict);
        let mut next = 258u32;
        let mut width = 9;
        emit(256, width, &mut out);
        let mut w: Vec<u8> = Vec::new();
        for &c in data {
            let mut wc = w.clone();
            wc.push(c);
            if dict.contains_key(&wc) {
                w = wc;
                continue;
            }
            emit(dict[&w], width, &mut out);
            dict.insert(wc, next);
            next += 1;
            if next + 1 > (1 << width) && width < 12 {
                width += 1;
            }
            if next >= 4094 {
                emit(256, width, &mut out);
                reset(&mut dict);
                next = 258;
                width = 9;
            }
            w = vec![c];
        }
        if !w.is_empty() {
            emit(dict[&w], width, &mut out);
        }
        emit(257, width, &mut out);
        if n > 0 {
            out.push((acc << (8 - n)) as u8);
        }
        out
    }
}
