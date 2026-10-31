// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Streaming raw-deflate and gzip readers on `miniz_oxide` 0.8 without its
//! SIMD feature (the same pin as galion-backup), plus CRC-32. flate2 is not
//! used: flate2 1.1 always turns on miniz_oxide's `simd` feature, which pulls
//! `simd-adler32` (UNIVERSAL-CPU.md: no arch SIMD).

use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};
use std::io::{self, BufRead, Read};

/// CRC-32 (IEEE, reflected 0xEDB88320), table-driven, portable.
#[derive(Clone)]
pub struct Crc32(u32);

const fn table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static TABLE: [u32; 256] = table();

impl Default for Crc32 {
    fn default() -> Self {
        Crc32(0xFFFF_FFFF)
    }
}

impl Crc32 {
    pub fn update(&mut self, data: &[u8]) {
        let mut c = self.0;
        for &b in data {
            c = TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        self.0 = c;
    }
    pub fn value(&self) -> u32 {
        !self.0
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::default();
    c.update(data);
    c.value()
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Raw deflate (no zlib/gzip framing) over a `BufRead`; bytes after the end
/// of the deflate stream stay in the inner reader.
pub struct Inflate<R: BufRead> {
    inner: R,
    state: Box<InflateState>,
    done: bool,
}

impl<R: BufRead> Inflate<R> {
    pub fn new(inner: R) -> Self {
        Inflate {
            inner,
            state: InflateState::new_boxed(DataFormat::Raw),
            done: false,
        }
    }
    pub fn into_inner(self) -> R {
        self.inner
    }
    pub fn finished(&self) -> bool {
        self.done
    }
}

impl<R: BufRead> Read for Inflate<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        loop {
            let input = self.inner.fill_buf()?;
            let eof = input.is_empty();
            let flush = if eof { MZFlush::Finish } else { MZFlush::None };
            let r = inflate(&mut self.state, input, buf, flush);
            self.inner.consume(r.bytes_consumed);
            match r.status {
                Ok(MZStatus::StreamEnd) => {
                    self.done = true;
                    return Ok(r.bytes_written);
                }
                Ok(_) | Err(MZError::Buf) => {
                    if r.bytes_written > 0 {
                        return Ok(r.bytes_written);
                    }
                    if eof && r.bytes_consumed == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "deflate stream cut off",
                        ));
                    }
                }
                Err(_) => return Err(bad("corrupt deflate data")),
            }
        }
    }
}

/// Checks CRC-32 and length at EOF (zip members, gzip members).
pub struct Checked<R: Read> {
    pub inner: R,
    crc: Crc32,
    n: u64,
    want: Option<(u32, u64)>,
}

impl<R: Read> Checked<R> {
    pub fn new(inner: R, want_crc: u32, want_len: u64) -> Self {
        Checked {
            inner,
            crc: Crc32::default(),
            n: 0,
            want: Some((want_crc, want_len)),
        }
    }
}

impl<R: Read> Read for Checked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let k = self.inner.read(buf)?;
        if k == 0 && !buf.is_empty() {
            if let Some((c, l)) = self.want {
                if self.n != l {
                    return Err(bad("member length does not match its header"));
                }
                if self.crc.value() != c {
                    return Err(bad("member CRC-32 does not match (corrupt archive)"));
                }
            }
            return Ok(0);
        }
        self.crc.update(&buf[..k]);
        self.n += k as u64;
        Ok(k)
    }
}

/// gzip (RFC 1952), multi-member, CRC + ISIZE checked per member.
pub struct Gz<R: BufRead> {
    z: Option<Inflate<R>>,
    crc: Crc32,
    n: u64,
}

fn read_header<R: BufRead>(r: &mut R) -> io::Result<()> {
    let mut h = [0u8; 10];
    r.read_exact(&mut h)?;
    if h[0] != 0x1f || h[1] != 0x8b || h[2] != 8 {
        return Err(bad("not a gzip stream"));
    }
    let flg = h[3];
    if flg & 4 != 0 {
        let mut x = [0u8; 2];
        r.read_exact(&mut x)?;
        io::copy(
            &mut r.by_ref().take(u16::from_le_bytes(x) as u64),
            &mut io::sink(),
        )?;
    }
    for bit in [8u8, 16] {
        if flg & bit != 0 {
            let mut skip = Vec::new();
            r.read_until(0, &mut skip)?;
        }
    }
    if flg & 2 != 0 {
        r.read_exact(&mut [0u8; 2])?;
    }
    Ok(())
}

impl<R: BufRead> Gz<R> {
    pub fn new(mut inner: R) -> io::Result<Self> {
        read_header(&mut inner)?;
        Ok(Gz {
            z: Some(Inflate::new(inner)),
            crc: Crc32::default(),
            n: 0,
        })
    }
}

impl<R: BufRead> Read for Gz<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let Some(z) = self.z.as_mut() else {
                return Ok(0);
            };
            let k = z.read(buf)?;
            if k > 0 {
                self.crc.update(&buf[..k]);
                self.n += k as u64;
                return Ok(k);
            }
            // Member done: trailer, then maybe another member.
            let mut inner = self
                .z
                .take()
                .map(Inflate::into_inner)
                .unwrap_or_else(|| unreachable!());
            let mut t = [0u8; 8];
            inner.read_exact(&mut t)?;
            let crc = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
            let len = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
            if crc != self.crc.value() || len != self.n as u32 {
                return Err(bad("gzip CRC/length mismatch (corrupt file)"));
            }
            if inner.fill_buf()?.first() == Some(&0x1f) {
                read_header(&mut inner)?;
                self.crc = Crc32::default();
                self.n = 0;
                self.z = Some(Inflate::new(inner));
            } else {
                return Ok(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn raw_round_trip_small_reads() {
        let data: Vec<u8> = (0..200_000u32)
            .map(|i| (i % 251) as u8 ^ (i / 997) as u8)
            .collect();
        let packed = miniz_oxide::deflate::compress_to_vec(&data, 6);
        let mut z = Inflate::new(io::BufReader::with_capacity(7, &packed[..]));
        let mut out = Vec::new();
        let mut b = [0u8; 13];
        loop {
            let k = z.read(&mut b).unwrap();
            if k == 0 {
                break;
            }
            out.extend_from_slice(&b[..k]);
        }
        assert_eq!(out, data);
        // Cut off: an error, not a silent short file.
        let mut z = Inflate::new(&packed[..packed.len() / 2]);
        assert!(io::copy(&mut z, &mut io::sink()).is_err());
    }

    fn gz(data: &[u8]) -> Vec<u8> {
        let mut v = vec![0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 3];
        v.extend_from_slice(b"name.tar\0");
        v.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 6));
        v.extend_from_slice(&crc32(data).to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v
    }

    #[test]
    fn gzip_multi_member_and_crc() {
        let mut two = gz(b"hello ");
        two.extend(gz(b"world"));
        let mut s = String::new();
        Gz::new(&two[..]).unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "hello world");
        let mut broken = gz(b"hello");
        let n = broken.len();
        broken[n - 8] ^= 1;
        assert!(io::copy(&mut Gz::new(&broken[..]).unwrap(), &mut io::sink()).is_err());
        assert!(Gz::new(&b"PK\x03\x04xxxxxxxx"[..]).is_err());
    }
}
