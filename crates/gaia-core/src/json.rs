// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! A tiny, dependency-free JSON value with a stable (insertion-ordered)
//! object layout, so simulation output is byte-for-byte deterministic.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// An empty object.
    pub fn obj() -> Json {
        Json::Obj(Vec::new())
    }

    /// Builder: append a field to an object (no-op on other variants).
    pub fn with(mut self, key: &str, value: impl Into<Json>) -> Json {
        if let Json::Obj(fields) = &mut self {
            fields.push((key.to_string(), value.into()));
        }
        self
    }

    /// Look up a field of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// Compact rendering.
    pub fn compact(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, None, 0);
        s
    }

    /// Two-space indented rendering.
    pub fn pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, Some(2), 0);
        s
    }

    fn write(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Str(s) => escape_into(out, s),
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent, depth + 1);
                    item.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push(']');
            }
            Json::Obj(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent, depth + 1);
                    escape_into(out, k);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    v.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push('}');
            }
        }
    }
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(w) = indent {
        out.push('\n');
        for _ in 0..w * depth {
            out.push(' ');
        }
    }
}

/// Write `s` as a JSON string literal.
pub fn escape_into(out: &mut String, s: &str) {
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

impl From<bool> for Json {
    fn from(b: bool) -> Self {
        Json::Bool(b)
    }
}
impl From<i64> for Json {
    fn from(n: i64) -> Self {
        Json::Num(n)
    }
}
impl From<u64> for Json {
    fn from(n: u64) -> Self {
        Json::Num(i64::try_from(n).unwrap_or(i64::MAX))
    }
}
impl From<u32> for Json {
    fn from(n: u32) -> Self {
        Json::Num(i64::from(n))
    }
}
impl From<u8> for Json {
    fn from(n: u8) -> Self {
        Json::Num(i64::from(n))
    }
}
impl From<usize> for Json {
    fn from(n: usize) -> Self {
        Json::Num(i64::try_from(n).unwrap_or(i64::MAX))
    }
}
impl From<&str> for Json {
    fn from(s: &str) -> Self {
        Json::Str(s.to_string())
    }
}
impl From<String> for Json {
    fn from(s: String) -> Self {
        Json::Str(s)
    }
}
impl From<Vec<Json>> for Json {
    fn from(v: Vec<Json>) -> Self {
        Json::Arr(v)
    }
}
impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(o: Option<T>) -> Self {
        o.map_or(Json::Null, Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_stable_compact_and_escapes() {
        let j = Json::obj()
            .with("a", 1i64)
            .with("b", "x\"y\n")
            .with("c", vec![Json::Bool(true), Json::Null]);
        assert_eq!(j.compact(), r#"{"a":1,"b":"x\"y\n","c":[true,null]}"#);
        assert!(j.pretty().contains("\n  \"a\": 1"));
        assert_eq!(j.get("a").and_then(Json::as_i64), Some(1));
    }
}
