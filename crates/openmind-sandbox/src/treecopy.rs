// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Directory tree copy for the private workdir tmpfs (`workdir_tmpfs_bytes`).
//!
//! Runs between fork and exec (copy in) and in the PID-namespace supervisor
//! after the tool exited (copy out), so it uses raw syscalls only: no
//! allocation, no std filesystem API, fixed stack buffers. Every name is
//! resolved relative to a directory fd with `O_NOFOLLOW`, so a symlink
//! planted in either tree is copied as a symlink and never followed.
//!
//! - Regular files, directories and symlinks are copied with their mode
//!   (set-uid/set-gid bits stripped) and mtime. FIFOs, sockets and device
//!   nodes are skipped and counted.
//! - Copy out writes only what changed: a host file with the same size and
//!   mtime as the tmpfs file is left alone. Deletions inside the sandbox are
//!   not propagated. A host entry of another type (a directory where the
//!   tmpfs has a file, a symlink where it has a directory) is never replaced.
//! - A byte and an entry budget bound copy out, so sparse files on the tmpfs
//!   cannot turn into more host disk than the quota.

use std::ffi::CStr;

/// Deepest directory nesting copied (deeper trees fail with ELOOP).
pub(crate) const MAX_DEPTH: u32 = 48;
/// Copy buffer size (one buffer, shared by the whole walk).
pub(crate) const BUF: usize = 32 * 1024;
const DIRENT_BUF: usize = 2048;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stats {
    /// Bytes written to the destination.
    pub bytes: u64,
    /// Files, directories and symlinks created or rewritten.
    pub entries: u64,
    /// Copy out: entries left alone because the host already had them.
    pub unchanged: u64,
    /// Entries not copied (special files, host entry of another type).
    pub skipped: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Host workdir -> fresh tmpfs. Any error fails the run (fail closed).
    In,
    /// Tmpfs -> host workdir, only changes, within a budget.
    Out,
}

pub(crate) struct Walk<'a> {
    pub mode: Mode,
    pub stats: Stats,
    /// Bytes still allowed to be written.
    pub bytes_left: u64,
    /// Entries still allowed to be created.
    pub entries_left: u64,
    pub buf: &'a mut [u8],
}

fn errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

fn stat_at(dir: i32, name: &CStr) -> Result<libc::stat, i32> {
    // SAFETY: zeroed stat is a valid out-param; name is NUL-terminated.
    unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstatat(dir, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) != 0 {
            return Err(errno());
        }
        Ok(st)
    }
}

// `stat.st_mode` is `c_uint` (u32) on armv7 Android while `mode_t` is u16
// there; on Linux musl/glibc and aarch64 Android both are u32 and the cast is
// the identity. File type and permission bits all sit in the low 16 bits
// (S_IFMT = 0o170000), so the narrowing cannot drop a bit we use.
#[allow(clippy::unnecessary_cast)]
fn mode(st: &libc::stat) -> libc::mode_t {
    st.st_mode as libc::mode_t
}

fn kind(st: &libc::stat) -> libc::mode_t {
    mode(st) & libc::S_IFMT
}

fn perm(st: &libc::stat) -> libc::mode_t {
    // strip set-uid / set-gid; keep sticky and rwx
    mode(st) & 0o1777
}

fn times(st: &libc::stat) -> [libc::timespec; 2] {
    [
        libc::timespec {
            tv_sec: st.st_atime as _,
            tv_nsec: st.st_atime_nsec as _,
        },
        libc::timespec {
            tv_sec: st.st_mtime as _,
            tv_nsec: st.st_mtime_nsec as _,
        },
    ]
}

