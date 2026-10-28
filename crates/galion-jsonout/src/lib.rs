// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-jsonout: JSON writer that reproduces CPython `json.dumps`
//! byte-for-byte. Shared by desk-hermes and link-watch:
//! - `ensure_ascii=True` (default) escapes every non-ASCII code point as
//!   `\uXXXX` (surrogate pairs above U+FFFF);
//! - compact separators `(",", ":")` when `spaced` is false (desk-hermes), or
//!   the CPython default `(", ", ": ")` when it is true (link-watch).
//!
//! Split out of `galion-common` (its former `serde-json` feature) by slot 325
//! (swarm/release-repro-unify, 2026-10-27): an optional feature on a crate that
//! every release binary links made `cargo build -p tuner` and the 12-binary
//! release build compile galion-common differently (D-312-4). A separate crate
//! is only built where it is used, so galion-common has no features at all.

use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

pub struct PyFormatter {
    pub spaced: bool,
}

impl serde_json::ser::Formatter for PyFormatter {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let bytes = fragment.as_bytes();
        let mut start = 0usize;
        let mut esc = String::new();
        for (i, ch) in fragment.char_indices() {
            if ch.is_ascii() {
                continue;
            }
            if start < i {
                writer.write_all(&bytes[start..i])?;
            }
            esc.clear();
            galion_common::json::push_ascii_escaped(&mut esc, ch);
            writer.write_all(esc.as_bytes())?;
            start = i + ch.len_utf8();
        }
        if start < bytes.len() {
            writer.write_all(&bytes[start..])?;
        }
        Ok(())
    }

    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.sep(writer, first)
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.sep(writer, first)
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        writer.write_all(if self.spaced { b": " } else { b":" })
    }
}

impl PyFormatter {
    fn sep<W: ?Sized + Write>(&self, writer: &mut W, first: bool) -> io::Result<()> {
        if first {
            Ok(())
        } else if self.spaced {
            writer.write_all(b", ")
        } else {
            writer.write_all(b",")
        }
    }
}

/// Serialise a value the way CPython's `json.dumps` would.
pub fn to_json_compat(value: &Value, spaced: bool) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PyFormatter { spaced });
    // Serialising a `Value` into a Vec cannot fail (no I/O error, no
    // non-string map keys), and the formatter only writes valid UTF-8.
    if value.serialize(&mut ser).is_err() {
        return String::new();
    }
    String::from_utf8(buf).unwrap_or_default()
}

/// CPython truthiness of a decoded JSON value (`x or default`).
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// CPython `str(x)` for a JSON value.
pub fn json_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) => "[]".to_string(),
        Value::Object(_) => "{}".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cpython_bytes() {
        let v = json!({"a": [1, "é", null, true], "b": {"c": "😀"}});
        assert_eq!(
            to_json_compat(&v, false),
            "{\"a\":[1,\"\\u00e9\",null,true],\"b\":{\"c\":\"\\ud83d\\ude00\"}}"
        );
        assert_eq!(
            to_json_compat(&v, true),
            "{\"a\": [1, \"\\u00e9\", null, true], \"b\": {\"c\": \"\\ud83d\\ude00\"}}"
        );
        assert!(!truthy(&json!(0)));
        assert!(truthy(&json!("x")));
        assert_eq!(json_str(&json!(null)), "None");
    }
}
