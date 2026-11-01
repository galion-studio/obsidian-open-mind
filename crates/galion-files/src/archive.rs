// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Archive listing and extraction, all pure Rust: RAR 1.3..7 through `rars`
//! (Apache-2.0), ZIP through `zip` (MIT), tar and tar.gz through
//! `galion-tar` + [`crate::inflate`] (miniz_oxide). The format is chosen from the magic bytes, not
//! the name.
//!
//! Extraction safety: member names are cleaned ([`clean_member`]: no
//! absolute paths, no `..`, no backslash tricks), links and devices are
//! skipped, existing files are never overwritten, and the total output is
//! capped. The caller extracts into a fresh staging folder.

use crate::{io_err, Error, Result};
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Members listed at most (the rest is counted).
pub const MAX_MEMBERS: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rar,
    Zip,
    Tar,
    TarGz,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Rar => "rar",
            Format::Zip => "zip",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
        }
    }
}

/// Format from the first bytes.
pub fn format_of(path: &Path) -> Result<Format> {
    let mut head = [0u8; 512];
    let mut f = File::open(path).map_err(io_err)?;
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) => return Err(io_err(e)),
        }
    }
    let h = &head[..n];
    if h.starts_with(b"Rar!\x1a\x07") {
        return Ok(Format::Rar);
    }
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") {
        return Ok(Format::Zip);
    }
    if h.len() > 262 && &h[257..262] == b"ustar" {
        return Ok(Format::Tar);
    }
    if h.starts_with(b"\x1f\x8b") {
        return Ok(Format::TarGz);
    }
    Err(Error::Unsupported(
        "not a zip, rar or tar archive (7z, xz and others are listed by name only)".into(),
    ))
}

/// Folder name for an extracted archive: `photos.tar.gz` -> `photos`.
pub fn stem(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for suf in [
        ".tar.gz", ".tgz", ".zip", ".rar", ".tar", ".cbz", ".cbr", ".jar", ".apk",
    ] {
        if lower.ends_with(suf) && name.len() > suf.len() {
            return name[..name.len() - suf.len()].to_string();
        }
    }
    format!("{name}.d")
}

/// A safe relative path for a member name, or `None` to skip it.
pub fn clean_member(name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    let mut out = PathBuf::new();
    for seg in name.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return None,
            s if s.contains('\0') || s.len() > 255 => return None,
            // Windows drive letters ("C:") are dropped, not joined.
            s if s.len() == 2 && s.ends_with(':') => continue,
            s => out.push(s),
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn rar_err(e: rars::Error) -> Error {
    Error::Unsupported(format!("rar: {e}"))
}

fn zip_err(e: zip::result::ZipError) -> Error {
    Error::Unsupported(format!("zip: {e}"))
}

fn gz_tar(path: &Path) -> Result<galion_tar::read::Reader<crate::inflate::Gz<BufReader<File>>>> {
    let f = File::open(path).map_err(io_err)?;
    let gz = crate::inflate::Gz::new(BufReader::new(f))
        .map_err(|e| Error::Unsupported(format!("gzip: {e}")))?;
    Ok(galion_tar::read::Reader::new(gz))
}

/// `{format, members: [{name, size, packed, dir, encrypted}], count, total_size, truncated}`.
pub fn list(path: &Path) -> Result<Value> {
    let fmt = format_of(path)?;
    let mut members = Vec::new();
    let mut count = 0usize;
    let mut total = 0u64;
    let mut push =
        |name: String, size: u64, packed: Option<u64>, dir: bool, enc: bool, link: bool| {
            count += 1;
            total += size;
            if members.len() < MAX_MEMBERS {
                members.push(
                    json!({"name": name, "size": size, "packed": packed, "dir": dir,
                "encrypted": enc, "link": link, "safe": clean_member(&name).is_some()}),
                );
            }
        };
    let mut solid = false;
    match fmt {
        Format::Rar => {
            let f = File::open(path).map_err(io_err)?;
            let a = rars::ArchiveReader::read_reader(f).map_err(rar_err)?;
            for m in a.members() {
                solid |= m.is_solid();
                let mm = &m.meta;
                push(
                    mm.name_lossy(),
                    mm.unpacked_size,
                    Some(mm.packed_size),
                    mm.is_directory,
                    mm.is_encrypted,
                    mm.is_redirection,
                );
            }
        }
        Format::Zip => {
            let f = File::open(path).map_err(io_err)?;
            let mut z = zip::ZipArchive::new(BufReader::new(f)).map_err(zip_err)?;
            for i in 0..z.len() {
                let e = z.by_index_raw(i).map_err(zip_err)?;
                let name = e
                    .name()
                    .map(|n| n.into_owned())
                    .unwrap_or_else(|_| format!("<member {i}>"));
                push(
                    name,
                    e.size(),
                    Some(e.compressed_size()),
                    e.is_dir(),
                    e.encrypted(),
                    e.is_symlink(),
                );
            }
        }
        Format::Tar | Format::TarGz => {
            let mut each = |r: &mut dyn FnMut() -> io::Result<Option<galion_tar::read::Member>>| -> Result<()> {
                while let Some(m) = r().map_err(|e| Error::Unsupported(format!("tar: {e}")))? {
                    use galion_tar::read::MemberKind as K;
                    push(m.name_lossy(), m.size, None, m.kind == K::Dir,
                        false, matches!(m.kind, K::Symlink | K::Hardlink));
                }
                Ok(())
            };
            if fmt == Format::Tar {
                let mut r = galion_tar::read::Reader::new(BufReader::new(
                    File::open(path).map_err(io_err)?,
                ));
                each(&mut || r.next_member())?;
            } else {
                let mut r = gz_tar(path)?;
                each(&mut || r.next_member())?;
            }
        }
    }
    Ok(
        json!({"format": fmt.as_str(), "members": members, "count": count,
        "total_size": total, "truncated": count > MAX_MEMBERS, "solid": solid}),
    )
}

/// What [`extract`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub files: u64,
    pub bytes: u64,
    /// Names skipped (unsafe names, links, devices, already present).
    pub skipped: Vec<String>,
}

