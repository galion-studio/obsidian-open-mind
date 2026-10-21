// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// DEFLATE (RFC 1951) and zlib (RFC 1950) decoder, std only, no unsafe, no
// SIMD, no assembly. Written from the RFCs for the PDF text reader
// (pdf_text.rs), which replaces the `pdftotext` subprocess. Canonical
// Huffman codes are decoded bit by bit with the "count per length" method
// described in RFC 1951 section 3.2.2 (the same idea zlib's puff.c uses; no
// code copied). Speed is not the goal: a PDF's first page is a few KB to a
// few MB of content stream.
//
// Hostile input (an uploaded PDF) must never panic, loop forever or eat the
// box: every read is bounds-checked and the output is capped by `cap`.

/// Why decoding stopped early. The bytes decoded so far are still returned
/// (PDF writers often truncate or mis-terminate streams; poppler shows what
/// it got, and so do we).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Input ended inside a block.
    Truncated,
    /// Output would exceed the caller's cap (zip-bomb guard).
    TooBig,
    /// Malformed stream: bad block type, bad code lengths, bad distance...
    Corrupt,
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits {
            data,
            pos: 0,
            bit: 0,
            nbits: 0,
        }
    }

    fn need(&mut self, n: u32) -> Result<u32, Error> {
        // n <= 16 everywhere below.
        while self.nbits < n {
            let byte = *self.data.get(self.pos).ok_or(Error::Truncated)?;
            self.pos += 1;
            self.bit |= u32::from(byte) << self.nbits;
            self.nbits += 8;
        }
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }

    fn align(&mut self) {
        self.bit = 0;
        self.nbits = 0;
    }
}

