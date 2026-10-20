// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! File operations with crash-safe ordering: write a sibling temp file, fsync it,
//! rename it over the destination, fsync the directory. A reader (systemd starting
//! the unit, or the next galion-deploy) sees the old file or the new file, never a
//! half-written one.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn sibling(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn sync_dir(p: &Path) {
    if let Some(d) = p.parent() {
        let d = if d.as_os_str().is_empty() {
            Path::new(".")
        } else {
            d
        };
        if let Ok(f) = File::open(d) {
            let _ = f.sync_all();
        }
    }
}

fn copy_into(src: &Path, tmp: &Path, mode: u32) -> io::Result<()> {
    let _ = fs::remove_file(tmp);
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(tmp)?;
    let mut inp = File::open(src)?;
    io::copy(&mut inp, &mut out)?;
    out.set_permissions(fs::Permissions::from_mode(mode))?;
    out.sync_all()
}

/// `install -m MODE src dst.new && mv -f dst.new dst`, plus fsyncs.
pub fn install(src: &Path, dst: &Path, mode: u32) -> io::Result<()> {
    let tmp = sibling(dst, ".new");
    if let Err(e) = copy_into(src, &tmp, mode) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, dst)?;
    sync_dir(dst);
    Ok(())
}

/// `cp -p src dst` (mode, owner when permitted, atime/mtime), atomically.
pub fn copy_preserving(src: &Path, dst: &Path) -> io::Result<()> {
    let md = fs::metadata(src)?;
    let tmp = sibling(dst, ".tmp");
    let mode = md.permissions().mode() & 0o7777;
    let run = || -> io::Result<()> {
        copy_into(src, &tmp, mode)?;
        let _ = std::os::unix::fs::chown(&tmp, Some(md.uid()), Some(md.gid()));
        // chown may clear set-id bits; restore the exact mode afterwards.
        fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        let f = File::options().write(true).open(&tmp)?;
        let times = fs::FileTimes::new()
            .set_accessed(md.accessed()?)
            .set_modified(md.modified()?);
        f.set_times(times)?;
        f.sync_all()
    };
    if let Err(e) = run() {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, dst)?;
    sync_dir(dst);
    Ok(())
}

/// Atomically replace `dst` with `data`.
pub fn write_atomic(dst: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    let tmp = sibling(dst, ".tmp");
    let _ = fs::remove_file(&tmp);
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, dst)?;
    sync_dir(dst);
    Ok(())
}

/// Create `dst` with `data` only if it does not exist yet (write temp, hard-link it
/// into place: link(2) fails with EEXIST if someone else got there first).
pub fn create_exclusive(dst: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    let tmp = sibling(dst, &format!(".{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    let r = fs::hard_link(&tmp, dst);
    let _ = fs::remove_file(&tmp);
    r?;
    sync_dir(dst);
    Ok(())
}

pub fn is_elf(p: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    File::open(p)
        .and_then(|mut f| f.read_exact(&mut b))
        .map(|_| &b[1..4] == b"ELF")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gd-files-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn install_and_copy() {
        let d = tmpdir("ic");
        let a = d.join("a");
        fs::write(&a, b"\x7fELFold").unwrap();
        fs::set_permissions(&a, fs::Permissions::from_mode(0o750)).unwrap();
        let old = SystemTime::now() - Duration::from_secs(86_400);
        File::options()
            .write(true)
            .open(&a)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old).set_accessed(old))
            .unwrap();
        let b = d.join("b");
        copy_preserving(&a, &b).unwrap();
        let mb = fs::metadata(&b).unwrap();
        assert_eq!(mb.permissions().mode() & 0o7777, 0o750);
        assert_eq!(
            mb.modified().unwrap(),
            fs::metadata(&a).unwrap().modified().unwrap()
        );
        assert_eq!(fs::read(&b).unwrap(), b"\x7fELFold");
        let c = d.join("c");
        fs::write(&c, b"x").unwrap();
        install(&a, &c, 0o755).unwrap();
        assert_eq!(fs::read(&c).unwrap(), b"\x7fELFold");
        assert_eq!(
            fs::metadata(&c).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        assert!(!sibling(&c, ".new").exists());
        assert!(is_elf(&c));
        fs::write(&c, b"#!/bin/sh\n").unwrap();
        assert!(!is_elf(&c));
        assert!(!is_elf(&d.join("missing")));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn exclusive() {
        let d = tmpdir("ex");
        let j = d.join("j");
        create_exclusive(&j, b"one", 0o600).unwrap();
        assert!(create_exclusive(&j, b"two", 0o600).is_err());
        assert_eq!(fs::read(&j).unwrap(), b"one");
        write_atomic(&j, b"three", 0o600).unwrap();
        assert_eq!(fs::read(&j).unwrap(), b"three");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        fs::remove_dir_all(&d).unwrap();
    }
}
