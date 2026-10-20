// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

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

#[cfg(test)]
mod tests {
    use super::to_json_compat;

    /// Source of this file above the test module.
    fn library_body() -> &'static str {
        const SRC: &str = include_str!("jsonout.rs");
        let Some((body, _)) = SRC.split_once("mod tests") else {
            return SRC;
        };
        body
    }

    /// Child processes of this test. A library call must not leave a python
    /// child behind.
    fn child_cmdlines() -> Vec<String> {
        let me = std::process::id();
        let mut out = Vec::new();
        let Ok(proc) = std::fs::read_dir("/proc") else {
            return out;
        };
        for ent in proc.flatten() {
            let name = ent.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(ent.path().join("stat")) else {
                continue;
            };
            let Some(end) = stat.rfind(')') else {
                continue;
            };
            let mut fields = stat[end + 1..].split_whitespace();
            let _state = fields.next();
            let Some(ppid) = fields.next() else {
                continue;
            };
            let Ok(ppid_n) = ppid.parse::<u32>() else {
                continue;
            };
            if ppid_n != me {
                continue;
            }
            let Ok(raw) = std::fs::read(ent.path().join("cmdline")) else {
                continue;
            };
            let cmd = String::from_utf8_lossy(&raw).replace('\0', " ");
            out.push(format!("{name} {cmd}"));
        }
        out.sort();
        out
    }

    /// Socket fds held by this process. Directory reads are dropped before
    /// return, so they do not show up here.
    fn socket_targets() -> Vec<String> {
        let mut out = Vec::new();
        let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
            return out;
        };
        for entry in dir.flatten() {
            let Ok(target) = std::fs::read_link(entry.path()) else {
                continue;
            };
            let text = target.to_string_lossy();
            if text.starts_with("socket:") {
                out.push(text.into_owned());
            }
        }
        out.sort();
        out
    }

    #[test]
    fn compact_cpython_separators_do_not_spawn_python_or_open_a_socket() {
        let body = library_body();
        assert!(
            body.contains("(\",\", \":\")"),
            "compact separators are already documented on the library path"
        );
        for banned in [
            "std::process",
            "Command::",
            "python3",
            "/usr/bin/python",
            "std::net",
            "TcpListener",
            "TcpStream",
            "UdpSocket",
            "UnixListener",
            "UnixStream",
            "socket2",
        ] {
            assert!(!body.contains(banned), "{banned}");
        }

        assert!(std::path::Path::new("/proc/self/fd").is_dir());
        assert!(std::path::Path::new("/proc/self/task").is_dir());
        let before_kids = child_cmdlines();
        let before_socks = socket_targets();

        // Desk-hermes calls `to_json_compat(..., false)`, the documented
        // compact separators `(",", ":")`, not the CPython default
        // `(", ", ": ")`.
        let value = serde_json::json!({
            "ok": true,
            "items": [1, 2],
            "note": {"k": "v"}
        });
        let compact = to_json_compat(&value, false);
        let spaced = to_json_compat(&value, true);

        assert_eq!(compact, r#"{"ok":true,"items":[1,2],"note":{"k":"v"}}"#);
        assert_eq!(
            spaced,
            r#"{"ok": true, "items": [1, 2], "note": {"k": "v"}}"#
        );
        assert!(!compact.contains(", "));
        assert!(!compact.contains(": "));
        assert_ne!(compact, spaced);

        let after_kids = child_cmdlines();
        let after_socks = socket_targets();
        assert_eq!(before_kids, after_kids);
        assert_eq!(before_socks, after_socks);
        assert!(after_kids.iter().all(|cmd| {
            let folded = cmd.to_ascii_lowercase();
            !folded.contains("python")
        }));
    }
}
