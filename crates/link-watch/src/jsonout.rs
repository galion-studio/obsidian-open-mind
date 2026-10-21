// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! JSON writer that reproduces CPython `json.dumps` byte-for-byte.
//!
//! link-watch uses the CPython default separators `(", ", ": ")` (it calls
//! `json.dumps(row)` with no `separators=` argument), so `spaced` is set to
//! `true` there. Non-ASCII code points are escaped as `\uXXXX` exactly like
//! `ensure_ascii=True` (the CPython default). The implementation is shared
//! with desk-hermes in `galion_common::jsonout`.

pub use galion_common::jsonout::{json_str, to_json_compat, truthy};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cpython_default_layout_and_ensure_ascii() {
        let v = json!({"at": "x", "laptop": {"name": "Café"}, "events": ["a", "b"], "ok": true});
        assert_eq!(
            to_json_compat(&v, true),
            r#"{"at": "x", "laptop": {"name": "Caf\u00e9"}, "events": ["a", "b"], "ok": true}"#
        );
        assert_eq!(to_json_compat(&json!(["😀"]), true), r#"["\ud83d\ude00"]"#);
    }
}
