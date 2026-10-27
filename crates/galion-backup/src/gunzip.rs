// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Streaming gzip reader (RFC 1952) over miniz_oxide's raw inflate, the read
//! side of [`crate::gzip::GzWriter`] (used by `restore-test` in place of
//! `tar -xzf`'s gzip). Header flags FEXTRA/FNAME/FCOMMENT/FHCRC are skipped;
//! the trailer CRC32 and ISIZE are verified. Concatenated members (what
//! `gzip -d` accepts) are read one after another; trailing zero padding is
//! ignored like gzip does; any other trailing bytes are an error.

use crate::gzip::crc32;
use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};
use std::io::{self, Read};

const IN: usize = 64 * 1024;

fn bad(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("gzip: {}", m.into()))
}

pub struct GzReader<R: Read> {
    inner: R,
    buf: Box<[u8]>,
    pos: usize,
    len: usize,
    eof: bool,
    st: Box<InflateState>,
    crc: u32,
    size: u32,
    /// false = a header must be read before more data.
    in_member: bool,
    done: bool,
    members: u32,
}

impl<R: Read> GzReader<R> {
    pub fn new(inner: R) -> Self {
        GzReader {
            inner,
            buf: vec![0u8; IN].into_boxed_slice(),
            pos: 0,
            len: 0,
            eof: false,
            st: InflateState::new_boxed(DataFormat::Raw),
            crc: 0,
            size: 0,
            in_member: false,
            done: false,
            members: 0,
        }
    }

    fn fill(&mut self) -> io::Result<()> {
        if self.pos == self.len && !self.eof {
            self.pos = 0;
            self.len = 0;
            loop {
                match self.inner.read(&mut self.buf) {
                    Ok(0) => {
                        self.eof = true;
                        break;
                    }
                    Ok(n) => {
                        self.len = n;
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// Next input byte, or None at end of input.
    fn byte(&mut self) -> io::Result<Option<u8>> {
        self.fill()?;
        if self.pos < self.len {
            self.pos += 1;
            Ok(Some(self.buf[self.pos - 1]))
        } else {
            Ok(None)
        }
    }

    fn need(&mut self, what: &str) -> io::Result<u8> {
        self.byte()?
            .ok_or_else(|| bad(format!("unexpected end of file in {what}")))
    }

    /// Reads a member header. Ok(false) = clean end of input.
    fn header(&mut self) -> io::Result<bool> {
        let b0 = match self.byte()? {
            None if self.members > 0 => return Ok(false),
            None => return Err(bad("empty input (not in gzip format)")),
            Some(b) => b,
        };
        if b0 == 0 && self.members > 0 {
            // gzip: trailing zeros are ignored ("decompression OK, trailing zero bytes ignored").
            while let Some(b) = self.byte()? {
                if b != 0 {
                    return Err(bad("trailing garbage after the last member"));
                }
            }
            return Ok(false);
        }
        let b1 = self.need("header")?;
        if (b0, b1) != (0x1f, 0x8b) {
            return Err(if self.members == 0 {
                bad("not in gzip format")
            } else {
                bad("trailing garbage after the last member")
            });
        }
        if self.need("header")? != 8 {
            return Err(bad("unknown compression method"));
        }
        let flg = self.need("header")?;
        if flg & 0xe0 != 0 {
            return Err(bad("reserved header flags set"));
        }
        for _ in 0..6 {
            self.need("header")?; // MTIME(4) XFL OS
        }
        if flg & 0x04 != 0 {
            let lo = self.need("FEXTRA")? as usize;
            let hi = self.need("FEXTRA")? as usize;
            for _ in 0..(lo | hi << 8) {
                self.need("FEXTRA")?;
            }
        }
        for (bit, what) in [(0x08u8, "FNAME"), (0x10, "FCOMMENT")] {
            if flg & bit != 0 {
                while self.need(what)? != 0 {}
            }
        }
        if flg & 0x02 != 0 {
            self.need("FHCRC")?;
            self.need("FHCRC")?;
        }
        self.st = InflateState::new_boxed(DataFormat::Raw);
        self.crc = 0;
        self.size = 0;
        self.in_member = true;
        self.members += 1;
        Ok(true)
    }

    fn trailer(&mut self) -> io::Result<()> {
        let mut t = [0u8; 8];
        for x in t.iter_mut() {
            *x = self.need("trailer")?;
        }
        let crc = u32::from_le_bytes(t[0..4].try_into().unwrap());
        let isize = u32::from_le_bytes(t[4..8].try_into().unwrap());
        if crc != self.crc {
            return Err(bad(format!(
                "invalid compressed data--crc error (stored {crc:08x}, computed {:08x})",
                self.crc
            )));
        }
        if isize != self.size {
            return Err(bad("invalid compressed data--length error"));
        }
        self.in_member = false;
        Ok(())
    }

    /// Number of gzip members read so far.
    pub fn members(&self) -> u32 {
        self.members
    }
}

impl<R: Read> Read for GzReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.done {
                return Ok(0);
            }
            if !self.in_member {
                if !self.header()? {
                    self.done = true;
                    return Ok(0);
                }
                continue;
            }
            self.fill()?;
            let r = inflate(
                &mut self.st,
                &self.buf[self.pos..self.len],
                out,
                MZFlush::None,
            );
            self.pos += r.bytes_consumed;
            let w = &out[..r.bytes_written];
            self.crc = crc32(self.crc, w);
            self.size = self.size.wrapping_add(w.len() as u32);
            match r.status {
                Ok(MZStatus::StreamEnd) => {
                    self.trailer()?;
                    if !w.is_empty() {
                        return Ok(w.len());
                    }
                }
                Ok(_) | Err(MZError::Buf) => {
                    if !w.is_empty() {
                        return Ok(w.len());
                    }
                    if r.bytes_consumed == 0 && self.pos == self.len && self.eof {
                        return Err(bad("unexpected end of file"));
                    }
                }
                Err(e) => {
                    return Err(bad(format!(
                        "invalid compressed data--format violated ({e:?})"
                    )))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gzip::GzWriter;
    use std::io::Write;

    fn gz(data: &[u8], level: i32) -> Vec<u8> {
        let mut g = GzWriter::new(Vec::new(), level).unwrap();
        g.write_all(data).unwrap();
        g.finish().unwrap()
    }

    fn data(n: usize) -> Vec<u8> {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                if i % 3 == 0 {
                    b'a'
                } else {
                    x as u8
                }
            })
            .collect()
    }

    /// A reader that hands out at most `n` bytes per read (exercises refills).
    struct Trickle<'a>(&'a [u8], usize);
    impl Read for Trickle<'_> {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let k = self.1.min(b.len()).min(self.0.len());
            b[..k].copy_from_slice(&self.0[..k]);
            self.0 = &self.0[k..];
            Ok(k)
        }
    }

    #[test]
    fn roundtrip_levels_and_chunkings() {
        for (n, lvl) in [(0usize, 6), (1, 6), (70_000, 0), (300_000, 6), (300_000, 9)] {
            let d = data(n);
            let z = gz(&d, lvl);
            for t in [1usize, 7, 4096, 1 << 20] {
                let mut out = Vec::new();
                GzReader::new(Trickle(&z, t)).read_to_end(&mut out).unwrap();
                assert_eq!(out, d, "n={n} lvl={lvl} trickle={t}");
            }
        }
    }

    #[test]
    fn concatenated_members_and_zero_padding() {
        let mut z = gz(b"hello ", 6);
        z.extend(gz(b"world", 1));
        z.extend([0u8; 512]);
        let mut r = GzReader::new(&z[..]);
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"hello world");
        assert_eq!(r.members(), 2);
    }

