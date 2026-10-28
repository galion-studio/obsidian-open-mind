// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Streaming reader for GNU and POSIX (ustar / minimal pax) archives.
//!
//! Handles everything [`crate::gnu::TarWriter`] and [`crate::ustar`] write,
//! plus what GNU tar 1.35 writes by default: `././@LongLink` names and link
//! targets (`L`/`K`), base-256 numbers, hard links, char/block devices, fifos,
//! the ustar `prefix` field (POSIX magic only), and pax `x` records for path,
//! linkpath, size, uid, gid, uname, gname and mtime (integer seconds). pax
//! `g` (global) records are skipped. Every header checksum is verified
//! (unsigned or historic signed sum). A truncated archive is an error.

use std::io::{self, Read, Write};

use crate::num::{checksum, checksum_signed, parse};
use crate::BLOCK;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// `0`, NUL or `7` (contiguous, treated as regular by GNU tar).
    File,
    /// `1`: `link` names the earlier member.
    Hardlink,
    /// `2`
    Symlink,
    /// `3`
    Char,
    /// `4`
    Block,
    /// `5`
    Dir,
    /// `6`
    Fifo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub kind: MemberKind,
    /// Member name as stored; directories keep no trailing '/'.
    pub name: Vec<u8>,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Data bytes that follow the header (0 for links, dirs, devices).
    pub size: u64,
    pub mtime: i64,
    pub link: Vec<u8>,
    pub uname: Vec<u8>,
    pub gname: Vec<u8>,
    /// (major, minor) for `Char`/`Block`.
    pub dev: Option<(u32, u32)>,
}

impl Member {
    /// Name as UTF-8 (lossy), for messages.
    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }
}

fn cstr(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&x| x == 0).unwrap_or(b.len());
    &b[..end]
}

fn bad(m: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.into())
}

fn num_u32(f: &[u8], what: &str) -> io::Result<u32> {
    u32::try_from(parse(f)?).map_err(|_| bad(format!("{what} out of range")))
}

#[derive(Default)]
struct Pending {
    long_name: Option<Vec<u8>>,
    long_link: Option<Vec<u8>>,
    pax: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Reads members one at a time; the current member's data is read with
/// [`Reader::read_data`] / [`Reader::copy_data`] or skipped automatically by
/// the next [`Reader::next_member`].
pub struct Reader<R: Read> {
    r: R,
    /// Unread data bytes of the current member, then its padding.
    left: u64,
    pad: u64,
    done: bool,
}

impl<R: Read> Reader<R> {
    pub fn new(r: R) -> Self {
        Reader {
            r,
            left: 0,
            pad: 0,
            done: false,
        }
    }

