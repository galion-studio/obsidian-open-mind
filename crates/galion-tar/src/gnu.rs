// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Minimal GNU-format tar writer with the `tar -c` semantics the bash backup
//! relied on (GNU tar 1.35 defaults, no -h): symlinks stored as symlinks,
//! directories walked in readdir order (`--sort=none`), hard links detected,
//! fifos/devices stored, sockets skipped, uname/gname resolved like tar
//! (through [`Names`]), long names via `././@LongLink`, archive padded to the
//! 10240-byte record. Files that vanish mid-walk are skipped and files that
//! shrink are zero-padded (GNU tar: warning + exit 1, which the script
//! tolerated); every other error is fatal (GNU tar exit 2).
//!
//! Moved verbatim from galion-backup `src/tarw.rs` (rust-ports 26a2a8e); the
//! only changes are the pluggable name lookup and a std-only major/minor split.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

use crate::names::{EtcNames, Names};
use crate::num::{dev_split, octal_or_256};
use crate::{BLOCK, RECORD};

/// Exclusion predicate on the member name (no leading `/`, no trailing `/`).
pub type Exclude = dyn Fn(&[u8]) -> bool;

pub struct TarWriter<W: Write> {
    out: W,
    written: u64,
    links: HashMap<(u64, u64), Vec<u8>>,
    users: HashMap<u32, Vec<u8>>,
    groups: HashMap<u32, Vec<u8>>,
    names: Box<dyn Names>,
    /// Polled between blocks; returning true aborts with `Interrupted`.
    pub cancel: fn() -> bool,
}

fn never() -> bool {
    false
}

fn put(field: &mut [u8], s: &[u8]) {
    let k = s.len().min(field.len());
    field[..k].copy_from_slice(&s[..k]);
}

pub(crate) struct Hdr {
    pub name: Vec<u8>,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: i64,
    pub kind: u8,
    pub link: Vec<u8>,
    pub uname: Vec<u8>,
    pub gname: Vec<u8>,
    pub dev: Option<(u32, u32)>,
}

pub(crate) fn header_block(h: &Hdr) -> [u8; BLOCK] {
    let mut b = [0u8; BLOCK];
    put(&mut b[0..100], &h.name);
    octal_or_256(&mut b[100..108], (h.mode & 0o7777) as i64);
    octal_or_256(&mut b[108..116], h.uid as i64);
    octal_or_256(&mut b[116..124], h.gid as i64);
    octal_or_256(&mut b[124..136], h.size as i64);
    octal_or_256(&mut b[136..148], h.mtime);
    b[156] = h.kind;
    put(&mut b[157..257], &h.link);
    b[257..263].copy_from_slice(b"ustar ");
    b[263..265].copy_from_slice(b" \0");
    put(&mut b[265..297], &h.uname);
    put(&mut b[297..329], &h.gname);
    if let Some((ma, mi)) = h.dev {
        octal_or_256(&mut b[329..337], ma as i64);
        octal_or_256(&mut b[337..345], mi as i64);
    }
    b[148..156].copy_from_slice(b"        ");
    let sum: u32 = b.iter().map(|&x| x as u32).sum();
    let s = format!("{sum:06o}");
    b[148..154].copy_from_slice(s.as_bytes());
    b[154] = 0;
    b[155] = b' ';
    b
}

fn vanished(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound
}

impl<W: Write> TarWriter<W> {
    /// Writer with std-only `/etc/passwd` + `/etc/group` name lookups.
    pub fn new(out: W) -> Self {
        Self::with_names(out, Box::new(EtcNames::default()))
    }

    /// Writer with a caller-supplied name source (looked up once per id).
    pub fn with_names(out: W, names: Box<dyn Names>) -> Self {
        TarWriter {
            out,
            written: 0,
            links: HashMap::new(),
            users: HashMap::new(),
            groups: HashMap::new(),
            names,
            cancel: never,
        }
    }

