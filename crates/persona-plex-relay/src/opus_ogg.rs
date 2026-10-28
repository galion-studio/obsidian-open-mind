// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `OpusOgg`: the PersonaPlex / moshi wire audio format (Opus in Ogg pages,
//! 24 kHz mono) as an [`AudioCodec`]. Feature `opus` (off by default).
//!
//! Codec: `opus-pure` 0.2.2 (BSD-3-Clause, pure Rust, no C, no build.rs). Its
//! audit (docs/ws-voice.md section 7) found runtime-dispatched SIMD kernels
//! inside the crate (AVX/AVX2 on x86_64 with scalar fallback, NEON on aarch64),
//! which is why this lives behind a non-default feature until Slavik accepts
//! that exception to UNIVERSAL-CPU.md.
//!
//! The Ogg layer is ours and streaming: every `encode()` returns complete pages
//! (the first call also the OpusHead + OpusTags pages), and `decode()` accepts
//! arbitrary byte chunks, buffers partial pages, verifies page CRCs and returns
//! whatever PCM the complete packets produce. opus-pure's own Ogg writer holds
//! one packet back before flushing a page, which adds a frame of latency, and
//! its reader wants a blocking `Read` source, so neither fits a live socket.

use opus_pure::{Application, OpusDecoder, OpusEncoder, OpusHead};

use crate::backend::AudioCodec;
use galion_voice_core::audio::Frame;

/// Moshi rate and 20 ms Opus frames (an 80 ms core frame = 4 Opus packets).
pub const RATE: u32 = 24_000;
pub const OPUS_FRAME: usize = 480;

// ---- Ogg CRC (polynomial 0x04c11db7, no reflection, init 0) ----

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut r = (i as u32) << 24;
        let mut j = 0;
        while j < 8 {
            r = if r & 0x8000_0000 != 0 {
                (r << 1) ^ 0x04c1_1db7
            } else {
                r << 1
            };
            j += 1;
        }
        t[i] = r;
        i += 1;
    }
    t
}
static CRC: [u32; 256] = crc_table();

pub fn ogg_crc(data: &[u8]) -> u32 {
    let mut c = 0u32;
    for &b in data {
        c = (c << 8) ^ CRC[((c >> 24) as u8 ^ b) as usize];
    }
    c
}

/// Build one Ogg page holding whole packets (each < 255*255 bytes).
pub fn ogg_page(
    serial: u32,
    seq: u32,
    granule: i64,
    header_type: u8,
    packets: &[&[u8]],
) -> Vec<u8> {
    let mut lacing = Vec::new();
    for p in packets {
        let mut n = p.len();
        while n >= 255 {
            lacing.push(255u8);
            n -= 255;
        }
        lacing.push(n as u8);
    }
    let mut page =
        Vec::with_capacity(27 + lacing.len() + packets.iter().map(|p| p.len()).sum::<usize>());
    page.extend_from_slice(b"OggS");
    page.push(0);
    page.push(header_type);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&serial.to_le_bytes());
    page.extend_from_slice(&seq.to_le_bytes());
    page.extend_from_slice(&[0; 4]);
    page.push(lacing.len() as u8);
    page.extend_from_slice(&lacing);
    for p in packets {
        page.extend_from_slice(p);
    }
    let crc = ogg_crc(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

/// Incremental Ogg demuxer: feed bytes, get complete packets.
#[derive(Debug, Default)]
pub struct OggDemux {
    buf: Vec<u8>,
    partial: Vec<u8>,
    pub pages: u64,
    pub bad_crc: u64,
}

impl OggDemux {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            // Resync to the next capture pattern.
            match self.buf.windows(4).position(|w| w == b"OggS") {
                Some(0) => {}
                Some(i) => {
                    self.buf.drain(..i);
                }
                None => {
                    let keep = self.buf.len().min(3);
                    self.buf.drain(..self.buf.len() - keep);
                    return out;
                }
            }
            if self.buf.len() < 27 {
                return out;
            }
            let nseg = self.buf[26] as usize;
            if self.buf.len() < 27 + nseg {
                return out;
            }
            let body: usize = self.buf[27..27 + nseg].iter().map(|&l| l as usize).sum();
            let total = 27 + nseg + body;
            if self.buf.len() < total {
                return out;
            }
            let mut page = self.buf[..total].to_vec();
            let want = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
            page[22..26].copy_from_slice(&[0; 4]);
            if ogg_crc(&page) != want || page[4] != 0 {
                // Corrupt: skip this capture pattern and resync.
                self.bad_crc += 1;
                self.partial.clear();
                self.buf.drain(..4);
                continue;
            }
            self.pages += 1;
            let continued = page[5] & 1 != 0;
            if !continued {
                self.partial.clear();
            }
            let mut pos = 27 + nseg;
            for &l in &page[27..27 + nseg] {
                self.partial.extend_from_slice(&page[pos..pos + l as usize]);
                pos += l as usize;
                if l < 255 {
                    out.push(std::mem::take(&mut self.partial));
                }
            }
            self.buf.drain(..total);
        }
    }
}

