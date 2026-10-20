// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! JSON writer that reproduces CPython `json.dumps` byte-for-byte (feature
//! `serde-json`). Moved here from the identical copies in desk-hermes and
//! link-watch:
//! - `ensure_ascii=True` (default) escapes every non-ASCII code point as
//!   `\uXXXX` (surrogate pairs above U+FFFF);
//! - compact separators `(",", ":")` when `spaced` is false (desk-hermes), or
//!   the CPython default `(", ", ": ")` when it is true (link-watch).
//! - floats as CPython `repr(float)` (`1e+16`, `1e-05`, `1.5e-07`), see
//!   [`crate::json::float_repr`].

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
            crate::json::push_ascii_escaped(&mut esc, ch);
            writer.write_all(esc.as_bytes())?;
            start = i + ch.len_utf8();
        }
        if start < bytes.len() {
            writer.write_all(&bytes[start..])?;
        }
        Ok(())
    }

    /// CPython `repr(float)` layout (`1.5e-07`, `1e-05`), not serde_json's
    /// (`1.5e-7`, `0.00001`). See [`crate::json::float_repr`].
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        writer.write_all(crate::json::float_repr(value).as_bytes())
    }

    fn write_f32<W>(&mut self, writer: &mut W, value: f32) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.write_f64(writer, f64::from(value))
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

    /// CPython: json.dumps([1e16, 1e-05, 0.0001, 2.0, -0.0, 8.888000000937703e+46])
    /// The last value is the fuzz finding in docs/fuzzing.md.
    #[test]
    fn cpython_floats() {
        let v = json!([1e16, 0.00001, 0.0001, 2.0, -0.0, 8.888000000937703e46]);
        assert_eq!(
            to_json_compat(&v, true),
            "[1e+16, 1e-05, 0.0001, 2.0, -0.0, 8.888000000937703e+46]"
        );
        assert_eq!(
            to_json_compat(&json!({"t": 1.5e-7}), false),
            "{\"t\":1.5e-07}"
        );
    }
}
