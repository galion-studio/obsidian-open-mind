// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! 64-bit SimHash (Charikar 2002) and a banded index for Hamming <= 3 lookup.
//!
//! Pigeonhole: two 64-bit fingerprints within Hamming distance 3 agree on at
//! least one of four 16-bit bands, so probing four buckets finds every
//! candidate (Manku, Jain, Das Sarma, WWW 2007). Buckets are intrusive singly
//! linked lists in flat vectors: no per-bucket allocation.

use crate::hash::IdMap;
use crate::text::mix;

/// Accumulates feature hashes on the stack.
pub struct SimAcc {
    v: [i32; 64],
    n: u32,
}

impl Default for SimAcc {
    fn default() -> Self {
        SimAcc { v: [0; 64], n: 0 }
    }
}

impl SimAcc {
    #[inline]
    pub fn add(&mut self, h: u64) {
        self.n += 1;
        let mut x = h;
        for slot in self.v.iter_mut() {
            *slot += ((x & 1) as i32) * 2 - 1;
            x >>= 1;
        }
    }
    pub fn features(&self) -> u32 {
        self.n
    }
    pub fn finish(&self) -> u64 {
        let mut out = 0u64;
        for (i, &c) in self.v.iter().enumerate() {
            if c > 0 {
                out |= 1 << i;
            }
        }
        out
    }
}

/// Banded near-duplicate index. Entries carry an extra `sig` that must match
/// exactly (we use it for the set of non-volatile IDs/numbers in a unit, so
/// "port 22041" and "port 22042" are never merged).
#[derive(Default)]
pub struct NearIndex {
    heads: IdMap<u32>,
    next: Vec<u32>,
    sims: Vec<u64>,
    sigs: Vec<u64>,
    ids: Vec<u32>,
}

const NONE: u32 = u32::MAX;
/// Upper bound on chain steps per band probe; keeps worst case linear.
const MAX_PROBE: usize = 64;

impl NearIndex {
    pub fn clear(&mut self) {
        self.heads.clear();
        self.next.clear();
        self.sims.clear();
        self.sigs.clear();
        self.ids.clear();
    }

    #[inline]
    fn key(band: u64, val: u64, sig: u64) -> u64 {
        crate::text::finish(mix(mix(band, val), sig))
    }

    /// Returns the id of a stored entry within `max_ham`, if any.
    pub fn find(&self, sim: u64, sig: u64, max_ham: u32) -> Option<u32> {
        for band in 0..4u64 {
            let val = (sim >> (band * 16)) & 0xffff;
            let mut cur = match self.heads.get(&Self::key(band, val, sig)) {
                Some(&h) => h,
                None => continue,
            };
            let mut steps = 0;
            while cur != NONE && steps < MAX_PROBE {
                let c = cur as usize;
                if self.sigs[c] == sig && (self.sims[c] ^ sim).count_ones() <= max_ham {
                    return Some(self.ids[c]);
                }
                cur = self.next[c];
                steps += 1;
            }
        }
        None
    }

    pub fn insert(&mut self, sim: u64, sig: u64, id: u32) {
        for band in 0..4u64 {
            let val = (sim >> (band * 16)) & 0xffff;
            let slot = self.sims.len() as u32;
            let head = self.heads.entry(Self::key(band, val, sig)).or_insert(NONE);
            self.next.push(*head);
            *head = slot;
            self.sims.push(sim);
            self.sigs.push(sig);
            self.ids.push(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_within_three_bits_only() {
        let mut ix = NearIndex::default();
        let a = 0xdead_beef_0123_4567u64;
        ix.insert(a, 7, 1);
        assert_eq!(ix.find(a ^ 0b111, 7, 3), Some(1));
        assert_eq!(
            ix.find(a ^ (1 << 63) ^ (1 << 40) ^ (1 << 20), 7, 3),
            Some(1)
        );
        assert_eq!(ix.find(a ^ 0b1111, 7, 3), None);
        assert_eq!(ix.find(a, 8, 3), None, "sig must match");
    }

    #[test]
    fn simacc_majority() {
        let mut s = SimAcc::default();
        s.add(0b101);
        s.add(0b101);
        s.add(0b011);
        assert_eq!(s.finish() & 0b111, 0b101);
    }
}
