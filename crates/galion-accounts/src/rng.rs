// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The OS CSPRNG (getrandom). Every secret here comes from it.

/// Fill `buf` from the OS CSPRNG. Panics only if the OS has no randomness
/// source at all, which is not a state we can serve accounts from.
pub fn fill(buf: &mut [u8]) {
    getrandom::getrandom(buf).expect("OS CSPRNG unavailable");
}

/// A random u64 from the OS CSPRNG.
pub fn u64() -> u64 {
    let mut b = [0u8; 8];
    fill(&mut b);
    u64::from_le_bytes(b)
}

/// `n` random bytes.
pub fn bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    fill(&mut v);
    v
}

/// Crockford base32 (no I, L, O, U): easy to read and to hand-write on paper.
pub const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// `n` random Crockford base32 characters (5 bits each).
pub fn crockford(n: usize) -> String {
    let raw = bytes(n);
    raw.iter()
        .map(|b| CROCKFORD[(b & 31) as usize] as char)
        .collect()
}
