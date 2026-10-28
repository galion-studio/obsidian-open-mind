// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! The backup's tar policy on top of the shared `galion-tar` GNU writer
//! (moved there from this file; byte output unchanged): the exclude set and
//! NSS-exact uname/gname lookups through `getpwuid_r`/`getgrgid_r`, which is
//! what GNU tar itself calls.

use std::ffi::CStr;
use std::io::Write;

pub use galion_tar::gnu::{Exclude, TarWriter};
use galion_tar::names::Names;

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

/// A GNU tar writer whose uname/gname come from the libc NSS lookups.
pub fn writer<W: Write>(out: W) -> TarWriter<W> {
    TarWriter::with_names(out, Box::new(LibcNames))
}

/// `getpwuid_r` / `getgrgid_r`, like GNU tar.
pub struct LibcNames;

impl Names for LibcNames {
    fn user(&mut self, uid: u32) -> Vec<u8> {
        lookup_user(uid)
    }
    fn group(&mut self, gid: u32) -> Vec<u8> {
        lookup_group(gid)
    }
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
    fn libc_names_agree_with_etc_files_for_root() {
        // The box and the VPS use nsswitch `files`, so both sources agree.
        let mut etc = galion_tar::names::EtcNames::default();
        assert_eq!(LibcNames.user(0), etc.user(0));
        assert_eq!(LibcNames.group(0), etc.group(0));
    }
}
