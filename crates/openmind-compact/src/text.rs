// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Allocation-free text primitives: word iteration, hashing, ID detection.

/// 64-bit multiplicative constant (FxHash family). Deterministic across runs
/// and platforms; not DoS-resistant, which is fine for in-process scoring.
const K: u64 = 0x517c_c1b7_2722_0a95;

#[inline]
pub fn mix(h: u64, b: u64) -> u64 {
    (h.rotate_left(5) ^ b).wrapping_mul(K)
}

/// Final avalanche (splitmix64 finaliser) so low bits are usable for banding.
#[inline]
pub fn finish(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Iterator over word slices (`[A-Za-z0-9_]` runs plus any non-ASCII bytes).
/// Borrowing, no allocation.
pub struct Words<'a> {
    s: &'a [u8],
    i: usize,
}

pub fn words(s: &str) -> Words<'_> {
    Words {
        s: s.as_bytes(),
        i: 0,
    }
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a [u8];
    #[inline]
    fn next(&mut self) -> Option<&'a [u8]> {
        let s = self.s;
        let mut i = self.i;
        while i < s.len() && !is_word_byte(s[i]) {
            i += 1;
        }
        if i >= s.len() {
            self.i = i;
            return None;
        }
        let start = i;
        while i < s.len() && is_word_byte(s[i]) {
            i += 1;
        }
        self.i = i;
        Some(&s[start..i])
    }
}

/// Case-insensitive hash of one word.
#[inline]
pub fn word_hash(w: &[u8]) -> u64 {
    let mut h = 0u64;
    for &b in w {
        h = mix(h, b.to_ascii_lowercase() as u64);
    }
    finish(h ^ w.len() as u64)
}

/// Word class used for anchoring and near-duplicate signatures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordKind {
    Plain,
    /// Looks like an identifier or a number worth keeping (SHA, port, ticket
    /// part, version, amount).
    Id,
    /// Pure short number (< 3 digits): too common to mean anything.
    Small,
}

#[inline]
pub fn word_kind(w: &[u8]) -> WordKind {
    let mut digits = 0usize;
    let mut alpha = 0usize;
    for &b in w {
        if b.is_ascii_digit() {
            digits += 1;
        } else if b.is_ascii_alphabetic() {
            alpha += 1;
        }
    }
    if digits == 0 {
        // all-caps acronym-ish tokens such as "OM" stay plain; hex words with
        // no digits (e.g. "deadbeef") are rare and ambiguous with English.
        return WordKind::Plain;
    }
    if alpha == 0 && digits < 3 {
        return WordKind::Small;
    }
    WordKind::Id
}

/// Timestamp-shaped run around a word: digits joined by `-:.T+/`, e.g.
/// `2026-10-21T17:10:01+02:00`. Used so heartbeat lines that differ only in
/// time still count as near duplicates. Checked on the raw bytes around the
/// word, so it needs the full line and the word offset.
#[inline]
pub fn is_volatile(line: &[u8], start: usize, end: usize) -> bool {
    let sep = |b: u8| matches!(b, b'-' | b':' | b'.' | b'T' | b'+' | b'/' | b'Z');
    let only_digits = line[start..end].iter().all(|b| b.is_ascii_digit())
        || (line[start..end].len() <= 20
            && line[start..end]
                .iter()
                .all(|b| b.is_ascii_digit() || *b == b'T' || *b == b'Z'));
    if !only_digits {
        return false;
    }
    let before =
        start > 0 && sep(line[start - 1]) && start >= 2 && line[start - 2].is_ascii_digit();
    // UTC offset such as +0200 or -0530
    let offset = end - start == 4
        && start > 0
        && (line[start - 1] == b'+'
            || (line[start - 1] == b'-' && (start < 2 || line[start - 2] == b' ')));
    let after = end + 1 < line.len() && sep(line[end]) && line[end + 1].is_ascii_digit();
    before || after || offset
}

/// Word iterator that also yields byte offsets (for `is_volatile`).
pub fn words_at(s: &str) -> impl Iterator<Item = (usize, &[u8])> {
    let base = s.as_ptr() as usize;
    words(s).map(move |w| (w.as_ptr() as usize - base, w))
}

/// Hash of the whitespace-collapsed, lowercased text (exact-duplicate key).
pub fn norm_hash(s: &str) -> u64 {
    let mut h = 0u64;
    let mut n = 0u64;
    for w in s.split_ascii_whitespace() {
        for &b in w.as_bytes() {
            h = mix(h, b.to_ascii_lowercase() as u64);
        }
        h = mix(h, 0x20);
        n += 1;
    }
    finish(h ^ n)
}

/// Token estimate consistent with hermes-rs (`bytes / 4`, rounded up).
#[inline]
pub fn est_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

/// Largest char boundary <= i.
#[inline]
pub fn floor_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_kinds() {
        let v: Vec<&[u8]> = words("Deploy SHA 3f9a2c1, port 22041; ok-7!").collect();
        assert_eq!(
            v,
            vec![
                &b"Deploy"[..],
                b"SHA",
                b"3f9a2c1",
                b"port",
                b"22041",
                b"ok",
                b"7"
            ]
        );
        assert_eq!(word_kind(b"3f9a2c1"), WordKind::Id);
        assert_eq!(word_kind(b"22041"), WordKind::Id);
        assert_eq!(word_kind(b"7"), WordKind::Small);
        assert_eq!(word_kind(b"Deploy"), WordKind::Plain);
        assert_eq!(word_kind(b"T32"), WordKind::Id);
    }

    #[test]
    fn hashes_case_insensitive_and_ws_collapsed() {
        assert_eq!(word_hash(b"Port"), word_hash(b"port"));
        assert_eq!(norm_hash("a  b\tC"), norm_hash("A b c"));
        assert_ne!(norm_hash("a b"), norm_hash("ab"));
    }

    #[test]
    fn volatile_timestamps() {
        let l = "2026-10-21T17:10:01+02:00 heartbeat ok port 22041";
        let ws: Vec<(usize, &[u8])> = words_at(l).collect();
        let vol: Vec<bool> = ws
            .iter()
            .map(|(o, w)| is_volatile(l.as_bytes(), *o, o + w.len()))
            .collect();
        // 2026 10 21T17 10 01 02 00 heartbeat ok port 22041
        assert!(vol[0] && vol[1]);
        assert!(!vol[vol.len() - 1], "a port is not volatile");
    }

    #[test]
    fn boundaries() {
        let s = "aé";
        assert_eq!(floor_boundary(s, 2), 1);
        assert_eq!(est_tokens("abcde"), 2);
    }
}

#[cfg(test)]
mod offset_tests {
    use super::*;
    #[test]
    fn utc_offset_volatile_ticket_not() {
        let l = "17:10:49 +0200 ticket OM-4821";
        let v: Vec<bool> = words_at(l)
            .map(|(o, w)| is_volatile(l.as_bytes(), o, o + w.len()))
            .collect();
        // 17 10 49 0200 ticket OM 4821
        assert_eq!(v, vec![true, true, true, true, false, false, false]);
    }
}
