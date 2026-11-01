// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Hand what the file tools make to the shared folder's user.
//!
//! Hermes runs as root with `UMask=0077`, so a plain `create_new` gives
//! `root:root 0600` and a new folder `root:root 0700`: the sandboxed terminal
//! (uid 65534) can't read them. With an owner set ([`crate::Root::with_owner`])
//! every new or moved entry gets that `uid:gid`, files `0644` (`0755` if it
//! was executable) and folders `0755`, set explicitly so the umask doesn't matter. The
//! same owner the agent tools use (`HERMES_RS_TOOLS_RUN_AS`, default
//! 65534:65534 under a root gateway).
//!
//! Safety: symlinks are never followed. A path is opened with `O_NOFOLLOW`,
//! the real path of the open handle (`/proc/self/fd/N`) must still be inside
//! the root, and chown/chmod go through the handle (`fchown`/`fchmod`), so a
//! link or a swapped folder can't point the change at something outside.
//! Links themselves only get `lchown`. Only entries still owned by the
//! gateway's own uid are changed (never someone else's files).

use std::fs::{self, File, Permissions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{fchown, lchown, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// Mode for new files (plus any exec bits the file already had).
pub const FILE_MODE: u32 = 0o644;
/// Mode for new folders.
pub const DIR_MODE: u32 = 0o755;

/// `0644`, or `0755` when `old` had any exec bit (a script stays runnable).
/// Never setuid/setgid/sticky, never group/other write.
pub fn file_mode(old: u32) -> u32 {
    if old & 0o111 != 0 {
        0o755
    } else {
        FILE_MODE
    }
}

/// Who the files are handed to, and who "we" are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    pub uid: u32,
    pub gid: u32,
    /// The process's effective uid: only entries it owns are re-owned.
    pub me: u32,
}

impl Owner {
    /// `me` is read from `/proc/self` (owned by the effective uid).
    pub fn new(uid: u32, gid: u32) -> Owner {
        let me = fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
        Owner { uid, gid, me }
    }

    /// chown + chmod through an open handle we made (race-free). `mode` is
    /// the wanted permission bits.
    pub fn fd(&self, f: &File, mode: u32) -> io::Result<()> {
        fchown(f, Some(self.uid), Some(self.gid))?;
        f.set_permissions(Permissions::from_mode(mode))
    }

    /// A new file handle: owner + `0644` (+ the exec bits it has).
    pub fn new_file(&self, f: &File) -> io::Result<()> {
        let m = f.metadata()?;
        self.fd(f, file_mode(m.mode()))
    }

    /// One entry (not its children). Returns true when it was changed.
    pub fn path(&self, base: &Path, p: &Path) -> io::Result<bool> {
        let lm = fs::symlink_metadata(p)?;
        if lm.uid() != self.me {
            return Ok(false);
        }
        if lm.file_type().is_symlink() {
            // The link itself; lchown never follows it. No chmod on links.
            lchown(p, Some(self.uid), Some(self.gid))?;
            return Ok(true);
        }
        if !lm.is_file() && !lm.is_dir() {
            return Ok(false);
        }
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY)
            .open(p)?;
        let real = fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))?;
        if !(real == base || real.starts_with(base)) {
            return Err(io::Error::other("entry left the root"));
        }
        let m = f.metadata()?;
        if m.dev() != lm.dev() || m.ino() != lm.ino() || m.uid() != self.me {
            return Ok(false);
        }
        let mode = if m.is_dir() {
            DIR_MODE
        } else if m.is_file() {
            file_mode(m.mode())
        } else {
            return Ok(false);
        };
        self.fd(&f, mode)?;
        Ok(true)
    }

    /// `p` and everything under it (links are re-owned, never followed or
    /// entered). Returns (changed, failed).
    pub fn tree(&self, base: &Path, p: &Path) -> (u64, u64) {
        let mut n = (0, 0);
        self.walk(base, p, &mut n);
        n
    }

    fn walk(&self, base: &Path, p: &Path, n: &mut (u64, u64)) {
        match self.path(base, p) {
            Ok(true) => n.0 += 1,
            Ok(false) => {}
            Err(_) => n.1 += 1,
        }
        let Ok(lm) = fs::symlink_metadata(p) else {
            return;
        };
        if !lm.is_dir() {
            return;
        }
        let Ok(rd) = fs::read_dir(p) else {
            n.1 += 1;
            return;
        };
        for e in rd.flatten() {
            self.walk(base, &e.path(), n);
        }
    }

    /// `p` and every folder between it and `base` (not `base` itself).
    pub fn up(&self, base: &Path, p: &Path) -> u64 {
        let mut failed = 0;
        let mut cur = Some(p);
        while let Some(c) = cur {
            if c == base || !c.starts_with(base) {
                break;
            }
            if self.path(base, c).is_err() {
                failed += 1;
            }
            cur = c.parent();
        }
        failed
    }
}
