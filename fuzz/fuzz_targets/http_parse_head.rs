// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-common HTTP/1.x request head parser, plus the client-side response
//! head parser and chunked decoder.
//!
//! Byte 0 picks the limits; the rest is the wire. Invariants beyond "no
//! panic": an accepted head respects the limits and survives a
//! serialise -> parse round trip unchanged.

#![no_main]

use galion_common::http::{dechunk, is_tchar, parse_head, parse_response_head, Error, Limits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&sel, wire)) = data.split_first() else {
        return;
    };
    let limits = Limits {
        max_head_bytes: [64, 512, 8 * 1024, 64 * 1024][(sel & 3) as usize],
        max_headers: [0, 1, 8, 64][((sel >> 2) & 3) as usize],
        ..Limits::default()
    };
    match parse_head(wire, &limits) {
        Ok(head) => {
            assert!(head.len <= wire.len());
            assert!(head.len <= limits.max_head_bytes);
            assert!(head.headers.len() <= limits.max_headers);
            assert!(head.minor <= 1);
            assert!(!head.method.is_empty() && head.method.len() <= 32);
            assert!(head.method.bytes().all(is_tchar));
            assert!(head.target.bytes().all(|c| (0x21..=0x7e).contains(&c)));
            for (name, value) in &head.headers {
                assert!(!name.is_empty() && name.bytes().all(is_tchar));
                assert!(!value.starts_with([' ', '\t']) && !value.ends_with([' ', '\t']));
                assert!(!name.eq_ignore_ascii_case("transfer-encoding"));
            }
            // Round trip: re-serialise and parse again with roomy limits
            // (lossy UTF-8 can grow a value from 1 to 3 bytes).
            let mut again = format!("{} {} HTTP/1.{}\r\n", head.method, head.target, head.minor);
            for (name, value) in &head.headers {
                again.push_str(name);
                again.push_str(": ");
                again.push_str(value);
                again.push_str("\r\n");
            }
            again.push_str("\r\n");
            let roomy = Limits {
                max_head_bytes: again.len() + 1,
                max_headers: head.headers.len(),
                ..Limits::default()
            };
            let back = parse_head(again.as_bytes(), &roomy).expect("re-serialised head must parse");
            assert_eq!(back.method, head.method);
            assert_eq!(back.target, head.target);
            assert_eq!(back.minor, head.minor);
            assert_eq!(back.headers, head.headers);
            assert_eq!(back.content_length, head.content_length);
            assert_eq!(back.len, again.len());
        }
        Err(Error::Incomplete) => assert!(wire.len() <= limits.max_head_bytes),
        Err(e) => {
            let code = e.status();
            assert!((400..=599).contains(&code));
        }
    }

    if let Some((status, _reason, _headers, len)) = parse_response_head(wire) {
        assert!((0..=999).contains(&status));
        assert!(len <= wire.len());
    }
    if let Some(body) = dechunk(wire) {
        assert!(body.len() <= wire.len());
    }
});
