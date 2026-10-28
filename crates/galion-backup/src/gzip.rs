// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Streaming gzip writer (RFC 1952) over miniz_oxide's raw deflate, level 6
//! (what `tar -z` -> `gzip` uses by default). Header as `gzip` writes it for
//! stdin input: no file name, MTIME 0, OS 3 (Unix). CRC32 is ours (slicing-by-8).
//! Level: `GALION_BACKUP_GZIP_LEVEL` (0-9, default 6 = gzip default).

use miniz_oxide::deflate::core::{create_comp_flags_from_zip_params, CompressorOxide};
use miniz_oxide::deflate::stream::deflate;
use miniz_oxide::{MZFlush, MZStatus};
use std::io::{self, Write};

/// Slicing-by-8 tables (Intel "slicing-by-8", public-domain technique):
/// `T[0]` is the classic byte table, `T[k][i]` = CRC of byte i followed by
/// k zero bytes. Pure scalar Rust, no SIMD: ~8x fewer dependent table
/// lookups per byte than the bytewise loop (66 MB: 0.155 s -> 0.034 s,
/// docs/real-tests/backup-perf.md).
const fn crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut s = 1;
    while s < 8 {
        let mut i = 0;
        while i < 256 {
            let p = t[s - 1][i];
            t[s][i] = (p >> 8) ^ t[0][(p & 0xff) as usize];
            i += 1;
        }
        s += 1;
    }
    t
}
static CRC: [[u32; 256]; 8] = crc_tables();

/// CRC-32 (IEEE), incremental: pass the previous value (start with 0).
pub fn crc32(prev: u32, data: &[u8]) -> u32 {
    let t = &CRC;
    let mut c = !prev;
    let (chunks, rest) = data.as_chunks::<8>();
    for b in chunks {
        let lo = c ^ u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let hi = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in rest {
        c = t[0][((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

pub struct GzWriter<W: Write> {
    out: W,
    comp: Box<CompressorOxide>,
    crc: u32,
    size: u32,
    buf: Vec<u8>,
}

impl<W: Write> GzWriter<W> {
    pub fn new(mut out: W, level: i32) -> io::Result<Self> {
        // ID1 ID2 CM=8 FLG=0 MTIME=0 XFL=0 OS=3
        out.write_all(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3])?;
        let flags = create_comp_flags_from_zip_params(level, -15, 0);
        Ok(GzWriter {
            out,
            comp: Box::new(CompressorOxide::new(flags)),
            crc: 0,
            size: 0,
            buf: vec![0u8; 64 * 1024],
        })
    }

    fn pump(&mut self, mut input: &[u8], flush: MZFlush) -> io::Result<()> {
        loop {
            let r = deflate(&mut self.comp, input, &mut self.buf, flush);
            input = &input[r.bytes_consumed..];
            self.out.write_all(&self.buf[..r.bytes_written])?;
            match r.status {
                Ok(MZStatus::StreamEnd) => return Ok(()),
                Ok(_) => {
                    if flush != MZFlush::Finish
                        && input.is_empty()
                        && r.bytes_written < self.buf.len()
                    {
                        return Ok(());
                    }
                }
                Err(miniz_oxide::MZError::Buf) if flush != MZFlush::Finish && input.is_empty() => {
                    return Ok(())
                }
                Err(e) => return Err(io::Error::other(format!("deflate: {e:?}"))),
            }
        }
    }

    /// Flush the deflate stream and write the gzip trailer.
    pub fn finish(mut self) -> io::Result<W> {
        self.pump(&[], MZFlush::Finish)?;
        let (crc, size) = (self.crc, self.size);
        self.out.write_all(&crc.to_le_bytes())?;
        self.out.write_all(&size.to_le_bytes())?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Write for GzWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.crc = crc32(self.crc, data);
        self.size = self.size.wrapping_add(data.len() as u32);
        self.pump(data, MZFlush::None)?;
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bit-at-a-time reference (no tables) for the slicing-by-8 path.
    fn crc_ref(prev: u32, data: &[u8]) -> u32 {
        let mut c = !prev;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    #[test]
    fn slicing_by_8_matches_bitwise_reference() {
        let mut x = 0x1234_5678_9abc_def0u64;
        let data: Vec<u8> = (0..5000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect();
        for start in 0..9 {
            for len in [
                0usize,
                1,
                7,
                8,
                9,
                15,
                16,
                17,
                63,
                64,
                65,
                1000,
                4991 - start,
            ] {
                let d = &data[start..start + len];
                assert_eq!(crc32(0, d), crc_ref(0, d), "start {start} len {len}");
                let (a, b) = d.split_at(len / 3);
                assert_eq!(crc32(crc32(0, a), b), crc_ref(0, d));
            }
        }
        assert_eq!(crc32(0, &[]), 0);
    }
    #[test]
    fn crc_vector() {
        assert_eq!(crc32(0, b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xcbf4_3926);
    }

    #[test]
    fn roundtrip_with_miniz_inflate() {
        let data: Vec<u8> = (0..300_000u32)
            .map(|i| (i % 251) as u8 ^ (i >> 9) as u8)
            .collect();
        let mut g = GzWriter::new(Vec::new(), 6).unwrap();
        for c in data.chunks(7777) {
            g.write_all(c).unwrap();
        }
        let gz = g.finish().unwrap();
        assert_eq!(&gz[..3], &[0x1f, 0x8b, 8]);
        let body = &gz[10..gz.len() - 8];
        let back = miniz_oxide::inflate::decompress_to_vec(body).unwrap();
        assert_eq!(back, data);
        let n = gz.len();
        assert_eq!(
            u32::from_le_bytes(gz[n - 8..n - 4].try_into().unwrap()),
            crc32(0, &data)
        );
        assert_eq!(
            u32::from_le_bytes(gz[n - 4..].try_into().unwrap()),
            data.len() as u32
        );
    }
}
