// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Counter-based deterministic randomness.
//!
//! Every random draw is a pure function of `(seed, stream, a, b)`, so the
//! event schedule and the customer demand do not depend on what the operator
//! did earlier. Two operators on the same seed face the same world. Integer
//! only: no floats anywhere, so x86_64 and aarch64 give bit-identical scores.

/// SplitMix64 finaliser (public-domain algorithm by Sebastiano Vigna, written
/// from the published constants, no code copied).
pub fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One draw for `(seed, stream, a, b)`.
pub fn draw(seed: u64, stream: u64, a: u64, b: u64) -> u64 {
    mix(mix(mix(seed ^ mix(stream)) ^ a) ^ b.wrapping_mul(0xD6E8_FEB8_6659_FD93))
}

/// Uniform integer in `lo..=hi` for `(seed, stream, a, b)`.
pub fn range(seed: u64, stream: u64, a: u64, b: u64, lo: u64, hi: u64) -> u64 {
    debug_assert!(lo <= hi);
    lo + draw(seed, stream, a, b) % (hi - lo + 1)
}

/// Small sequential generator for building the schedule.
pub struct Seq {
    seed: u64,
    stream: u64,
    n: u64,
}

impl Seq {
    pub fn new(seed: u64, stream: u64) -> Self {
        Self { seed, stream, n: 0 }
    }

    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        self.n += 1;
        range(self.seed, self.stream, self.n, 0, lo, hi)
    }
}

/// FNV-1a 64 over bytes, used for the state hash in the durable log.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_and_in_range() {
        assert_eq!(draw(42, 1, 2, 3), draw(42, 1, 2, 3));
        assert_ne!(draw(42, 1, 2, 3), draw(43, 1, 2, 3));
        for i in 0..1000 {
            let v = range(7, 9, i, 0, 3, 5);
            assert!((3..=5).contains(&v));
        }
    }

    #[test]
    fn fnv_known_vector() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
