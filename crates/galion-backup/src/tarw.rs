// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Minimal GNU-format tar writer with the `tar -c` semantics the bash backup
//! relied on (GNU tar 1.35 defaults, no -h): symlinks stored as symlinks,
//! directories walked in readdir order (`--sort=none`), hard links detected,
//! fifos/devices stored, sockets skipped, uname/gname resolved like tar
//! (getpwuid/getgrgid), long names via `././@LongLink`, archive padded to the
//! 10240-byte record. Files that vanish mid-walk are skipped and files that
//! shrink are zero-padded (GNU tar: warning + exit 1, which the script
//! tolerated); every other error is fatal (GNU tar exit 2).

use std::collections::HashMap;
use std::ffi::{CStr, OsStr};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

const BLOCK: usize = 512;
const RECORD: u64 = 10240;

/// Exclusion predicate on the member name (no leading `/`, no trailing `/`).
pub type Exclude = dyn Fn(&[u8]) -> bool;

/// GNU tar `--exclude=PAT` defaults for exclusion are no-anchored,
/// wildcards-match-slash and leading-dir, so a pattern excludes a member when
/// any path component matches it (and an excluded directory prunes its
/// subtree). This is the backup's exclude set:
/// `--exclude='*.db' --exclude='*.db-wal' --exclude='*.db-shm' --exclude='.backup'`.
pub fn backup_exclude(name: &[u8]) -> bool {
    name.split(|&b| b == b'/').any(|c| {
        c.ends_with(b".db") || c.ends_with(b".db-wal") || c.ends_with(b".db-shm") || c == b".backup"
    })
}

pub struct TarWriter<W: Write> {
    out: W,
    written: u64,
    links: HashMap<(u64, u64), Vec<u8>>,
    users: HashMap<u32, Vec<u8>>,
    groups: HashMap<u32, Vec<u8>>,
    /// Polled between blocks; returning true aborts with `Interrupted`.
    pub cancel: fn() -> bool,
}

fn never() -> bool {
    false
}

fn octal_or_256(field: &mut [u8], v: i64) {
    let n = field.len();
    // Octal with a trailing NUL when it fits (GNU style: zero padded).
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
        // GNU base-256: big-endian two's complement, top bit of byte 0 set.
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

fn put(field: &mut [u8], s: &[u8]) {
    let k = s.len().min(field.len());
    field[..k].copy_from_slice(&s[..k]);
}

struct Hdr {
    name: Vec<u8>,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    mtime: i64,
    kind: u8,
    link: Vec<u8>,
    uname: Vec<u8>,
    gname: Vec<u8>,
    dev: Option<(u32, u32)>,
}

fn header_block(h: &Hdr) -> [u8; BLOCK] {
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

fn lookup_user(uid: u32) -> Vec<u8> {
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut res: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers refer to live, correctly sized buffers.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut res) };
    if rc == 0 && !res.is_null() && !pw.pw_name.is_null() {
        // SAFETY: getpwuid_r succeeded, pw_name is a NUL-terminated string in buf.
        unsafe { CStr::from_ptr(pw.pw_name) }.to_bytes().to_vec()
    } else {
        Vec::new()
    }
}

fn lookup_group(gid: u32) -> Vec<u8> {
    let mut buf = vec![0 as libc::c_char; 16384];
    let mut gr: libc::group = unsafe { std::mem::zeroed() };
    let mut res: *mut libc::group = std::ptr::null_mut();
    // SAFETY: as above.
    let rc = unsafe { libc::getgrgid_r(gid, &mut gr, buf.as_mut_ptr(), buf.len(), &mut res) };
    if rc == 0 && !res.is_null() && !gr.gr_name.is_null() {
        // SAFETY: getgrgid_r succeeded.
        unsafe { CStr::from_ptr(gr.gr_name) }.to_bytes().to_vec()
    } else {
        Vec::new()
    }
}

fn vanished(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound
}

impl<W: Write> TarWriter<W> {
    pub fn new(out: W) -> Self {
        TarWriter {
            out,
            written: 0,
            links: HashMap::new(),
            users: HashMap::new(),
            groups: HashMap::new(),
            cancel: never,
        }
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
        let uname = self
            .users
            .entry(uid)
            .or_insert_with(|| lookup_user(uid))
            .clone();
        let gname = self
            .groups
            .entry(gid)
            .or_insert_with(|| lookup_group(gid))
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
                let r = md.rdev();
                h.dev = Some((libc::major(r) as u32, libc::minor(r) as u32));
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
    fn exclude_matches_gnu_semantics() {
        for n in [
            "var/lib/hermes-rs/rs-state.db",
            "var/lib/hermes-rs/rs-state.db-wal",
            "var/lib/hermes-rs/rs-state.db-shm",
            "var/lib/memory-tencentdb/.backup",
            "var/lib/memory-tencentdb/.backup/x",
            "a/dir.db/inner.txt",
            "a/.db",
        ] {
            assert!(backup_exclude(n.as_bytes()), "{n}");
        }
        for n in [
            "var/lib/memory-tencentdb",
            "var/lib/memory-tencentdb/x.dbx",
            "var/lib/memory-tencentdb/x.db.bak",
            "var/lib/memory-tencentdb/backup",
            "var/lib/memory-tencentdb/.backup2",
            "etc/openmind-memory.env",
        ] {
            assert!(!backup_exclude(n.as_bytes()), "{n}");
        }
    }

    #[test]
    fn octal_and_base256() {
        let mut f = [0u8; 12];
        octal_or_256(&mut f, 0o1234);
        assert_eq!(&f, b"00000001234\0");
        octal_or_256(&mut f, 1 << 34); // 16 GiB > 8^11-1
        assert_eq!(f[0], 0x80);
        assert_eq!(u64::from_be_bytes(f[4..].try_into().unwrap()), 1 << 34);
        let mut m = [0u8; 8];
        octal_or_256(&mut m, 0o644);
        assert_eq!(&m, b"0000644\0");
    }

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
        let mut c = b;
        c[148..156].copy_from_slice(b"        ");
        assert_eq!(stored, c.iter().map(|&x| x as u32).sum::<u32>());
        assert_eq!(&b[100..108], b"0000600\0");
    }
}
