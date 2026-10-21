// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Minimal POSIX ustar writer and reader (regular files, directories,
//! symlinks), std only. Output is plain `tar -xpf` compatible, with relative
//! member names, so a single path can be restored with
//! `tar -xpf backup.tar -C / opt/galion/rust/bin/galion-nerve`.

use std::io::{self, Read, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    /// Relative name, no leading '/', directories without a trailing '/'.
    pub name: String,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime: i64,
    pub link: String,
    pub data: Vec<u8>,
}

fn octal(field: &mut [u8], v: u64) -> io::Result<()> {
    let s = format!("{:0w$o}", v, w = field.len() - 1);
    if s.len() > field.len() - 1 {
        return Err(io::Error::other("number too large for a ustar field"));
    }
    field[..s.len()].copy_from_slice(s.as_bytes());
    field[s.len()] = 0;
    Ok(())
}

/// Split a long name into (prefix, name) at a '/', per ustar.
fn split_name(name: &str) -> io::Result<(&str, &str)> {
    if name.len() <= 100 {
        return Ok(("", name));
    }
    for (i, c) in name.char_indices() {
        if c == '/' && i <= 155 && name.len() - i - 1 <= 100 && i > 0 {
            return Ok((&name[..i], &name[i + 1..]));
        }
    }
    Err(io::Error::other(format!("path too long for ustar: {name}")))
}

pub fn header(e: &Entry) -> io::Result<[u8; 512]> {
    let mut h = [0u8; 512];
    let full = if e.kind == Kind::Dir {
        format!("{}/", e.name)
    } else {
        e.name.clone()
    };
    let (prefix, name) = split_name(&full)?;
    h[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut h[100..108], u64::from(e.mode & 0o7777))?;
    octal(&mut h[108..116], u64::from(e.uid))?;
    octal(&mut h[116..124], u64::from(e.gid))?;
    let size = if e.kind == Kind::File {
        e.data.len() as u64
    } else {
        0
    };
    octal(&mut h[124..136], size)?;
    octal(&mut h[136..148], e.mtime.max(0) as u64)?;
    h[156] = match e.kind {
        Kind::File => b'0',
        Kind::Dir => b'5',
        Kind::Symlink => b'2',
    };
    if e.link.len() > 100 {
        return Err(io::Error::other("symlink target too long for ustar"));
    }
    h[157..157 + e.link.len()].copy_from_slice(e.link.as_bytes());
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

pub fn write_entry<W: Write>(w: &mut W, e: &Entry) -> io::Result<()> {
    w.write_all(&header(e)?)?;
    if e.kind == Kind::File {
        w.write_all(&e.data)?;
        let pad = (512 - e.data.len() % 512) % 512;
        w.write_all(&vec![0u8; pad])?;
    }
    Ok(())
}

pub fn finish<W: Write>(w: &mut W) -> io::Result<()> {
    w.write_all(&[0u8; 1024])
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|x| *x == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn parse_octal(b: &[u8]) -> io::Result<u64> {
    let s = cstr(b);
    let s = s.trim_matches(|c: char| c == ' ' || c == '\0');
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| io::Error::other(format!("bad octal field {s:?}")))
}

/// Read every entry of a ustar stream (checksums verified).
pub fn read_all<R: Read>(r: &mut R) -> io::Result<Vec<Entry>> {
    let mut out = vec![];
    loop {
        let mut h = [0u8; 512];
        r.read_exact(&mut h)?;
        if h.iter().all(|b| *b == 0) {
            return Ok(out);
        }
        let want = parse_octal(&h[148..156])?;
        let mut hh = h;
        hh[148..156].copy_from_slice(b"        ");
        let got: u64 = hh.iter().map(|b| u64::from(*b)).sum();
        if want != got {
            return Err(io::Error::other("ustar header checksum mismatch"));
        }
        let name = cstr(&h[..100]);
        let prefix = cstr(&h[345..500]);
        let mut full = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let kind = match h[156] {
            b'0' | 0 => Kind::File,
            b'5' => Kind::Dir,
            b'2' => Kind::Symlink,
            t => return Err(io::Error::other(format!("unsupported ustar type {t}"))),
        };
        if kind == Kind::Dir {
            while full.ends_with('/') {
                full.pop();
            }
        }
        let size = parse_octal(&h[124..136])? as usize;
        let mut data = vec![0u8; size];
        r.read_exact(&mut data)?;
        let pad = (512 - size % 512) % 512;
        let mut skip = vec![0u8; pad];
        r.read_exact(&mut skip)?;
        out.push(Entry {
            kind,
            name: full,
            mode: parse_octal(&h[100..108])? as u32,
            uid: parse_octal(&h[108..116])? as u32,
            gid: parse_octal(&h[116..124])? as u32,
            mtime: parse_octal(&h[136..148])? as i64,
            link: cstr(&h[157..257]),
            data,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(kind: Kind, name: &str, data: &[u8]) -> Entry {
        Entry {
            kind,
            name: name.into(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 1_760_000_000,
            link: if kind == Kind::Symlink {
                "../x".into()
            } else {
                String::new()
            },
            data: data.to_vec(),
        }
    }

    #[test]
    fn roundtrip_including_long_names() {
        let long = format!("var/lib/{}/state.json", "d".repeat(120));
        let es = vec![
            e(Kind::Dir, "opt/galion/tuner", b""),
            e(Kind::File, "opt/galion/tuner/bin/tuner", &[7u8; 1300]),
            e(Kind::File, "empty", b""),
            e(Kind::Symlink, "link", b""),
            e(Kind::File, &long, b"{}"),
        ];
        let mut buf = vec![];
        for x in &es {
            write_entry(&mut buf, x).unwrap();
        }
        finish(&mut buf).unwrap();
        assert_eq!(buf.len() % 512, 0);
        let back = read_all(&mut buf.as_slice()).unwrap();
        assert_eq!(back, es);
    }

    #[test]
    fn corrupt_header_is_detected() {
        let mut buf = vec![];
        write_entry(&mut buf, &e(Kind::File, "a", b"hello")).unwrap();
        finish(&mut buf).unwrap();
        buf[3] ^= 1;
        assert!(read_all(&mut buf.as_slice()).is_err());
    }
}