fn opus_head(pre_skip: u16) -> Vec<u8> {
    let mut h = b"OpusHead".to_vec();
    h.push(1); // version
    h.push(1); // channels
    h.extend_from_slice(&pre_skip.to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&0i16.to_le_bytes()); // gain
    h.push(0); // mapping family 0
    h
}

fn opus_tags() -> Vec<u8> {
    let vendor = b"galion persona-plex-relay";
    let mut t = b"OpusTags".to_vec();
    t.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    t.extend_from_slice(vendor);
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

/// Opus-in-Ogg codec, 24 kHz mono, one page per encoded core frame.
pub struct OpusOgg {
    enc: OpusEncoder,
    dec: OpusDecoder,
    demux: OggDemux,
    serial: u32,
    seq: u32,
    granule: i64,
    pre_skip: u16,
    headers_sent: bool,
    pending: Vec<i16>,
    pub packets_in: u64,
    pub errors: u64,
}

impl OpusOgg {
    /// `serial` identifies our logical stream; any value works.
    pub fn new(serial: u32, bitrate_bps: i32) -> Result<Self, String> {
        let mut enc =
            OpusEncoder::new(RATE as i32, 1, Application::Voip).map_err(|e| e.to_string())?;
        enc.bitrate_bps = bitrate_bps;
        let pre_skip = OpusHead::for_encoder(&enc, RATE).pre_skip;
        let dec = OpusDecoder::new(RATE as i32, 1).map_err(|e| e.to_string())?;
        Ok(OpusOgg {
            enc,
            dec,
            demux: OggDemux::default(),
            serial,
            seq: 0,
            granule: 0,
            pre_skip,
            headers_sent: false,
            pending: Vec::new(),
            packets_in: 0,
            errors: 0,
        })
    }

    pub fn pre_skip(&self) -> u16 {
        self.pre_skip
    }
}

impl AudioCodec for OpusOgg {
    fn encode(&mut self, frame: &Frame) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.headers_sent {
            out.extend(ogg_page(
                self.serial,
                0,
                0,
                0x02,
                &[&opus_head(self.pre_skip)],
            ));
            out.extend(ogg_page(self.serial, 1, 0, 0, &[&opus_tags()]));
            self.seq = 2;
            self.headers_sent = true;
        }
        self.pending.extend_from_slice(&frame.samples);
        let mut packets: Vec<Vec<u8>> = Vec::new();
        let mut buf = [0u8; 1500];
        while self.pending.len() >= OPUS_FRAME {
            let pcm: Vec<f32> = self
                .pending
                .drain(..OPUS_FRAME)
                .map(|s| s as f32 / 32768.0)
                .collect();
            match self.enc.encode(&pcm, OPUS_FRAME, &mut buf) {
                Ok(n) => {
                    packets.push(buf[..n].to_vec());
                    self.granule += (OPUS_FRAME as i64) * 2; // 48 kHz granule units
                }
                Err(_) => self.errors += 1,
            }
        }
        if !packets.is_empty() {
            let refs: Vec<&[u8]> = packets.iter().map(|p| p.as_slice()).collect();
            out.extend(ogg_page(self.serial, self.seq, self.granule, 0, &refs));
            self.seq += 1;
        }
        out
    }

    fn decode(&mut self, payload: &[u8]) -> Result<Vec<i16>, String> {
        let mut pcm = Vec::new();
        let mut buf = vec![0f32; 5760];
        for p in self.demux.push(payload) {
            if p.starts_with(b"OpusHead") || p.starts_with(b"OpusTags") || p.is_empty() {
                continue;
            }
            self.packets_in += 1;
            match self.dec.decode(&p, 5760, &mut buf) {
                Ok(n) => pcm.extend(
                    buf[..n]
                        .iter()
                        .map(|&v| (v.clamp(-1.0, 1.0) * 32767.0) as i16),
                ),
                Err(_) => self.errors += 1,
            }
        }
        Ok(pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galion_voice_core::audio::synth;
    use std::io::Cursor;

    fn frame(i: usize) -> Frame {
        Frame {
            seq: i as u64,
            samples: synth::tone(RATE, 300.0, 0.3, 1920, i * 1920),
        }
    }

    #[test]
    fn crc_matches_known_value() {
        // Ogg CRC-32 of "123456789" (poly 0x04c11db7, init 0, no reflect, no xorout).
        assert_eq!(ogg_crc(b"123456789"), 0x89a1_897f);
    }

    #[test]
    fn our_stream_is_readable_by_opus_pure_reader() {
        let mut c = OpusOgg::new(7, 24_000).unwrap();
        let mut bytes = Vec::new();
        for i in 0..10 {
            bytes.extend(c.encode(&frame(i)));
        }
        let mut r = opus_pure::OggOpusReader::new(Cursor::new(bytes)).unwrap();
        assert_eq!(r.head().channel_count, 1);
        assert_eq!(r.head().pre_skip, c.pre_skip());
        let n = r.packets().filter_map(|p| p.ok()).count();
        assert_eq!(n, 40, "10 core frames x 4 Opus packets");
    }

    #[test]
    fn opus_pure_writer_stream_decodes_through_our_demux_in_odd_chunks() {
        let mut enc = OpusEncoder::new(24_000, 1, Application::Voip).unwrap();
        let head = OpusHead::for_encoder(&enc, 24_000);
        let mut w = opus_pure::OggOpusWriter::new(Vec::new(), head).unwrap();
        let mut buf = [0u8; 1500];
        let tone = synth::tone(RATE, 300.0, 0.3, OPUS_FRAME * 50, 0);
        for chunk in tone.chunks(OPUS_FRAME) {
            let pcm: Vec<f32> = chunk.iter().map(|&s| s as f32 / 32768.0).collect();
            let n = enc.encode(&pcm, OPUS_FRAME, &mut buf).unwrap();
            w.write_packet(&buf[..n]).unwrap();
        }
        let bytes = w.finish().unwrap();
        let mut c = OpusOgg::new(1, 24_000).unwrap();
        let mut pcm = Vec::new();
        for chunk in bytes.chunks(97) {
            pcm.extend(c.decode(chunk).unwrap());
        }
        assert_eq!(c.packets_in, 50);
        assert_eq!(pcm.len(), 50 * OPUS_FRAME);
        let r = galion_voice_core::audio::rms(&pcm[OPUS_FRAME * 5..]);
        assert!((r - 0.212).abs() < 0.03, "decoded tone rms {r}");
    }

    #[test]
    fn round_trip_and_corruption_resync() {
        let mut tx = OpusOgg::new(9, 32_000).unwrap();
        let mut rx = OpusOgg::new(9, 32_000).unwrap();
        let mut got = 0;
        for i in 0..20 {
            let mut page = tx.encode(&frame(i));
            if i == 10 {
                let l = page.len();
                page[l - 3] ^= 0xFF; // corrupt one audio page
            }
            got += rx.decode(&page).unwrap().len();
        }
        assert_eq!(rx.demux.bad_crc, 1);
        assert_eq!(got, 19 * 1920, "only the corrupted page is lost");
        assert_eq!(rx.errors, 0);
        // Garbage never panics.
        let mut x = 1u32;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let junk: Vec<u8> = (0..(x % 600)).map(|k| (x >> (k % 24)) as u8).collect();
            let _ = rx.decode(&junk);
            let _ = rx.decode(b"OggS");
        }
    }
}