/// Open a directory entry as a directory without following a symlink.
fn open_dir_at(dir: i32, name: &CStr) -> Result<i32, i32> {
    // SAFETY: plain openat on a NUL-terminated name.
    let fd = unsafe {
        libc::openat(
            dir,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(errno())
    } else {
        Ok(fd)
    }
}

struct Fd(i32);
impl Drop for Fd {
    fn drop(&mut self) {
        // SAFETY: closing an fd this walk opened.
        unsafe { libc::close(self.0) };
    }
}

impl Walk<'_> {
    pub(crate) fn new(mode: Mode, bytes_left: u64, entries_left: u64, buf: &mut [u8]) -> Walk<'_> {
        Walk {
            mode,
            stats: Stats::default(),
            bytes_left,
            entries_left,
            buf,
        }
    }

    fn take_entry(&mut self) -> Result<(), i32> {
        if self.entries_left == 0 {
            return Err(libc::EDQUOT);
        }
        self.entries_left -= 1;
        self.stats.entries += 1;
        Ok(())
    }

    /// Copy everything below `src` into `dst` (both open directory fds).
    pub(crate) fn copy_dir(&mut self, src: i32, dst: i32, depth: u32) -> Result<(), i32> {
        if depth > MAX_DEPTH {
            return Err(libc::ELOOP);
        }
        let mut db = [0u8; DIRENT_BUF];
        loop {
            // SAFETY: getdents64 into a stack buffer of the length passed.
            let n = unsafe {
                libc::syscall(
                    libc::SYS_getdents64,
                    src,
                    db.as_mut_ptr(),
                    DIRENT_BUF as libc::c_uint,
                )
            };
            if n < 0 {
                return Err(errno());
            }
            if n == 0 {
                return Ok(());
            }
            let n = n as usize;
            let mut off = 0usize;
            while off + 19 < n {
                // linux_dirent64: u64 ino, i64 off, u16 reclen, u8 type, name
                let reclen = u16::from_ne_bytes([db[off + 16], db[off + 17]]) as usize;
                if reclen < 20 || off + reclen > n {
                    return Err(libc::EIO);
                }
                let rec = &db[off + 19..off + reclen];
                off += reclen;
                let Ok(name) = CStr::from_bytes_until_nul(rec) else {
                    return Err(libc::EIO);
                };
                let nb = name.to_bytes();
                if nb == b"." || nb == b".." {
                    continue;
                }
                self.entry(src, dst, name, depth)?;
            }
        }
    }

    fn entry(&mut self, src: i32, dst: i32, name: &CStr, depth: u32) -> Result<(), i32> {
        let st = stat_at(src, name)?;
        let have = match stat_at(dst, name) {
            Ok(s) => Some(s),
            Err(libc::ENOENT) => None,
            Err(e) => return Err(e),
        };
        match kind(&st) {
            libc::S_IFDIR => self.dir(src, dst, name, &st, have, depth),
            libc::S_IFREG => self.file(src, dst, name, &st, have),
            libc::S_IFLNK => self.link(src, dst, name, have),
            _ => {
                self.stats.skipped += 1;
                Ok(())
            }
        }
    }

    fn dir(
        &mut self,
        src: i32,
        dst: i32,
        name: &CStr,
        st: &libc::stat,
        have: Option<libc::stat>,
        depth: u32,
    ) -> Result<(), i32> {
        match have {
            Some(h) if kind(&h) != libc::S_IFDIR => {
                self.stats.skipped += 1;
                return Ok(());
            }
            Some(_) => {}
            None => {
                self.take_entry()?;
                // owner rwx while filling it; the real mode is set afterwards
                // SAFETY: mkdirat on a NUL-terminated name.
                if unsafe { libc::mkdirat(dst, name.as_ptr(), 0o700) } != 0 {
                    return Err(errno());
                }
            }
        }
        let s = Fd(open_dir_at(src, name)?);
        let d = Fd(open_dir_at(dst, name)?);
        let r = self.copy_dir(s.0, d.0, depth + 1);
        if have.is_none() {
            // SAFETY: fchmod/futimens on our own open directory fd.
            unsafe {
                libc::fchmod(d.0, perm(st) | 0o700);
                libc::futimens(d.0, times(st).as_ptr());
            }
        }
        r
    }

    fn file(
        &mut self,
        src: i32,
        dst: i32,
        name: &CStr,
        st: &libc::stat,
        have: Option<libc::stat>,
    ) -> Result<(), i32> {
        if let Some(h) = have {
            if kind(&h) != libc::S_IFREG {
                self.stats.skipped += 1;
                return Ok(());
            }
            if self.mode == Mode::Out
                && h.st_size == st.st_size
                && h.st_mtime == st.st_mtime
                && h.st_mtime_nsec == st.st_mtime_nsec
            {
                self.stats.unchanged += 1;
                return Ok(());
            }
        }
        let size = st.st_size.max(0) as u64;
        if size > self.bytes_left {
            return Err(libc::EDQUOT);
        }
        self.take_entry()?;
        // SAFETY: openat with NUL-terminated names; fds wrapped right away.
        let s = unsafe {
            libc::openat(
                src,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if s < 0 {
            return Err(errno());
        }
        let s = Fd(s);
        // SAFETY: as above.
        let d = unsafe {
            libc::openat(
                dst,
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if d < 0 {
            return Err(errno());
        }
        let d = Fd(d);
        loop {
            // SAFETY: read into our buffer of the length passed.
            let n = unsafe { libc::read(s.0, self.buf.as_mut_ptr().cast(), self.buf.len()) };
            if n < 0 {
                let e = errno();
                if e == libc::EINTR {
                    continue;
                }
                return Err(e);
            }
            if n == 0 {
                break;
            }
            let n = n as usize;
            if n as u64 > self.bytes_left {
                return Err(libc::EDQUOT);
            }
            let mut w = 0usize;
            while w < n {
                // SAFETY: writing bytes we just read.
                let m = unsafe { libc::write(d.0, self.buf[w..n].as_ptr().cast(), n - w) };
                if m < 0 {
                    let e = errno();
                    if e == libc::EINTR {
                        continue;
                    }
                    return Err(e);
                }
                w += m as usize;
            }
            self.bytes_left -= n as u64;
            self.stats.bytes += n as u64;
        }
        // SAFETY: fchmod/futimens on our own open file fd.
        unsafe {
            libc::fchmod(d.0, perm(st));
            if libc::futimens(d.0, times(st).as_ptr()) != 0 {
                return Err(errno());
            }
        }
        Ok(())
    }

    fn link(
        &mut self,
        src: i32,
        dst: i32,
        name: &CStr,
        have: Option<libc::stat>,
    ) -> Result<(), i32> {
        let mut t = [0u8; 4097];
        // SAFETY: readlinkat into a stack buffer, one byte kept for the NUL.
        let n = unsafe { libc::readlinkat(src, name.as_ptr(), t.as_mut_ptr().cast(), t.len() - 1) };
        if n < 0 {
            return Err(errno());
        }
        let n = n as usize;
        t[n] = 0;
        if let Some(h) = have {
            if kind(&h) != libc::S_IFLNK {
                self.stats.skipped += 1;
                return Ok(());
            }
            let mut o = [0u8; 4097];
            // SAFETY: as above.
            let m =
                unsafe { libc::readlinkat(dst, name.as_ptr(), o.as_mut_ptr().cast(), o.len() - 1) };
            if m >= 0 && m as usize == n && o[..n] == t[..n] {
                self.stats.unchanged += 1;
                return Ok(());
            }
            // SAFETY: removes the symlink entry itself (unlinkat never follows).
            if unsafe { libc::unlinkat(dst, name.as_ptr(), 0) } != 0 {
                return Err(errno());
            }
        }
        self.take_entry()?;
        // SAFETY: both strings are NUL-terminated.
        if unsafe { libc::symlinkat(t.as_ptr().cast(), dst, name.as_ptr()) } != 0 {
            return Err(errno());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let p = std::env::temp_dir().join(format!(
                "om-treecopy-{}-{tag}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(p.join("src")).unwrap();
            std::fs::create_dir_all(p.join("dst")).unwrap();
            Tmp(p)
        }
        fn src(&self) -> PathBuf {
            self.0.join("src")
        }
        fn dst(&self) -> PathBuf {
            self.0.join("dst")
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn open(p: &Path) -> Fd {
        let c = CString::new(p.as_os_str().as_bytes()).unwrap();
        // SAFETY: test helper, valid path.
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };
        assert!(fd >= 0, "open {}", p.display());
        Fd(fd)
    }

    fn copy(
        mode: Mode,
        from: &Path,
        to: &Path,
        bytes: u64,
        entries: u64,
    ) -> (Result<(), i32>, Stats) {
        let mut buf = vec![0u8; BUF];
        let (s, d) = (open(from), open(to));
        let mut w = Walk::new(mode, bytes, entries, &mut buf);
        let r = w.copy_dir(s.0, d.0, 0);
        (r, w.stats)
    }

    fn sample(src: &Path) {
        std::fs::write(src.join("a.txt"), b"hello\n").unwrap();
        std::fs::create_dir_all(src.join("sub/deeper")).unwrap();
        std::fs::write(src.join("sub/deeper/b.bin"), vec![7u8; 100_000]).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", src.join("sub/link")).unwrap();
        std::fs::write(src.join("exe"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(src.join("exe"), std::fs::Permissions::from_mode(0o4755)).unwrap();
    }

    #[test]
    fn copy_in_reproduces_tree_modes_mtimes_links() {
        let t = Tmp::new("in");
        sample(&t.src());
        let (r, st) = copy(Mode::In, &t.src(), &t.dst(), u64::MAX, u64::MAX);
        assert_eq!(r, Ok(()));
        assert_eq!(st.bytes, 6 + 100_000 + 10);
        assert_eq!(st.entries, 6); // a.txt sub deeper b.bin link exe
        assert_eq!(std::fs::read(t.dst().join("a.txt")).unwrap(), b"hello\n");
        assert_eq!(
            std::fs::read(t.dst().join("sub/deeper/b.bin"))
                .unwrap()
                .len(),
            100_000
        );
        assert_eq!(
            std::fs::read_link(t.dst().join("sub/link")).unwrap(),
            Path::new("/etc/hostname")
        );
        let m = std::fs::metadata(t.dst().join("exe")).unwrap();
        assert_eq!(m.mode() & 0o7777, 0o755, "set-uid stripped, rwx kept");
        let (a, b) = (
            std::fs::metadata(t.src().join("sub/deeper/b.bin")).unwrap(),
            std::fs::metadata(t.dst().join("sub/deeper/b.bin")).unwrap(),
        );
        assert_eq!((a.mtime(), a.mtime_nsec()), (b.mtime(), b.mtime_nsec()));
    }

    #[test]
    fn copy_out_writes_only_changes_and_never_follows_host_links() {
        let t = Tmp::new("out");
        sample(&t.src());
        let (r, _) = copy(Mode::In, &t.src(), &t.dst(), u64::MAX, u64::MAX);
        assert_eq!(r, Ok(()));
        // "dst" plays the tmpfs now: the tool changes one file, adds one.
        std::fs::write(t.dst().join("a.txt"), b"changed\n").unwrap();
        std::fs::write(t.dst().join("sub/new.txt"), b"new\n").unwrap();
        // The host has a symlink where the tmpfs has a directory: never followed.
        let outside = t.0.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(t.dst().join("evil")).unwrap();
        std::fs::write(t.dst().join("evil/x"), b"x").unwrap();
        std::os::unix::fs::symlink(&outside, t.src().join("evil")).unwrap();
        let (r, st) = copy(Mode::Out, &t.dst(), &t.src(), u64::MAX, u64::MAX);
        assert_eq!(r, Ok(()));
        assert_eq!(std::fs::read(t.src().join("a.txt")).unwrap(), b"changed\n");
        assert_eq!(
            std::fs::read(t.src().join("sub/new.txt")).unwrap(),
            b"new\n"
        );
        assert_eq!(st.bytes, 8 + 4);
        assert_eq!(st.entries, 2);
        assert!(st.unchanged >= 3, "{st:?}"); // b.bin, link, exe
        assert_eq!(st.skipped, 1, "evil/ vs host symlink skipped");
        assert!(!outside.join("x").exists(), "followed a host symlink");
    }

    #[test]
    fn copy_out_budget_stops_with_edquot() {
        let t = Tmp::new("budget");
        std::fs::write(t.src().join("big"), vec![1u8; 300_000]).unwrap();
        let (r, st) = copy(Mode::Out, &t.src(), &t.dst(), 200_000, u64::MAX);
        assert_eq!(r, Err(libc::EDQUOT));
        assert_eq!(st.bytes, 0, "refused before writing");
        // sparse file: 4 KiB of data, 1 GiB apparent size, still refused
        let f = std::fs::File::create(t.src().join("sparse")).unwrap();
        f.set_len(1 << 30).unwrap();
        std::fs::remove_file(t.src().join("big")).unwrap();
        let (r, _) = copy(Mode::Out, &t.src(), &t.dst(), 64 << 20, u64::MAX);
        assert_eq!(r, Err(libc::EDQUOT));
        let (r, _) = copy(Mode::Out, &t.src(), &t.dst(), u64::MAX, 0);
        assert_eq!(r, Err(libc::EDQUOT), "entry budget");
    }

    #[test]
    fn too_deep_is_eloop_and_specials_are_skipped() {
        let t = Tmp::new("deep");
        let mut p = t.src();
        for _ in 0..=MAX_DEPTH + 1 {
            p = p.join("d");
        }
        std::fs::create_dir_all(&p).unwrap();
        let (r, _) = copy(Mode::In, &t.src(), &t.dst(), u64::MAX, u64::MAX);
        assert_eq!(r, Err(libc::ELOOP));
        let t = Tmp::new("fifo");
        let c = CString::new(t.src().join("fifo").as_os_str().as_bytes()).unwrap();
        // SAFETY: test helper.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (r, st) = copy(Mode::In, &t.src(), &t.dst(), u64::MAX, u64::MAX);
        assert_eq!(r, Ok(()));
        assert_eq!(st.skipped, 1);
        assert!(!t.dst().join("fifo").exists());
    }

    #[test]
    fn many_entries_cross_several_getdents_batches() {
        let t = Tmp::new("many");
        for i in 0..500 {
            std::fs::write(
                t.src().join(format!("file-with-a-longish-name-{i:04}")),
                [i as u8],
            )
            .unwrap();
        }
        let (r, st) = copy(Mode::In, &t.src(), &t.dst(), u64::MAX, u64::MAX);
        assert_eq!(r, Ok(()));
        assert_eq!(st.entries, 500);
        assert_eq!(std::fs::read_dir(t.dst()).unwrap().count(), 500);
    }
}