/// A canonical Huffman code: how many codes of each length, and the symbols
/// in code order.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Build from per-symbol code lengths (0 = unused). Over-subscribed sets
    /// are rejected; incomplete ones are allowed (RFC 1951 permits a single
    /// distance code), a code that is never assigned just fails to decode.
    fn new(lengths: &[u8]) -> Result<Self, Error> {
        let mut count = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return Err(Error::Corrupt);
            }
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for c in count.iter().skip(1) {
            left <<= 1;
            left -= i32::from(*c);
            if left < 0 {
                return Err(Error::Corrupt);
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, Error> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= bits.need(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - first < count {
                return self
                    .symbol
                    .get((index + code - first) as usize)
                    .copied()
                    .ok_or(Error::Corrupt);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Error::Corrupt)
    }
}

const LBASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEXT: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DBASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEXT: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn codes(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    cap: usize,
) -> Result<(), Error> {
    loop {
        let sym = lit.decode(bits)?;
        if sym < 256 {
            if out.len() >= cap {
                return Err(Error::TooBig);
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let s = (sym - 257) as usize;
            if s >= 29 {
                return Err(Error::Corrupt);
            }
            let len = LBASE[s] as usize + bits.need(u32::from(LEXT[s]))? as usize;
            let d = dist.decode(bits)? as usize;
            if d >= 30 {
                return Err(Error::Corrupt);
            }
            let back = DBASE[d] as usize + bits.need(u32::from(DEXT[d]))? as usize;
            if back > out.len() {
                return Err(Error::Corrupt);
            }
            if out.len() + len > cap {
                return Err(Error::TooBig);
            }
            let start = out.len() - back;
            for i in 0..len {
                let b = out[start + i];
                out.push(b);
            }
        }
    }
}

fn fixed() -> (Huffman, Huffman) {
    let mut l = [0u8; 288];
    for (i, v) in l.iter_mut().enumerate() {
        *v = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    // Both built from constant, valid tables: cannot fail.
    let lit = Huffman::new(&l).unwrap_or(Huffman {
        count: [0; 16],
        symbol: Vec::new(),
    });
    let dist = Huffman::new(&[5u8; 30]).unwrap_or(Huffman {
        count: [0; 16],
        symbol: Vec::new(),
    });
    (lit, dist)
}

fn dynamic(bits: &mut Bits) -> Result<(Huffman, Huffman), Error> {
    let nlen = bits.need(5)? as usize + 257;
    let ndist = bits.need(5)? as usize + 1;
    let ncode = bits.need(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(Error::Corrupt);
    }
    let mut cl = [0u8; 19];
    for &o in ORDER.iter().take(ncode) {
        cl[o] = bits.need(3)? as u8;
    }
    let clh = Huffman::new(&cl)?;
    let mut lengths = [0u8; 316];
    let mut i = 0;
    while i < nlen + ndist {
        let sym = clh.decode(bits)?;
        if sym < 16 {
            lengths[i] = sym as u8;
            i += 1;
            continue;
        }
        let (val, rep) = match sym {
            16 => {
                if i == 0 {
                    return Err(Error::Corrupt);
                }
                (lengths[i - 1], 3 + bits.need(2)? as usize)
            }
            17 => (0, 3 + bits.need(3)? as usize),
            _ => (0, 11 + bits.need(7)? as usize),
        };
        if i + rep > nlen + ndist {
            return Err(Error::Corrupt);
        }
        for slot in lengths.iter_mut().skip(i).take(rep) {
            *slot = val;
        }
        i += rep;
    }
    if lengths[256] == 0 {
        return Err(Error::Corrupt);
    }
    let lit = Huffman::new(&lengths[..nlen])?;
    let dist = Huffman::new(&lengths[nlen..nlen + ndist])?;
    Ok((lit, dist))
}

/// Raw DEFLATE. Returns the output (possibly partial) and how it ended.
pub fn inflate_raw(data: &[u8], cap: usize) -> (Vec<u8>, Result<(), Error>) {
    let mut out = Vec::with_capacity(data.len().saturating_mul(3).min(cap).min(1 << 20));
    let mut bits = Bits::new(data);
    let res = (|| loop {
        let last = bits.need(1)?;
        match bits.need(2)? {
            0 => {
                bits.align();
                let p = bits.pos;
                let hdr = data.get(p..p + 4).ok_or(Error::Truncated)?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]);
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
                if len != !nlen {
                    return Err(Error::Corrupt);
                }
                let len = len as usize;
                let start = p + 4;
                let avail = data.len().saturating_sub(start);
                let take = len.min(avail);
                if out.len() + take > cap {
                    return Err(Error::TooBig);
                }
                out.extend_from_slice(&data[start..start + take]);
                bits.pos = start + take;
                if take < len {
                    return Err(Error::Truncated);
                }
            }
            1 => {
                let (lit, dist) = fixed();
                codes(&mut bits, &mut out, &lit, &dist, cap)?;
            }
            2 => {
                let (lit, dist) = dynamic(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, cap)?;
            }
            _ => return Err(Error::Corrupt),
        }
        if last == 1 {
            return Ok(());
        }
    })();
    (out, res)
}

/// zlib-wrapped DEFLATE (what PDF's /FlateDecode holds). A missing or bad
/// zlib header falls back to raw DEFLATE (some writers omit it); the Adler-32
/// trailer is not checked (poppler does not either).
pub fn inflate_zlib(data: &[u8], cap: usize) -> (Vec<u8>, Result<(), Error>) {
    if data.len() >= 2 {
        let cmf = data[0];
        let flg = data[1];
        let ok = cmf & 0x0f == 8
            && cmf >> 4 <= 7
            && (u16::from(cmf) * 256 + u16::from(flg)) % 31 == 0
            && flg & 0x20 == 0;
        if ok {
            return inflate_raw(&data[2..], cap);
        }
    }
    inflate_raw(data, cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_block() {
        // BFINAL=1, BTYPE=00, LEN=5, NLEN=!5, "hello"
        let d = [0x01, 0x05, 0x00, 0xfa, 0xff, b'h', b'e', b'l', b'l', b'o'];
        let (out, r) = inflate_raw(&d, 100);
        assert_eq!(r, Ok(()));
        assert_eq!(out, b"hello");
    }

    #[test]
    fn fixed_block_from_zlib() {
        // `printf 'hello hello hello hello' | gzip -9 -n` minus the gzip
        // header/trailer, re-wrapped as zlib (78 9c): fixed Huffman with a
        // back-reference. Adler-32 left off on purpose (not checked).
        let d = [
            0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01,
        ];
        let (out, r) = inflate_zlib(&d, 100);
        assert_eq!(r, Ok(()));
        assert_eq!(out, b"hello hello hello hello");
        let (out, r) = inflate_zlib(&d, 10);
        assert_eq!(r, Err(Error::TooBig));
        assert!(out.len() <= 10);
        // Truncated: partial output, error says so.
        let (out, r) = inflate_zlib(&d[..8], 100);
        assert_eq!(r, Err(Error::Truncated));
        assert!(b"hello hello hello hello".starts_with(&out));
    }

    /// gzip(1) as an independent encoder (test oracle only, never at run
    /// time): raw DEFLATE = gzip output minus 10-byte header, 8-byte trailer.
    fn gzip_raw(data: &[u8], level: &str) -> Option<Vec<u8>> {
        let _exec = crate::host::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("gzip")
            .args(["-n", "-c", level])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut si = child.stdin.take()?;
        let owned = data.to_vec();
        let w = std::thread::spawn(move || si.write_all(&owned));
        let out = child.wait_with_output().ok()?;
        w.join().ok()?.ok()?;
        (out.stdout.len() >= 18).then(|| out.stdout[10..out.stdout.len() - 8].to_vec())
    }

    #[test]
    fn matches_gzip_on_many_inputs() {
        if gzip_raw(b"x", "-6").is_none() {
            eprintln!("SKIP: gzip not installed");
            return;
        }
        let mut x: u64 = 0x1234_5678_9abc_def1;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let words: [&[u8]; 6] = [
            b"Galion ", b"hermes ", b"nerve ", b"\n", b"0 0 Td ", b"(Tj) ",
        ];
        let mut checked = 0;
        for case in 0..60usize {
            let len = [0usize, 1, 7, 300, 5000, 70_000, 200_000][case % 7];
            let mut d = Vec::with_capacity(len);
            while d.len() < len {
                match case % 3 {
                    0 => d.push(next() as u8),                              // stored blocks
                    1 => d.extend_from_slice(words[(next() % 6) as usize]), // dynamic
                    _ => d.push(b'a' + (next() % 3) as u8),                 // long matches
                }
            }
            d.truncate(len);
            for level in ["-1", "-9"] {
                let raw = gzip_raw(&d, level).expect("gzip");
                let (out, r) = inflate_raw(&raw, 1 << 22);
                assert_eq!(r, Ok(()), "case {case} level {level} len {len}");
                assert_eq!(out, d, "case {case} level {level}");
                checked += 1;
            }
        }
        // A bomb: 4 MB of zeros (about 4 KB compressed) capped at 64 KB.
        let raw = gzip_raw(&vec![0u8; 4 << 20], "-9").expect("gzip");
        let (out, r) = inflate_raw(&raw, 65_536);
        assert_eq!(r, Err(Error::TooBig));
        assert!(out.len() <= 65_536);
        eprintln!(
            "inflate == gzip on {checked} streams; bomb {} B -> capped",
            raw.len()
        );
    }

    #[test]
    fn garbage_never_panics() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for n in 0..4000usize {
            let len = n % 300;
            let mut d = Vec::with_capacity(len);
            for _ in 0..len {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                d.push(x as u8);
            }
            let (out, _) = inflate_raw(&d, 1 << 16);
            assert!(out.len() <= 1 << 16);
            let _ = inflate_zlib(&d, 1 << 16);
        }
    }

    #[test]
    fn oversubscribed_code_is_corrupt() {
        assert!(Huffman::new(&[1, 1, 1]).is_err());
        assert!(Huffman::new(&[1, 1]).is_ok());
    }
}