    fn block(&mut self) -> io::Result<[u8; BLOCK]> {
        let mut b = [0u8; BLOCK];
        self.r.read_exact(&mut b).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                bad("truncated archive (no end-of-archive blocks)")
            } else {
                e
            }
        })?;
        Ok(b)
    }

    fn skip(&mut self, mut n: u64) -> io::Result<()> {
        let mut buf = [0u8; 8192];
        while n > 0 {
            let k = n.min(buf.len() as u64) as usize;
            self.r.read_exact(&mut buf[..k]).map_err(|e| {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    bad("truncated archive (member data cut off)")
                } else {
                    e
                }
            })?;
            n -= k as u64;
        }
        Ok(())
    }

    fn finish_member(&mut self) -> io::Result<()> {
        let n = self.left + self.pad;
        self.left = 0;
        self.pad = 0;
        self.skip(n)
    }

    fn read_small(&mut self, size: u64, what: &str) -> io::Result<Vec<u8>> {
        if size > 1 << 20 {
            return Err(bad(format!("{what} record larger than 1 MiB")));
        }
        let mut v = vec![0u8; size as usize];
        self.r
            .read_exact(&mut v)
            .map_err(|_| bad(format!("truncated {what} record")))?;
        self.skip((BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64)?;
        Ok(v)
    }

    /// Next member header, or `None` at the end-of-archive marker.
    pub fn next_member(&mut self) -> io::Result<Option<Member>> {
        if self.done {
            return Ok(None);
        }
        self.finish_member()?;
        let mut p = Pending::default();
        loop {
            let h = self.block()?;
            if h.iter().all(|&b| b == 0) {
                // GNU tar stops at the first zero block too (it only warns
                // about a missing second one).
                if p.long_name.is_some() || p.long_link.is_some() || !p.pax.is_empty() {
                    return Err(bad("archive ends after an extended header"));
                }
                self.done = true;
                return Ok(None);
            }
            let want = parse(&h[148..156])?;
            if want != i64::from(checksum(&h)) && want != checksum_signed(&h) {
                return Err(bad("tar header checksum mismatch"));
            }
            let size = parse(&h[124..136])?;
            let size = u64::try_from(size).map_err(|_| bad("negative member size"))?;
            match h[156] {
                b'L' => {
                    p.long_name = Some(cstr(&self.read_small(size, "long name")?).to_vec());
                    continue;
                }
                b'K' => {
                    p.long_link = Some(cstr(&self.read_small(size, "long link")?).to_vec());
                    continue;
                }
                b'x' => {
                    let rec = self.read_small(size, "pax")?;
                    p.pax.extend(parse_pax(&rec)?);
                    continue;
                }
                b'g' => {
                    self.read_small(size, "pax global")?;
                    continue;
                }
                _ => {}
            }
            return self.member(&h, size, p).map(Some);
        }
    }

    fn member(&mut self, h: &[u8; BLOCK], mut size: u64, p: Pending) -> io::Result<Member> {
        let kind = match h[156] {
            b'0' | 0 | b'7' => MemberKind::File,
            b'1' => MemberKind::Hardlink,
            b'2' => MemberKind::Symlink,
            b'3' => MemberKind::Char,
            b'4' => MemberKind::Block,
            b'5' => MemberKind::Dir,
            b'6' => MemberKind::Fifo,
            t => return Err(bad(format!("unsupported tar member type {:?}", t as char))),
        };
        let posix = &h[257..263] == b"ustar\0";
        let mut name = cstr(&h[..100]).to_vec();
        if posix {
            let prefix = cstr(&h[345..500]);
            if !prefix.is_empty() {
                let mut full = prefix.to_vec();
                full.push(b'/');
                full.extend_from_slice(&name);
                name = full;
            }
        }
        if let Some(n) = p.long_name {
            name = n;
        }
        let mut link = cstr(&h[157..257]).to_vec();
        if let Some(l) = p.long_link {
            link = l;
        }
        let mut m = Member {
            kind,
            name,
            mode: num_u32(&h[100..108], "mode")?,
            uid: num_u32(&h[108..116], "uid")?,
            gid: num_u32(&h[116..124], "gid")?,
            size: 0,
            mtime: parse(&h[136..148])?,
            link,
            uname: cstr(&h[265..297]).to_vec(),
            gname: cstr(&h[297..329]).to_vec(),
            dev: None,
        };
        if matches!(kind, MemberKind::Char | MemberKind::Block) {
            m.dev = Some((
                num_u32(&h[329..337], "devmajor")?,
                num_u32(&h[337..345], "devminor")?,
            ));
        }
        for (k, v) in p.pax {
            let num = || -> io::Result<i64> {
                let s = std::str::from_utf8(&v).map_err(|_| bad("pax number not UTF-8"))?;
                let s = s.split('.').next().unwrap_or("");
                s.parse::<i64>().map_err(|_| bad("bad pax number"))
            };
            match k.as_slice() {
                b"path" => m.name = v,
                b"linkpath" => m.link = v,
                b"uname" => m.uname = v,
                b"gname" => m.gname = v,
                b"size" => size = u64::try_from(num()?).map_err(|_| bad("bad pax size"))?,
                b"uid" => m.uid = u32::try_from(num()?).map_err(|_| bad("bad pax uid"))?,
                b"gid" => m.gid = u32::try_from(num()?).map_err(|_| bad("bad pax gid"))?,
                b"mtime" => m.mtime = num()?,
                _ => {}
            }
        }
        if kind == MemberKind::Dir {
            while m.name.len() > 1 && m.name.ends_with(b"/") {
                m.name.pop();
            }
        }
        // Hard links, symlinks, devices and fifos carry no data blocks.
        let data_bearing = matches!(kind, MemberKind::File | MemberKind::Dir);
        let data = if data_bearing { size } else { 0 };
        m.size = data;
        self.left = data;
        self.pad = (BLOCK as u64 - data % BLOCK as u64) % BLOCK as u64;
        Ok(m)
    }

    /// Read the rest of the current member's data into memory.
    pub fn read_data(&mut self) -> io::Result<Vec<u8>> {
        let mut v = Vec::with_capacity(self.left.min(64 << 20) as usize);
        self.copy_data(&mut v)?;
        Ok(v)
    }

    /// Stream the rest of the current member's data into `w`.
    pub fn copy_data(&mut self, w: &mut dyn Write) -> io::Result<u64> {
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0;
        while self.left > 0 {
            let k = self.left.min(buf.len() as u64) as usize;
            self.r
                .read_exact(&mut buf[..k])
                .map_err(|_| bad("truncated archive (member data cut off)"))?;
            w.write_all(&buf[..k])?;
            self.left -= k as u64;
            total += k as u64;
        }
        Ok(total)
    }

    pub fn into_inner(self) -> R {
        self.r
    }
}

