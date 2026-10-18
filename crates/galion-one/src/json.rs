// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Small std-only JSON reader/writer shared by models_refresh and set-model.
// Numbers are kept as their raw text so a rewrite does not change them.
// `dump_pretty` writes the way Python's json.dumps(indent=2) does
// (ensure_ascii, "key": value, two-space indent), so files the old Python
// helpers wrote keep their exact form.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn lit(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("bad json at {}", self.i))
        }
    }

    fn value(&mut self, depth: u32) -> Result<Json, String> {
        if depth > 64 {
            return Err("json too deep".into());
        }
        self.ws();
        match self.b.get(self.i) {
            None => Err("unexpected end of json".into()),
            Some(b'n') => self.lit("null", Json::Null),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        return Ok(Json::Arr(out));
                    }
                    out.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {}
                        _ => return Err(format!("bad array at {}", self.i)),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        return Ok(Json::Obj(out));
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return Err(format!("expected colon at {}", self.i));
                    }
                    self.i += 1;
                    out.push((k, self.value(depth + 1)?));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {}
                        _ => return Err(format!("bad object at {}", self.i)),
                    }
                }
            }
            Some(_) => {
                let start = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                if self.i == start {
                    Err(format!("bad json at {}", self.i))
                } else {
                    Ok(Json::Num(
                        String::from_utf8_lossy(&self.b[start..self.i]).into_owned(),
                    ))
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let s = self
            .b
            .get(self.i..self.i + 4)
            .and_then(|s| std::str::from_utf8(s).ok())
            .ok_or("bad unicode escape")?;
        let v = u32::from_str_radix(s, 16).map_err(|_| "bad unicode escape".to_string())?;
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        if self.b.get(self.i) != Some(&b'"') {
            return Err(format!("expected string at {}", self.i));
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.b.get(self.i).ok_or("unterminated string")?;
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or("unterminated escape")?;
                    self.i += 1;
                    let ch = match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.b.get(self.i..self.i + 2) == Some(b"\\u")
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000
                                    + ((cp - 0xD800) << 10)
                                    + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            char::from_u32(cp).unwrap_or('\u{fffd}')
                        }
                        other => other as char,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "invalid utf-8 in string".to_string())
    }
}

pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err("trailing data after json".into());
    }
    Ok(v)
}

pub fn get<'a>(obj: &'a Json, key: &str) -> Option<&'a Json> {
    match obj {
        Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

pub fn text_of(v: Option<&Json>) -> String {
    match v {
        Some(Json::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Python json.dumps string form (ensure_ascii).
pub fn dump_str(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for u in c.encode_utf16(&mut units) {
                    o.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Python json.dumps(value, indent=2) form.
pub fn dump_pretty(v: &Json) -> String {
    let mut out = String::new();
    pretty(v, 0, &mut out);
    out
}

fn pretty(v: &Json, level: usize, out: &mut String) {
    let pad = |n: usize| "  ".repeat(n);
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => out.push_str(n),
        Json::Str(s) => out.push_str(&dump_str(s)),
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(kv) if kv.is_empty() => out.push_str("{}"),
        Json::Arr(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&pad(level + 1));
                pretty(item, level + 1, out);
            }
            out.push('\n');
            out.push_str(&pad(level));
            out.push(']');
        }
        Json::Obj(kv) => {
            out.push_str("{\n");
            for (i, (k, item)) in kv.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&pad(level + 1));
                out.push_str(&dump_str(k));
                out.push_str(": ");
                pretty(item, level + 1, out);
            }
            out.push('\n');
            out.push_str(&pad(level));
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_pretty_like_python() {
        let v = parse(r#"{"modes":{"plan":"a/b","action":""},"saved":[{"name":"x","model":"y\u00e9"}],"n":1.50,"e":[],"o":{},"t":true,"z":null}"#).unwrap();
        let want = "{\n  \"modes\": {\n    \"plan\": \"a/b\",\n    \"action\": \"\"\n  },\n  \"saved\": [\n    {\n      \"name\": \"x\",\n      \"model\": \"y\\u00e9\"\n    }\n  ],\n  \"n\": 1.50,\n  \"e\": [],\n  \"o\": {},\n  \"t\": true,\n  \"z\": null\n}";
        assert_eq!(dump_pretty(&v), want);
    }
}
