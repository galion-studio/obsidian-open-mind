// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! JSON writer that reproduces CPython `json.dumps` byte-for-byte. The
//! implementation is shared with link-watch in the `galion-jsonout` crate
//! (formerly galion-common feature `serde-json`):
//! - `ensure_ascii=True` (default) escapes every non-ASCII code point as
//!   `\uXXXX` (surrogate pairs above U+FFFF);
//! - compact separators `(",", ":")` for desk-hermes, or the CPython default
//!   `(", ", ": ")` when `spaced` is set (link-watch uses the default).

pub use galion_jsonout::{json_str, to_json_compat, truthy};

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

#[cfg(test)]
mod value_tests {
    // From swarm/desk-linkwatch (a54c792), kept next to swarm/common's test above.
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn compact_and_spaced_separators() {
        let value = json!({"ok": true, "items": [1, 2], "note": {"k": "v"}});
        assert_eq!(
            to_json_compat(&value, false),
            r#"{"ok":true,"items":[1,2],"note":{"k":"v"}}"#
        );
        assert_eq!(
            to_json_compat(&value, true),
            r#"{"ok": true, "items": [1, 2], "note": {"k": "v"}}"#
        );
        assert_eq!(to_json_compat(&json!([]), false), "[]");
        assert_eq!(to_json_compat(&json!({}), true), "{}");
    }

    #[test]
    fn non_ascii_is_escaped_like_ensure_ascii() {
        assert_eq!(to_json_compat(&json!("Café"), false), r#""Caf\u00e9""#);
        assert_eq!(to_json_compat(&json!("😀"), false), r#""\ud83d\ude00""#);
        assert_eq!(to_json_compat(&json!("§ end"), false), r#""\u00a7 end""#);
        assert_eq!(to_json_compat(&json!("q\"\n"), false), r#""q\"\n""#);
    }

    #[test]
    fn python_str_and_truthiness() {
        assert_eq!(json_str(&Value::Null), "None");
        assert_eq!(json_str(&json!(true)), "True");
        assert_eq!(json_str(&json!(false)), "False");
        assert_eq!(json_str(&json!(3)), "3");
        assert_eq!(json_str(&json!("s")), "s");
        assert_eq!(json_str(&json!([1])), "[]");
        assert_eq!(json_str(&json!({"a": 1})), "{}");
        for falsy in [
            json!(null),
            json!(false),
            json!(0),
            json!(0.0),
            json!(""),
            json!([]),
            json!({}),
        ] {
            assert!(!truthy(&falsy), "{falsy}");
        }
        for t in [
            json!(true),
            json!(1),
            json!(-0.5),
            json!("x"),
            json!([0]),
            json!({"a": 0}),
        ] {
            assert!(truthy(&t), "{t}");
        }
    }
}