/// A writer that fails once the shared total passes the cap.
struct Capped<W: Write> {
    w: W,
    used: Arc<AtomicU64>,
    cap: u64,
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let now = self.used.fetch_add(buf.len() as u64, Ordering::SeqCst) + buf.len() as u64;
        if now > self.cap {
            return Err(io::Error::other("extraction cap reached"));
        }
        self.w.write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

/// Create `stage/rel` for writing (parents made, never overwriting).
fn create_in(stage: &Path, rel: &Path) -> io::Result<File> {
    let p = stage.join(rel);
    if let Some(d) = p.parent() {
        fs::create_dir_all(d)?;
    }
    fs::OpenOptions::new().write(true).create_new(true).open(p)
}

fn too_big(cap: u64) -> Error {
    Error::TooLarge(format!("archive expands past the {} MiB cap", cap >> 20))
}

/// Extract everything into `stage` (an empty folder the caller made).
pub fn extract(path: &Path, stage: &Path, cap: u64) -> Result<Stats> {
    let fmt = format_of(path)?;
    let used = Arc::new(AtomicU64::new(0));
    let mut st = Stats::default();
    match fmt {
        Format::Rar => {
            let f = File::open(path).map_err(io_err)?;
            let a = rars::ArchiveReader::read_reader(f).map_err(rar_err)?;
            if a.members().any(|m| m.meta.is_encrypted) {
                return Err(Error::Unsupported(
                    "rar: encrypted members need a password (not supported)".into(),
                ));
            }
            let skipped = std::cell::RefCell::new(Vec::new());
            let files = std::cell::Cell::new(0u64);
            let opts = rars::ArchiveReadOptions::new().with_max_total_output_bytes(cap);
            let r = a.extract_to_with_options(opts, |meta| {
                let name = meta.name_lossy();
                let sink = || Ok(Box::new(io::sink()) as Box<dyn Write>);
                let Some(rel) = clean_member(&name) else {
                    skipped.borrow_mut().push(name);
                    return sink();
                };
                if meta.is_directory {
                    fs::create_dir_all(stage.join(&rel))?;
                    return sink();
                }
                match create_in(stage, &rel) {
                    Ok(f) => {
                        files.set(files.get() + 1);
                        Ok(Box::new(Capped {
                            w: io::BufWriter::new(f),
                            used: used.clone(),
                            cap,
                        }) as Box<dyn Write>)
                    }
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        skipped.borrow_mut().push(name);
                        sink()
                    }
                    Err(e) => Err(e.into()),
                }
            });
            if let Err(e) = r {
                if used.load(Ordering::SeqCst) > cap || e.to_string().contains("limit") {
                    return Err(too_big(cap));
                }
                return Err(rar_err(e));
            }
            st.files = files.get();
            st.skipped = skipped.into_inner();
        }
        Format::Zip => {
            let f = File::open(path).map_err(io_err)?;
            let mut z = zip::ZipArchive::new(BufReader::new(f)).map_err(zip_err)?;
            for i in 0..z.len() {
                let e = z.by_index_raw(i).map_err(zip_err)?;
                let name = e.name().map(|n| n.into_owned()).unwrap_or_default();
                if e.encrypted() {
                    return Err(Error::Unsupported(
                        "zip: encrypted members are not supported".into(),
                    ));
                }
                let rel = match (e.is_symlink(), e.enclosed_name().and(clean_member(&name))) {
                    (false, Some(r)) => r,
                    _ => {
                        st.skipped.push(name);
                        continue;
                    }
                };
                if e.is_dir() {
                    fs::create_dir_all(stage.join(&rel)).map_err(io_err)?;
                    continue;
                }
                let deflated = match e.compression() {
                    zip::CompressionMethod::Stored => false,
                    m if m == zip::CompressionMethod::DEFLATE => true,
                    m => {
                        st.skipped
                            .push(format!("{name} (zip method {m} not supported)"));
                        continue;
                    }
                };
                let out = match create_in(stage, &rel) {
                    Ok(f) => f,
                    Err(x) if x.kind() == io::ErrorKind::AlreadyExists => {
                        st.skipped.push(name);
                        continue;
                    }
                    Err(x) => return Err(io_err(x)),
                };
                let mut w = Capped {
                    w: io::BufWriter::new(out),
                    used: used.clone(),
                    cap,
                };
                let (crc, size) = (e.crc32(), e.size());
                let copied = if deflated {
                    let z = crate::inflate::Inflate::new(io::BufReader::new(e));
                    io::copy(&mut crate::inflate::Checked::new(z, crc, size), &mut w)
                } else {
                    io::copy(&mut crate::inflate::Checked::new(e, crc, size), &mut w)
                };
                if let Err(x) = copied.and_then(|_| w.flush()) {
                    if used.load(Ordering::SeqCst) > cap {
                        return Err(too_big(cap));
                    }
                    return Err(Error::Unsupported(format!("zip: {x}")));
                }
                st.files += 1;
            }
        }
        Format::Tar | Format::TarGz => {
            fn run<R: Read>(
                mut r: galion_tar::read::Reader<R>,
                stage: &Path,
                used: &Arc<AtomicU64>,
                cap: u64,
                st: &mut Stats,
            ) -> Result<()> {
                use galion_tar::read::MemberKind as K;
                while let Some(m) = r
                    .next_member()
                    .map_err(|e| Error::Unsupported(format!("tar: {e}")))?
                {
                    let name = m.name_lossy();
                    let Some(rel) = clean_member(&name) else {
                        st.skipped.push(name);
                        continue;
                    };
                    match m.kind {
                        K::Dir => fs::create_dir_all(stage.join(&rel)).map_err(io_err)?,
                        K::File => {
                            let out = match create_in(stage, &rel) {
                                Ok(f) => f,
                                Err(x) if x.kind() == io::ErrorKind::AlreadyExists => {
                                    st.skipped.push(name);
                                    continue;
                                }
                                Err(x) => return Err(io_err(x)),
                            };
                            let mut w = Capped {
                                w: io::BufWriter::new(out),
                                used: used.clone(),
                                cap,
                            };
                            if let Err(x) = r.copy_data(&mut w).and_then(|_| w.flush()) {
                                if used.load(Ordering::SeqCst) > cap {
                                    return Err(too_big(cap));
                                }
                                return Err(Error::Unsupported(format!("tar: {x}")));
                            }
                            st.files += 1;
                        }
                        _ => st.skipped.push(name),
                    }
                }
                Ok(())
            }
            if fmt == Format::Tar {
                run(
                    galion_tar::read::Reader::new(BufReader::new(
                        File::open(path).map_err(io_err)?,
                    )),
                    stage,
                    &used,
                    cap,
                    &mut st,
                )?;
            } else {
                run(gz_tar(path)?, stage, &used, cap, &mut st)?;
            }
        }
    }
    st.bytes = used.load(Ordering::SeqCst);
    Ok(st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_names_are_cleaned() {
        assert_eq!(clean_member("a/b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(
            clean_member("/etc/passwd"),
            Some(PathBuf::from("etc/passwd"))
        );
        assert_eq!(clean_member("C:\\x\\y"), Some(PathBuf::from("x/y")));
        assert_eq!(clean_member("./a"), Some(PathBuf::from("a")));
        assert_eq!(clean_member("../evil"), None);
        assert_eq!(clean_member("a/../../evil"), None);
        assert_eq!(clean_member("a\\..\\evil"), None);
        assert_eq!(clean_member(""), None);
    }

    #[test]
    fn stems() {
        assert_eq!(stem("ai video Instagram.rar"), "ai video Instagram");
        assert_eq!(stem("x.tar.gz"), "x");
        assert_eq!(stem("noext"), "noext.d");
    }
}
