// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! A small, strict JSON reader and writer (RFC 8259) for the console API
//! payloads. std only. Depth and size are bounded by the caller's body cap
//! and [`MAX_DEPTH`].

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Deepest nesting accepted (API payloads are 3-4 levels deep).
pub const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    /// Keys in insertion order (the API's order is meaningful for display).
    Obj(Vec<(String, Value)>),
}

static NULL: Value = Value::Null;

impl Value {
    /// Member of an object, or `Null`.
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Obj(m) => m
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Compact serialisation (same shape as `JSON.stringify(v)`).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self);
        out
    }
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => {
            if n.is_finite() && n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                let _ = write!(out, "{}", *n as i64);
            } else if n.is_finite() {
                let _ = write!(out, "{n}");
            } else {
                out.push_str("null");
            }
        }
        Value::Str(s) => write_str(out, s),
        Value::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, x);
            }
            out.push(']');
        }
        Value::Obj(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(out, k);
                out.push(':');
                write_value(out, x);
            }
            out.push('}');
        }
    }
}

/// Write `s` as a JSON string literal.
pub fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub what: &'static str,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "json: {} at byte {}", self.what, self.at)
    }
}

pub fn parse(input: &[u8]) -> Result<Value, ParseError> {
    let mut p = Parser { s: input, i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing data"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &'static str) -> ParseError {
        ParseError { at: self.i, what }
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.s.get(self.i) {
            self.i += 1;
        }
    }

    fn lit(&mut self, word: &[u8], v: Value) -> Result<Value, ParseError> {
        if self.s[self.i..].starts_with(word) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("bad literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("too deep"));
        }
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end")),
            Some(b'n') => self.lit(b"null", Value::Null),
            Some(b't') => self.lit(b"true", Value::Bool(true)),
            Some(b'f') => self.lit(b"false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return Err(self.err("expected , or ]")),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut m: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(self.err("expected key"));
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return Err(self.err("expected :"));
                    }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    // Last duplicate wins, like JSON.parse.
                    if let Some(slot) = m.iter_mut().find(|(x, _)| *x == k) {
                        slot.1 = v;
                    } else {
                        m.push((k, v));
                    }
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return Err(self.err("expected , or }")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected byte")),
        }
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        let digits = |p: &mut Self| {
            let s = p.i;
            while let Some(b'0'..=b'9') = p.s.get(p.i) {
                p.i += 1;
            }
            p.i - s
        };
        match self.s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return Err(self.err("bad number")),
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            if digits(self) == 0 {
                return Err(self.err("bad fraction"));
            }
        }
        if let Some(b'e' | b'E') = self.s.get(self.i) {
            self.i += 1;
            if let Some(b'+' | b'-') = self.s.get(self.i) {
                self.i += 1;
            }
            if digits(self) == 0 {
                return Err(self.err("bad exponent"));
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.err("utf8"))?;
        text.parse::<f64>()
            .map(Value::Num)
            .map_err(|_| self.err("bad number"))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let h = self
            .s
            .get(self.i..self.i + 4)
            .ok_or(self.err("short \\u"))?;
        let t = std::str::from_utf8(h).map_err(|_| self.err("bad \\u"))?;
        let v = u32::from_str_radix(t, 16).map_err(|_| self.err("bad \\u"))?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1; // opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&b) = self.s.get(self.i) else {
                return Err(self.err("unterminated string"));
            };
            match b {
                b'"' => {
                    self.i += 1;
                    return String::from_utf8(out).map_err(|_| self.err("invalid utf8"));
                }
                b'\\' => {
                    self.i += 1;
                    let Some(&e) = self.s.get(self.i) else {
                        return Err(self.err("bad escape"));
                    };
                    self.i += 1;
                    let c = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.s.get(self.i..self.i + 2) != Some(b"\\u") {
                                    return Err(self.err("lone surrogate"));
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.err("bad surrogate"));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(self.err("lone surrogate"));
                            } else {
                                hi
                            };
                            char::from_u32(cp).ok_or(self.err("bad code point"))?
                        }
                        _ => return Err(self.err("bad escape")),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                0x00..=0x1f => return Err(self.err("control byte in string")),
                _ => {
                    out.push(b);
                    self.i += 1;
                }
            }
        }
    }
}

/// Build an object from pairs (for request bodies).
pub fn obj(pairs: &[(&str, Value)]) -> Value {
    Value::Obj(
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    )
}

/// Count of distinct keys, handy in tests.
pub fn keys(v: &Value) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    if let Value::Obj(o) = v {
        for (k, _) in o {
            *m.entry(k.clone()).or_insert(0) += 1;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let src = br#"{"a":[1,2.5,-3e2,true,false,null],"b":{"c":"x\"\\\n\u00e9\ud83d\ude00"}}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.get("a").as_array().len(), 6);
        assert_eq!(v.get("a").as_array()[2].as_f64(), Some(-300.0));
        assert_eq!(v.get("b").get("c").as_str(), Some("x\"\\\n\u{e9}\u{1F600}"));
        let again = parse(v.to_json().as_bytes()).unwrap();
        assert_eq!(v, again);
        assert_eq!(parse(b"[1,-300]").unwrap().to_json(), "[1,-300]");
    }

    #[test]
    fn rejects() {
        for bad in [
            &b""[..],
            b"{",
            b"[1,]",
            b"{\"a\" 1}",
            b"01",
            b"1.",
            b"\"\x01\"",
            b"\"\\ud800\"",
            b"nul",
            b"{} x",
            b"\"\\q\"",
        ] {
            assert!(parse(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
        let deep = "[".repeat(MAX_DEPTH + 2);
        assert!(parse(deep.as_bytes()).is_err());
    }

    #[test]
    fn missing_is_null() {
        let v = parse(b"{\"a\":1}").unwrap();
        assert!(v.get("zz").is_null());
        assert!(v.get("a").get("b").is_null());
        assert_eq!(keys(&v).len(), 1);
        assert_eq!(
            obj(&[("cmd", Value::Str("health".into()))]).to_json(),
            r#"{"cmd":"health"}"#
        );
    }
}
