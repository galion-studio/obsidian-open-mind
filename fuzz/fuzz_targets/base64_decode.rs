// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-common base64: strict decode of arbitrary text, and encode/decode
//! round trips of arbitrary bytes in both alphabets.
//!
//! Invariants: whatever `decode` accepts is canonical (re-encoding gives the
//! same text up to alphabet and padding), and `decode(encode(x)) == x`.

#![no_main]

use galion_common::base64::{decode, encode, encode_url};
use libfuzzer_sys::fuzz_target;

fn canon(s: &str) -> String {
    s.trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fuzz_target!(|data: &[u8]| {
    // Arbitrary bytes as input to the encoders.
    let std = encode(data);
    let url = encode_url(data);
    assert_eq!(std.len() % 4, 0);
    assert!(!url.contains('='));
    assert_eq!(decode(&std).as_deref(), Ok(data));
    assert_eq!(decode(&url).as_deref(), Ok(data));
    assert_eq!(canon(&std), url);

    // Arbitrary text as input to the decoder.
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(bytes) = decode(text) {
            assert!(bytes.len() <= text.len());
            assert_eq!(encode_url(&bytes), canon(text));
            // Padded input must be exactly the standard padded form length.
            if text.ends_with('=') {
                assert_eq!(text.len(), encode(&bytes).len());
            }
        }
    }
});
