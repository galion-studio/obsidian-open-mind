// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Deterministic POSIX ustar writer and a strict reader, std only.
//!
//! Release archives must be byte-reproducible so SHA256SUMS can be rebuilt
//! and compared: every entry gets uid/gid 0, owner "root", one fixed mtime
//! (SOURCE_DATE_EPOCH from the release commit) and entries are written in the
//! order given (the packager sorts them). Paths are short (< 100 bytes),
//! regular files and directories only: no links, devices or long names.

/// One archive member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path inside the archive; directories end with `/`.
    pub path: String,
    pub mode: u32,
    pub data: Vec<u8>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.path.ends_with('/')
    }
}

fn octal(field: &mut [u8], v: u64) -> Result<(), String> {
    let w = field.len() - 1;
    let s = format!("{v:0w$o}");
    if s.len() > w {
        return Err(format!("tar: value {v} does not fit {w} octal digits"));
    }
    field[..w].copy_from_slice(s.as_bytes());
    field[w] = 0;
    Ok(())
}

fn header(e: &Entry, mtime: u64) -> Result<[u8; 512], String> {
    let name = e.path.as_bytes();
    if name.is_empty() || name.len() >= 100 {
        return Err(format!(
            "tar: path length {} not in 1..99: {}",
            name.len(),
            e.path
        ));
    }
    if e.path.starts_with('/') || e.path.split('/').any(|c| c == "..") {
        return Err(format!("tar: unsafe path {}", e.path));
    }
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name);
    octal(&mut h[100..108], u64::from(e.mode & 0o7777))?;
    octal(&mut h[108..116], 0)?;
    octal(&mut h[116..124], 0)?;
    let size = if e.is_dir() { 0 } else { e.data.len() as u64 };
    octal(&mut h[124..136], size)?;
    octal(&mut h[136..148], mtime)?;
    h[156] = if e.is_dir() { b'5' } else { b'0' };
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[265..269].copy_from_slice(b"root");
    h[297..301].copy_from_slice(b"root");
    // Checksum: sum of all bytes with the checksum field read as spaces.
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

/// Serialise `entries` (in the given order) as a ustar archive.
pub fn write(entries: &[Entry], mtime: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for e in entries {
        out.extend_from_slice(&header(e, mtime)?);
        if !e.is_dir() {
            out.extend_from_slice(&e.data);
            let pad = (512 - e.data.len() % 512) % 512;
            out.extend(std::iter::repeat_n(0u8, pad));
        }
    }
    // End of archive: two zero blocks, then pad to a 10240-byte record like tar.
    out.extend(std::iter::repeat_n(0u8, 1024));
    let rec = 10240;
    let pad = (rec - out.len() % rec) % rec;
    out.extend(std::iter::repeat_n(0u8, pad));
    Ok(out)
}

fn parse_octal(f: &[u8]) -> Result<u64, String> {
    let s: String = f
        .iter()
        .take_while(|&&b| b != 0 && b != b' ')
        .map(|&b| b as char)
        .collect();
    let s = s.trim_start();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| format!("tar: bad octal field {s:?}"))
}

/// Parse a ustar archive. Checks every header checksum; refuses links,
/// absolute paths and `..` so extraction can never escape its directory.
pub fn read(b: &[u8]) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut p = 0usize;
    loop {
        let h = b
            .get(p..p + 512)
            .ok_or("tar: truncated (no end-of-archive)")?;
        if h.iter().all(|&x| x == 0) {
            return Ok(out);
        }
        let want = parse_octal(&h[148..156])?;
        let mut sum: u64 = 0;
        for (i, &x) in h.iter().enumerate() {
            sum += if (148..156).contains(&i) {
                32
            } else {
                u64::from(x)
            };
        }
        if sum != want {
            return Err(format!("tar: header checksum mismatch at offset {p}"));
        }
        let name_end = h[..100].iter().position(|&x| x == 0).unwrap_or(100);
        let mut path = String::from_utf8(h[..name_end].to_vec())
            .map_err(|_| "tar: non-UTF-8 path".to_string())?;
        let prefix_end = h[345..500].iter().position(|&x| x == 0).unwrap_or(155);
        if prefix_end > 0 {
            let pre = String::from_utf8(h[345..345 + prefix_end].to_vec())
                .map_err(|_| "tar: non-UTF-8 prefix".to_string())?;
            path = format!("{pre}/{path}");
        }
        if path.starts_with('/') || path.split('/').any(|c| c == "..") {
            return Err(format!("tar: unsafe path {path}"));
        }
        let mode = parse_octal(&h[100..108])? as u32;
        let size = usize::try_from(parse_octal(&h[124..136])?).map_err(|_| "tar: size")?;
        let kind = h[156];
        p += 512;
        match kind {
            b'5' => {
                if !path.ends_with('/') {
                    path.push('/');
                }
                out.push(Entry {
                    path,
                    mode,
                    data: Vec::new(),
                });
            }
            b'0' | 0 => {
                let data = b
                    .get(p..p.checked_add(size).ok_or("tar: size overflow")?)
                    .ok_or("tar: truncated member")?
                    .to_vec();
                p += size.div_ceil(512) * 512;
                out.push(Entry { path, mode, data });
            }
            k => {
                return Err(format!(
                    "tar: unsupported member type {:?} ({path})",
                    k as char
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Entry> {
        vec![
            Entry {
                path: "pkg/".into(),
                mode: 0o755,
                data: vec![],
            },
            Entry {
                path: "pkg/bin".into(),
                mode: 0o755,
                data: (0..1500u32).map(|i| i as u8).collect(),
            },
            Entry {
                path: "pkg/README.md".into(),
                mode: 0o644,
                data: b"hi\n".to_vec(),
            },
            Entry {
                path: "pkg/empty".into(),
                mode: 0o644,
                data: vec![],
            },
        ]
    }

    #[test]
    fn roundtrip() {
        let a = write(&sample(), 1_700_000_000).unwrap();
        assert_eq!(a.len() % 10240, 0);
        assert_eq!(read(&a).unwrap(), sample());
    }

    #[test]
    fn deterministic() {
        assert_eq!(write(&sample(), 42).unwrap(), write(&sample(), 42).unwrap());
        assert_ne!(write(&sample(), 42).unwrap(), write(&sample(), 43).unwrap());
    }

    #[test]
    fn checksum_and_paths_enforced() {
        let mut a = write(&sample(), 1).unwrap();
        a[600] ^= 0xff; // inside the 2nd header (offset 512..1024)
        assert!(read(&a).unwrap_err().contains("checksum"));
        let bad = |p: &str| {
            write(
                &[Entry {
                    path: p.into(),
                    mode: 0o644,
                    data: vec![],
                }],
                0,
            )
        };
        assert!(bad("/etc/passwd").is_err());
        assert!(bad("a/../../x").is_err());
        assert!(bad(&"x".repeat(100)).is_err());
        assert!(read(&[0u8; 100]).is_err());
    }
}
