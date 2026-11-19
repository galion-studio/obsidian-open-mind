// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Char-boundary cuts for text. `&s[i..j]`, `split_at` and `String::truncate`
//! panic when an index lands inside a multi-byte character (a Czech `č`
//! after `&` did that to the gateway). These helpers move the index to a
//! boundary and never panic.

/// Largest `<= i` (and `<= len`) that is a char boundary. `0` and `len` always are.
pub fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest `>= i` that is a char boundary, capped at `len`.
pub fn ceil_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// First `n` bytes, cut back so the end is a char boundary.
pub fn prefix_bytes(s: &str, n: usize) -> &str {
    &s[..floor_char_boundary(s, n)]
}

/// Last `n` bytes, cut forward so the start is a char boundary.
pub fn suffix_bytes(s: &str, n: usize) -> &str {
    if n >= s.len() {
        return s;
    }
    let start = ceil_char_boundary(s, s.len() - n);
    &s[start..]
}

/// `s` split at `i`, moved back to a char boundary. Both halves are valid `&str`.
pub fn split_at_floor(s: &str, i: usize) -> (&str, &str) {
    s.split_at(floor_char_boundary(s, i))
}

/// `String::truncate` that will not panic on a mid-character index.
pub fn truncate_to(s: &mut String, n: usize) {
    let i = floor_char_boundary(s, n);
    s.truncate(i);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: &[&str] = &[
        "",
        "ascii",
        "Črnomerec Šibenik Đakovo",
        "Zażółć gęślą jaźń",
        "Češljuga u Đakovu",
        "中文测试",
        "日本語の試験",
        "مرحبا بالعالم",
        "नमस्ते दुनिया",
        "👩‍👩‍👧‍👦 🏳️‍🌈 👍🏽",
        "e\u{301} ZWJ\u{200d} RTL\u{200f}",
        "a &č; end",
    ];

    fn corpus() -> Vec<String> {
        let mut v: Vec<String> = SAMPLES.iter().map(|s| (*s).to_string()).collect();
        let lossy = String::from_utf8_lossy(&[0xff, 0xfe, 0xc3, 0x28, b'&', 0xc4, b'c', 0x80]);
        v.push(lossy.into_owned());
        v
    }

    #[test]
    fn every_byte_offset_is_a_char_boundary() {
        for s in corpus() {
            for n in 0..=s.len() + 3 {
                let p = prefix_bytes(&s, n);
                let u = suffix_bytes(&s, n);
                assert!(s.is_char_boundary(p.len()), "prefix {n} in {s:?}");
                assert!(s.get(s.len() - u.len()..).is_some() || u.is_empty());
                assert!(s.starts_with(p), "{p:?} not a prefix of {s:?}");
                assert!(s.ends_with(u), "{u:?} not a suffix of {s:?}");
                let (a, b) = split_at_floor(&s, n);
                assert_eq!(format!("{a}{b}"), s);
                let mut t = s.clone();
                truncate_to(&mut t, n);
                assert!(s.starts_with(t.as_str()));
            }
        }
    }

    #[test]
    fn floor_backs_up_into_a_multibyte_char() {
        let s = "€";
        assert_eq!(floor_char_boundary(s, 1), 0);
        assert_eq!(floor_char_boundary(s, 2), 0);
        assert_eq!(prefix_bytes(s, 2), "");
        assert_eq!(ceil_char_boundary(s, 1), 3);
        // Two bytes cannot hold `€`, so the tail starts after it.
        assert_eq!(suffix_bytes(s, 2), "");
        assert_eq!(suffix_bytes(s, 3), "€");
    }
}