    /// Bytes written so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    fn emit(&mut self, data: &[u8]) -> io::Result<()> {
        self.out.write_all(data)?;
        self.written += data.len() as u64;
        Ok(())
    }

    fn check_cancel(&self) -> io::Result<()> {
        if (self.cancel)() {
            Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"))
        } else {
            Ok(())
        }
    }

    fn long(&mut self, kind: u8, v: &[u8]) -> io::Result<()> {
        let mut data = v.to_vec();
        data.push(0);
        let h = Hdr {
            name: b"././@LongLink".to_vec(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            size: data.len() as u64,
            mtime: 0,
            kind,
            link: Vec::new(),
            uname: b"root".to_vec(),
            gname: b"root".to_vec(),
            dev: None,
        };
        self.emit(&header_block(&h))?;
        self.emit(&data)?;
        self.pad(data.len() as u64)
    }

    fn pad(&mut self, n: u64) -> io::Result<()> {
        let r = (n % BLOCK as u64) as usize;
        if r != 0 {
            self.emit(&[0u8; BLOCK][..BLOCK - r])?;
        }
        Ok(())
    }

    fn write_header(&mut self, h: &Hdr) -> io::Result<()> {
        if h.link.len() > 100 {
            self.long(b'K', &h.link.clone())?;
        }
        if h.name.len() > 100 {
            self.long(b'L', &h.name.clone())?;
        }
        self.emit(&header_block(h))
    }

    fn base_header(&mut self, name: Vec<u8>, md: &fs::Metadata, kind: u8) -> Hdr {
        let (uid, gid) = (md.uid(), md.gid());
        let names = &mut self.names;
        let uname = self
            .users
            .entry(uid)
            .or_insert_with(|| names.user(uid))
            .clone();
        let gname = self
            .groups
            .entry(gid)
            .or_insert_with(|| names.group(gid))
            .clone();
        Hdr {
            name,
            mode: md.mode(),
            uid,
            gid,
            size: 0,
            mtime: md.mtime(),
            kind,
            link: Vec::new(),
            uname,
            gname,
            dev: None,
        }
    }

    /// Archive `fs_path` (and, for a directory, everything under it) as
    /// member `name`. Like `tar -c NAME`: a missing path is skipped.
    pub fn add_tree(&mut self, fs_path: &Path, name: &[u8], exclude: &Exclude) -> io::Result<()> {
        self.check_cancel()?;
        if exclude(name) {
            return Ok(());
        }
        let md = match fs::symlink_metadata(fs_path) {
            Ok(m) => m,
            Err(e) if vanished(&e) => return Ok(()),
            Err(e) => return Err(e),
        };
        let ft = md.file_type();
        if !ft.is_dir() && md.nlink() > 1 {
            let key = (md.dev(), md.ino());
            if let Some(first) = self.links.get(&key).cloned() {
                let mut h = self.base_header(name.to_vec(), &md, b'1');
                h.link = first;
                return self.write_header(&h);
            }
            self.links.insert(key, name.to_vec());
        }
        if ft.is_dir() {
            let mut n = name.to_vec();
            if !n.ends_with(b"/") {
                n.push(b'/');
            }
            let h = self.base_header(n, &md, b'5');
            self.write_header(&h)?;
            let rd = match fs::read_dir(fs_path) {
                Ok(r) => r,
                Err(e) if vanished(&e) => return Ok(()),
                Err(e) => return Err(e),
            };
            for ent in rd {
                let ent = match ent {
                    Ok(e) => e,
                    Err(e) if vanished(&e) => continue,
                    Err(e) => return Err(e),
                };
                let fname = ent.file_name();
                let mut child = name.to_vec();
                if !child.ends_with(b"/") {
                    child.push(b'/');
                }
                child.extend_from_slice(fname.as_bytes());
                self.add_tree(&fs_path.join(&fname), &child, exclude)?;
            }
            Ok(())
        } else if ft.is_file() {
            let mut f = match File::open(fs_path) {
                Ok(f) => f,
                Err(e) if vanished(&e) => return Ok(()),
                Err(e) => return Err(e),
            };
            let mut h = self.base_header(name.to_vec(), &md, b'0');
            h.size = md.len();
            self.write_header(&h)?;
            let mut left = md.len();
            let mut buf = vec![0u8; 64 * 1024];
            while left > 0 {
                self.check_cancel()?;
                let want = left.min(buf.len() as u64) as usize;
                let got = f.read(&mut buf[..want])?;
                if got == 0 {
                    // Shrank while reading: GNU tar pads with zeros.
                    let zeros = vec![0u8; want];
                    self.emit(&zeros)?;
                    left -= want as u64;
                    continue;
                }
                self.emit(&buf[..got])?;
                left -= got as u64;
            }
            self.pad(md.len())
        } else if ft.is_symlink() {
            let target = match fs::read_link(fs_path) {
                Ok(t) => t,
                Err(e) if vanished(&e) => return Ok(()),
                Err(e) => return Err(e),
            };
            let mut h = self.base_header(name.to_vec(), &md, b'2');
            h.link = OsStr::new(&target).as_bytes().to_vec();
            self.write_header(&h)
        } else if ft.is_fifo() || ft.is_char_device() || ft.is_block_device() {
            let kind = if ft.is_fifo() {
                b'6'
            } else if ft.is_char_device() {
                b'3'
            } else {
                b'4'
            };
            let mut h = self.base_header(name.to_vec(), &md, kind);
            if !ft.is_fifo() {
                h.dev = Some(dev_split(md.rdev()));
            }
            self.write_header(&h)
        } else {
            // Socket: GNU tar prints "socket ignored" and skips it.
            Ok(())
        }
    }

    /// End-of-archive (two zero blocks), padded to the 10240-byte record.
    pub fn finish(mut self) -> io::Result<W> {
        self.emit(&[0u8; BLOCK * 2])?;
        let r = self.written % RECORD;
        if r != 0 {
            let z = vec![0u8; (RECORD - r) as usize];
            self.emit(&z)?;
        }
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_checksum_is_valid() {
        let h = Hdr {
            name: b"x".to_vec(),
            mode: 0o100600,
            uid: 0,
            gid: 0,
            size: 3,
            mtime: 1,
            kind: b'0',
            link: vec![],
            uname: b"root".to_vec(),
            gname: b"root".to_vec(),
            dev: None,
        };
        let b = header_block(&h);
        let stored = u32::from_str_radix(std::str::from_utf8(&b[148..154]).unwrap(), 8).unwrap();
        assert_eq!(stored, crate::num::checksum(&b));
        assert_eq!(&b[100..108], b"0000600\0");
    }
}
