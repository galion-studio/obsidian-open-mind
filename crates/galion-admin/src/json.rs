// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Minimal JSON for the audit log: escape strings, and parse one flat object whose values are
//! strings or unsigned integers. That is all the audit format uses; nested values are rejected.

/// Escape `s` as the inside of a JSON string. Control characters become `\u00XX`.
pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Num(u64),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Num(_) => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Str(_) => None,
        }
    }
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let s = self.b.get(self.i..self.i + 4).ok_or("short \\u escape")?;
        let s = std::str::from_utf8(s).map_err(|_| "bad \\u escape")?;
        self.i += 4;
        u32::from_str_radix(s, 16).map_err(|_| "bad \\u escape".to_string())
    }
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = *self.b.get(self.i).ok_or("unterminated string")?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("unterminated escape")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xd800..0xdc00).contains(&cp)
                                && self.b.get(self.i..self.i + 2) == Some(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000
                                    + ((cp - 0xd800) << 10)
                                    + (lo.wrapping_sub(0xdc00) & 0x3ff);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    }
                }
                c if c < 0x20 => return Err("raw control character in string".into()),
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "string is not UTF-8".into())
    }
    fn number(&mut self) -> Result<u64, String> {
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("expected unsigned integer at byte {start}"))
    }
}

/// Parse one flat JSON object of string / unsigned-integer values, keeping key order.
pub fn parse_flat(s: &str) -> Result<Vec<(String, Value)>, String> {
    let mut p = P {
        b: s.as_bytes(),
        i: 0,
    };
    p.eat(b'{')?;
    let mut out = Vec::new();
    p.ws();
    if p.b.get(p.i) == Some(&b'}') {
        p.i += 1;
    } else {
        loop {
            let k = p.string()?;
            p.eat(b':')?;
            p.ws();
            let v = match p.b.get(p.i) {
                Some(b'"') => Value::Str(p.string()?),
                Some(c) if c.is_ascii_digit() => Value::Num(p.number()?),
                _ => return Err(format!("unsupported value for key {k:?}")),
            };
            out.push((k, v));
            p.ws();
            match p.b.get(p.i) {
                Some(b',') => p.i += 1,
                Some(b'}') => {
                    p.i += 1;
                    break;
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", p.i)),
            }
        }
    }
    p.ws();
    if p.i != p.b.len() {
        return Err("trailing data after object".into());
    }
    Ok(out)
}

/// Look up a key in a parsed object.
pub fn get<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_escapes() {
        let nasty = "a\"b\\c\nd\te\u{1}f\u{7f}ž🚀";
        let line = format!("{{\"k\":\"{}\",\"n\":42}}", escape(nasty));
        assert!(!line.contains('\n'));
        let o = parse_flat(&line).unwrap();
        assert_eq!(get(&o, "k").unwrap().as_str(), Some(nasty));
        assert_eq!(get(&o, "n").unwrap().as_u64(), Some(42));
    }

    #[test]
    fn surrogate_pair() {
        let o = parse_flat(r#"{"k":"\ud83d\ude80"}"#).unwrap();
        assert_eq!(get(&o, "k").unwrap().as_str(), Some("🚀"));
    }

    #[test]
    fn rejects_nested_and_garbage() {
        assert!(parse_flat(r#"{"k":{"x":1}}"#).is_err());
        assert!(parse_flat(r#"{"k":"x"} trailing"#).is_err());
        assert!(parse_flat(r#"{"k":-1}"#).is_err());
        assert!(parse_flat("{\"k\":\"raw\u{1}\"}").is_err());
        assert!(parse_flat(r#"{"k":"x""#).is_err());
        assert_eq!(parse_flat("{}").unwrap(), vec![]);
    }
}
