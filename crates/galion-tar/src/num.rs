// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Header numeric fields and checksums.

use std::io;

/// GNU style: zero-padded octal with a trailing NUL when it fits, else GNU
/// base-256 (big-endian two's complement, top bit of byte 0 set).
pub fn octal_or_256(field: &mut [u8], v: i64) {
    let n = field.len();
    let max = if n >= 22 {
        i64::MAX
    } else {
        (1i64 << (3 * (n - 1))) - 1
    };
    if (0..=max).contains(&v) {
        let s = format!("{:0w$o}", v, w = n - 1);
        field[..n - 1].copy_from_slice(s.as_bytes());
        field[n - 1] = 0;
    } else {
        let bytes = v.to_be_bytes();
        let fill = if v < 0 { 0xff } else { 0 };
        for b in field.iter_mut() {
            *b = fill;
        }
        let k = bytes.len().min(n);
        field[n - k..].copy_from_slice(&bytes[8 - k..]);
        field[0] = if v < 0 { 0xff } else { 0x80 };
    }
}

/// POSIX ustar: zero-padded octal with a trailing NUL; error when it does not fit.
pub fn octal_strict(field: &mut [u8], v: u64) -> io::Result<()> {
    let s = format!("{:0w$o}", v, w = field.len() - 1);
    if s.len() > field.len() - 1 {
        return Err(io::Error::other("number too large for a ustar field"));
    }
    field[..s.len()].copy_from_slice(s.as_bytes());
    field[s.len()] = 0;
    Ok(())
}

/// Parse an octal (space/NUL padded) or GNU base-256 numeric field.
pub fn parse(field: &[u8]) -> io::Result<i64> {
    if let Some(&b0) = field.first() {
        if b0 & 0x80 != 0 {
            let neg = b0 == 0xff;
            let mut v: i64 = if neg { -1 } else { 0 };
            for (i, &b) in field.iter().enumerate() {
                let b = if i == 0 && !neg { b & 0x7f } else { b };
                if !neg && v > (i64::MAX >> 8) {
                    return Err(io::Error::other("base-256 field overflows i64"));
                }
                v = (v << 8) | i64::from(b);
            }
            return Ok(v);
        }
    }
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    let s = std::str::from_utf8(&field[..end])
        .map_err(|_| io::Error::other("non-ASCII numeric field"))?
        .trim_matches(' ');
    if s.is_empty() {
        return Ok(0);
    }
    i64::from_str_radix(s, 8).map_err(|_| io::Error::other(format!("bad octal field {s:?}")))
}

/// Unsigned header checksum with the checksum field counted as spaces.
pub fn checksum(h: &[u8; 512]) -> u32 {
    h.iter()
        .enumerate()
        .map(|(i, &b)| {
            if (148..156).contains(&i) {
                32
            } else {
                u32::from(b)
            }
        })
        .sum()
}

/// Signed variant (some historic tars); accepted by the reader.
pub fn checksum_signed(h: &[u8; 512]) -> i64 {
    h.iter()
        .enumerate()
        .map(|(i, &b)| {
            if (148..156).contains(&i) {
                32
            } else {
                i64::from(b as i8)
            }
        })
        .sum()
}

/// Linux `dev_t` -> (major, minor), the glibc/musl `gnu_dev_major/minor` encoding.
pub fn dev_split(dev: u64) -> (u32, u32) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & 0xffff_f000);
    let minor = (dev & 0xff) | ((dev >> 12) & 0xffff_ff00);
    (major as u32, minor as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_and_base256() {
        let mut f = [0u8; 12];
        octal_or_256(&mut f, 0o1234);
        assert_eq!(&f, b"00000001234\0");
        assert_eq!(parse(&f).unwrap(), 0o1234);
        octal_or_256(&mut f, 1 << 34); // 16 GiB > 8^11-1
        assert_eq!(f[0], 0x80);
        assert_eq!(u64::from_be_bytes(f[4..].try_into().unwrap()), 1 << 34);
        assert_eq!(parse(&f).unwrap(), 1 << 34);
        octal_or_256(&mut f, -5);
        assert_eq!(parse(&f).unwrap(), -5);
        let mut m = [0u8; 8];
        octal_or_256(&mut m, 0o644);
        assert_eq!(&m, b"0000644\0");
        assert_eq!(parse(b"  644 \0\0").unwrap(), 0o644);
        assert!(octal_strict(&mut m, 1 << 21).is_err());
    }

    #[test]
    fn dev_split_matches_linux_encoding() {
        // makedev(8, 1) and makedev(259, 70000) per glibc's gnu_dev_makedev.
        let mk = |ma: u64, mi: u64| {
            (ma & 0xfff) << 8 | (ma & !0xfff) << 32 | (mi & 0xff) | (mi & !0xff) << 12
        };
        assert_eq!(dev_split(mk(8, 1)), (8, 1));
        assert_eq!(dev_split(mk(259, 70000)), (259, 70000));
        assert_eq!(dev_split(mk(4095 + 4096, 0xffff_ffff)), (8191, 0xffff_ffff));
    }
}