/// Parse pax `"<len> <key>=<value>\n"` records.
fn parse_pax(mut rec: &[u8]) -> io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut out = vec![];
    while !rec.is_empty() && rec[0] != 0 {
        let sp = rec
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| bad("bad pax record"))?;
        let len: usize = std::str::from_utf8(&rec[..sp])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| bad("bad pax record length"))?;
        if len <= sp + 1 || len > rec.len() || rec[len - 1] != b'\n' {
            return Err(bad("bad pax record length"));
        }
        let kv = &rec[sp + 1..len - 1];
        let eq = kv
            .iter()
            .position(|&b| b == b'=')
            .ok_or_else(|| bad("bad pax record"))?;
        out.push((kv[..eq].to_vec(), kv[eq + 1..].to_vec()));
        rec = &rec[len..];
    }
    Ok(out)
}

/// Read every member and its data into memory.
pub fn read_all<R: Read>(r: R) -> io::Result<Vec<(Member, Vec<u8>)>> {
    let mut rd = Reader::new(r);
    let mut out = vec![];
    while let Some(m) = rd.next_member()? {
        let d = rd.read_data()?;
        out.push((m, d));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ustar::{self, Entry, Kind};

    #[test]
    fn reads_ustar_entries_with_prefix() {
        let long = format!("var/lib/{}/state.json", "d".repeat(120));
        let mut buf = vec![];
        for (k, n, d) in [
            (Kind::Dir, "opt/galion", &b""[..]),
            (Kind::File, long.as_str(), b"{}"),
            (Kind::Symlink, "l", b""),
        ] {
            let e = Entry {
                kind: k,
                name: n.into(),
                mode: 0o750,
                uid: 1,
                gid: 2,
                mtime: 1_760_000_000,
                link: if k == Kind::Symlink {
                    "t".into()
                } else {
                    String::new()
                },
                data: d.to_vec(),
            };
            ustar::write_entry(&mut buf, &e).unwrap();
        }
        ustar::finish(&mut buf).unwrap();
        let all = read_all(buf.as_slice()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].0.kind, MemberKind::Dir);
        assert_eq!(all[0].0.name, b"opt/galion");
        assert_eq!(all[1].0.name, long.as_bytes());
        assert_eq!(all[1].1, b"{}");
        assert_eq!((all[1].0.mode, all[1].0.uid, all[1].0.gid), (0o750, 1, 2));
        assert_eq!(all[2].0.link, b"t");
    }

    #[test]
    fn pax_records_override_header() {
        let rec = b"20 path=a/very/long\n14 linkpath=x\n11 size=11\n";
        let p = parse_pax(rec).unwrap();
        assert_eq!(p[0], (b"path".to_vec(), b"a/very/long".to_vec()));
        assert_eq!(p[2], (b"size".to_vec(), b"11".to_vec()));
        assert!(parse_pax(b"99 path=x\n").is_err());
    }

    #[test]
    fn truncation_and_corruption_are_errors() {
        let mut buf = vec![];
        let e = Entry {
            kind: Kind::File,
            name: "a".into(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
            link: String::new(),
            data: vec![1; 600],
        };
        ustar::write_entry(&mut buf, &e).unwrap();
        let cut = buf.clone();
        assert!(read_all(&cut[..700]).is_err());
        assert!(read_all(cut.as_slice()).is_err()); // no end marker
        ustar::finish(&mut buf).unwrap();
        assert!(read_all(buf.as_slice()).is_ok());
        buf[0] ^= 1;
        assert!(read_all(buf.as_slice()).is_err());
    }
}
