// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Lower-case hex.

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Lower-case hex of `bytes`.
pub fn encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
    s
}

/// Decode hex (either case). `None` on odd length or a non-hex digit.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    b.as_chunks::<2>()
        .0
        .iter()
        .map(|p| Some((nibble(p[0])? << 4) | nibble(p[1])?))
        .collect()
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(super::decode(&super::encode(&all)).unwrap(), all);
        assert_eq!(super::encode(&[0xde, 0xad]), "dead");
        assert_eq!(super::decode("DEAD").unwrap(), vec![0xde, 0xad]);
        assert!(super::decode("abc").is_none());
        assert!(super::decode("zz").is_none());
    }
}
