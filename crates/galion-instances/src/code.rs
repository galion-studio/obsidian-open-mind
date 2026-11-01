// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Pairing codes and device tokens.
//!
//! A pairing code is 8 characters from an alphabet without look-alikes
//! (no 0/O, 1/I/L), shown as `ABCD-EFGH`: about 40 bits, single use, alive
//! 10 minutes, and joins are rate limited, so guessing is not practical.
//! Only the SHA-256 of a code or token is kept; the plain value exists once,
//! in the answer that shows it.

use galion_common::sha256;
use std::io::Read;

/// Characters a code is made of (31; easy to read aloud and type).
pub const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// Characters in a code, without the dash.
pub const CODE_LEN: usize = 8;

/// `n` random bytes from the OS (`/dev/urandom`). `None` if unavailable.
pub fn random(n: usize) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open("/dev/urandom").ok()?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

/// A fresh code like `K7QM-3XPD` (rejection sampling, no modulo bias).
pub fn new_code() -> Option<String> {
    let mut out = String::new();
    while out.len() < CODE_LEN {
        for b in random(16)? {
            // 248 = 8 * 31: values above it would bias the first letters.
            if b < 248 && out.len() < CODE_LEN {
                out.push(ALPHABET[(b % 31) as usize] as char);
            }
        }
    }
    Some(format!("{}-{}", &out[..4], &out[4..]))
}

/// A fresh 256-bit device token (hex).
pub fn new_token() -> Option<String> {
    Some(galion_common::hex::encode(&random(32)?))
}

/// Normalise what a person typed: case, spaces and dashes do not matter,
/// and the look-alikes map to the letter we use (O->0 is not in the
/// alphabet, so O and 0 are refused, as are I, L and 1).
pub fn normalize(typed: &str) -> Option<String> {
    let s: String = typed
        .chars()
        .filter(|c| !matches!(c, '-' | ' ' | '\t'))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (s.len() == CODE_LEN && s.bytes().all(|b| ALPHABET.contains(&b))).then_some(s)
}

/// Stored form of a code or token.
pub fn hash(secret: &str) -> String {
    sha256::hex(&sha256::digest(secret.as_bytes()))
}

/// Constant-time string compare (for stored hashes).
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_well_formed_and_differ() {
        let a = new_code().unwrap();
        let b = new_code().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.len(), 9);
        assert_eq!(&a[4..5], "-");
        assert_eq!(normalize(&a).unwrap().len(), 8);
        assert_eq!(new_token().unwrap().len(), 64);
    }

    #[test]
    fn typing_is_forgiving_but_strict() {
        assert_eq!(normalize(" k7qm-3xpd ").as_deref(), Some("K7QM3XPD"));
        assert_eq!(normalize("K7QM 3XPD").as_deref(), Some("K7QM3XPD"));
        assert_eq!(normalize("K7QM3XP"), None);
        assert_eq!(normalize("K7QM3XP0"), None, "0 is not in the alphabet");
        assert_eq!(normalize("K7QM3XPI"), None);
        assert_eq!(normalize("K7QM3XPDD"), None);
    }

    #[test]
    fn hash_and_compare() {
        assert_eq!(hash("x").len(), 64);
        assert!(same(&hash("x"), &hash("x")));
        assert!(!same(&hash("x"), &hash("y")));
        assert!(!same("ab", "abc"));
    }
}
