// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! RFC 4648 base64: standard alphabet with `=` padding (`encode`) and the
//! URL-safe alphabet without padding (`encode_url`, as used for JWT-style
//! tokens). Replaces the copies in galion-one, one-talk and edge util.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn enc(data: &[u8], t: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = ((chunk[0] as u32) << 16) | (b1 << 8) | b2;
        out.push(t[((n >> 18) & 63) as usize] as char);
        out.push(t[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(t[((n >> 6) & 63) as usize] as char);
        } else if pad {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(t[(n & 63) as usize] as char);
        } else if pad {
            out.push('=');
        }
    }
    out
}

/// Standard alphabet, padded (`TWE=`).
pub fn encode(data: &[u8]) -> String {
    enc(data, STD, true)
}

/// URL-safe alphabet, no padding (`TWE`).
pub fn encode_url(data: &[u8]) -> String {
    enc(data, URL, false)
}

/// Why [`decode`] refused its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// A byte outside both alphabets (whitespace is not skipped).
    InvalidByte(usize),
    /// Length that no encoding produces, or padding in the wrong place.
    InvalidLength,
    /// Non-zero trailing bits (a non-canonical encoding).
    TrailingBits,
}

fn val(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' | b'-' => 62,
        b'/' | b'_' => 63,
        _ => return None,
    } as u32)
}

/// Decode either alphabet, padded or not. Strict: no whitespace, canonical
/// trailing bits only.
pub fn decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    let b = s.as_bytes();
    let body = b
        .strip_suffix(b"==")
        .or_else(|| b.strip_suffix(b"="))
        .unwrap_or(b);
    if body.len() != b.len() && !b.len().is_multiple_of(4) {
        return Err(DecodeError::InvalidLength);
    }
    if body.len() % 4 == 1 {
        return Err(DecodeError::InvalidLength);
    }
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 2);
    for (ci, chunk) in body.chunks(4).enumerate() {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let v = val(c).ok_or(DecodeError::InvalidByte(ci * 4 + i))?;
            n |= v << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        let keep = chunk.len() - 1;
        if (keep == 1 && n & 0xffff != 0) || (keep == 2 && n & 0xff != 0) {
            return Err(DecodeError::TrailingBits);
        }
        out.extend_from_slice(&bytes[..keep]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_vectors() {
        let v = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, coded) in v {
            assert_eq!(encode(plain.as_bytes()), coded);
            assert_eq!(decode(coded).unwrap(), plain.as_bytes());
            assert_eq!(encode_url(plain.as_bytes()), coded.trim_end_matches('='));
            assert_eq!(
                decode(coded.trim_end_matches('=')).unwrap(),
                plain.as_bytes()
            );
        }
        assert_eq!(encode(&[0xfb, 0xff]), "+/8=");
        assert_eq!(encode_url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn round_trip_and_rejects() {
        let mut x = 0x1234_5678u32;
        for len in 0..200 {
            let data: Vec<u8> = (0..len)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x as u8
                })
                .collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data);
            assert_eq!(decode(&encode_url(&data)).unwrap(), data);
        }
        assert_eq!(decode("Zg"), Ok(b"f".to_vec()));
        assert_eq!(decode("Z"), Err(DecodeError::InvalidLength));
        assert_eq!(decode("Zg="), Err(DecodeError::InvalidLength));
        assert_eq!(decode("Zh=="), Err(DecodeError::TrailingBits));
        assert_eq!(decode("Zm 9"), Err(DecodeError::InvalidByte(2)));
        assert!(decode("=").is_err());
        assert!(decode("====").is_err());
    }
}