    #[test]
    fn header_flags_skipped() {
        let mut z = gz(b"payload", 6);
        // Set FNAME|FCOMMENT|FEXTRA and insert the fields after the 10-byte header.
        z[3] = 0x04 | 0x08 | 0x10;
        let mut extra = vec![3u8, 0, 1, 2, 3];
        extra.extend(b"name.tar\0comment\0");
        z.splice(10..10, extra);
        let mut out = Vec::new();
        GzReader::new(&z[..]).read_to_end(&mut out).unwrap();
        assert_eq!(out, b"payload");
    }

    fn err_of(z: &[u8]) -> String {
        let mut out = Vec::new();
        GzReader::new(z)
            .read_to_end(&mut out)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn corruption_is_detected() {
        let d = data(100_000);
        let z = gz(&d, 6);
        assert!(err_of(&[]).contains("not in gzip"));
        assert!(err_of(b"PK\x03\x04 zip").contains("not in gzip"));
        assert!(err_of(&z[..z.len() / 2]).contains("unexpected end"));
        assert!(err_of(&z[..z.len() - 3]).contains("unexpected end"));
        let mut c = z.clone();
        let n = c.len();
        c[n - 6] ^= 1; // CRC
        assert!(err_of(&c).contains("crc error"));
        let mut c = z.clone();
        c[n - 2] ^= 1; // ISIZE
        assert!(err_of(&c).contains("length error"));
        let mut c = z.clone();
        c.extend(b"junk");
        assert!(err_of(&c).contains("trailing garbage"));
        let mut c = z.clone();
        c[2] = 7;
        assert!(err_of(&c).contains("compression method"));
    }

    #[test]
    fn flipped_body_byte_never_passes_silently() {
        let d = data(50_000);
        let z = gz(&d, 6);
        for off in [12usize, 100, 1000, z.len() / 2, z.len() - 20] {
            let mut c = z.clone();
            c[off] ^= 0x40;
            let mut out = Vec::new();
            let r = GzReader::new(&c[..]).read_to_end(&mut out);
            assert!(r.is_err() || out == d, "offset {off} silently corrupted");
            assert!(r.is_err(), "offset {off}: flip not detected");
        }
    }
}
