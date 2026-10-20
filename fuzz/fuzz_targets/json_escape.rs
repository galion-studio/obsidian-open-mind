// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-common JSON escaping (`json::string`, `json::string_ascii`) and the
//! CPython-compatible `jsonout` writer.
//!
//! Invariants: every escaped string is a valid JSON string literal that
//! decodes back to the input; the `ensure_ascii` forms are pure ASCII; and a
//! JSON document written by `to_json_compat` parses back to the same value;
//! `float_repr` round-trips every finite f64 bit for bit.

#![no_main]

use galion_common::json::{float_repr, string, string_ascii};
use galion_common::jsonout::{json_str, to_json_compat, truthy};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);

    let plain = string(&s);
    let back: String = serde_json::from_str(&plain).expect("string() is valid JSON");
    assert_eq!(back, s);

    let ascii = string_ascii(&s);
    assert!(ascii.is_ascii());
    let back: String = serde_json::from_str(&ascii).expect("string_ascii() is valid JSON");
    assert_eq!(back, s);

    // The serde writer must agree with the hand escaper on strings.
    assert_eq!(to_json_compat(&Value::String(s.to_string()), false), ascii);

    // CPython float repr: round-trips bit for bit, CPython layout.
    if let Some(bits) = data.get(..8) {
        let v = f64::from_le_bytes(bits.try_into().unwrap_or([0; 8]));
        let r = float_repr(v);
        if v.is_finite() {
            let back: f64 = r.parse().expect("float_repr parses");
            assert_eq!(back.to_bits(), v.to_bits(), "{r}");
            assert!(
                r.contains('.') || r.contains("e+") || r.contains("e-"),
                "{r}"
            );
            if let Some((_, exp)) = r.split_once('e') {
                assert!(exp.len() >= 3, "{r}");
            }
            if let Some(n) = serde_json::Number::from_f64(v) {
                assert_eq!(to_json_compat(&Value::Number(n), false), r);
            }
        }
    }

    if let Ok(value) = serde_json::from_slice::<Value>(data) {
        for spaced in [false, true] {
            let out = to_json_compat(&value, spaced);
            assert!(out.is_ascii());
            let again: Value = serde_json::from_str(&out).expect("to_json_compat is valid JSON");
            // Needs serde_json `float_roundtrip` (fuzz Cargo.toml): the
            // default fast float parser can land 1 ulp off on long literals
            // (docs/fuzzing.md, finding 1).
            assert_eq!(again, value);
        }
        let _ = truthy(&value);
        let _ = json_str(&value);
    }
});
