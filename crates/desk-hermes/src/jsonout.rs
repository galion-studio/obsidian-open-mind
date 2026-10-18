//! JSON writer that reproduces CPython `json.dumps` byte-for-byte:
//! - `ensure_ascii=True` (default) escapes every non-ASCII code point as
//!   `\uXXXX` (surrogate pairs above U+FFFF);
//! - compact separators `(",", ":")` for desk-hermes, or the CPython default
//!   `(", ", ": ")` when `spaced` is set (link-watch uses the default).

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
        for (i, ch) in fragment.char_indices() {
            if ch.is_ascii() {
                continue;
            }
            if start < i {
                writer.write_all(&bytes[start..i])?;
            }
            let cp = ch as u32;
            if cp <= 0xFFFF {
                write!(writer, "\\u{cp:04x}")?;
            } else {
                let c = cp - 0x1_0000;
                let hi = 0xD800 + (c >> 10);
                let lo = 0xDC00 + (c & 0x3FF);
                write!(writer, "\\u{hi:04x}\\u{lo:04x}")?;
            }
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
        if first {
            Ok(())
        } else if self.spaced {
            writer.write_all(b", ")
        } else {
            writer.write_all(b",")
        }
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        if first {
            Ok(())
        } else if self.spaced {
            writer.write_all(b", ")
        } else {
            writer.write_all(b",")
        }
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        if self.spaced {
            writer.write_all(b": ")
        } else {
            writer.write_all(b":")
        }
    }
}

/// Serialise a value the way CPython's `json.dumps` would.
pub fn to_json_compat(value: &Value, spaced: bool) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, PyFormatter { spaced });
    value.serialize(&mut ser).expect("serialise json");
    String::from_utf8(buf).expect("utf-8 json")
}

/// CPython `str(x)` for a JSON value, used by `str(...)` calls in the sources.
pub fn json_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) => "[]".to_string(),
        Value::Object(_) => "{}".to_string(),
    }
}

/// CPython truthiness for `x or default` expressions.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
