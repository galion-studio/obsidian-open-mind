// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only `unsafe` in openmind-nerve: four tiny blocking Linux syscall
//! wrappers (inotify, pidfd_open, poll, read/close) so the event mode can
//! sleep in the kernel until something happens instead of polling.
//!
//! No `libc` crate: the symbols come from the C library std already links
//! (glibc or musl; bionic on Android). Every wrapper owns its fd through
//! [`std::os::fd::OwnedFd`], so close happens exactly once. On non-Linux
//! targets every call returns `Unsupported`.

#![allow(unsafe_code)]

use std::io;
#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

/// inotify event mask bits (linux/inotify.h).
pub const IN_MODIFY: u32 = 0x0000_0002;
pub const IN_ATTRIB: u32 = 0x0000_0004;
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
pub const IN_MOVED_TO: u32 = 0x0000_0080;
pub const IN_CREATE: u32 = 0x0000_0100;
pub const IN_DELETE: u32 = 0x0000_0200;
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
pub const IN_IGNORED: u32 = 0x0000_8000;

/// One decoded inotify record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InotifyRec {
    pub wd: i32,
    pub mask: u32,
    pub name: String,
}

/// Decode a buffer of `struct inotify_event` records (wd i32, mask u32,
/// cookie u32, len u32, name[len] NUL padded). Pure, so it is unit tested.
pub fn decode_inotify(buf: &[u8]) -> Vec<InotifyRec> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 16 <= buf.len() {
        let rd = |o: usize| u32::from_ne_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        let wd = rd(i) as i32;
        let mask = rd(i + 4);
        let len = rd(i + 12) as usize;
        let end = (i + 16 + len).min(buf.len());
        let raw = &buf[i + 16..end];
        let n = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        out.push(InotifyRec {
            wd,
            mask,
            name: String::from_utf8_lossy(&raw[..n]).into_owned(),
        });
        i += 16 + len;
    }
    out
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod ffi {
    use std::ffi::{c_char, c_int, c_long, c_short, c_uint, c_ulong, c_void};
    #[repr(C)]
    pub struct PollFd {
        pub fd: c_int,
        pub events: c_short,
        pub revents: c_short,
    }
    extern "C" {
        pub fn inotify_init1(flags: c_int) -> c_int;
        pub fn inotify_add_watch(fd: c_int, path: *const c_char, mask: c_uint) -> c_int;
        pub fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
        pub fn poll(fds: *mut PollFd, n: c_ulong, timeout: c_int) -> c_int;
        pub fn syscall(num: c_long, ...) -> c_long;
    }
    /// IN_CLOEXEC == O_CLOEXEC (02000000 octal) on every Linux arch we build.
    pub const IN_CLOEXEC: c_int = 0o2_000_000;
    /// pidfd_open(2): 434 in the unified syscall table (x86_64, aarch64, arm, ...).
    pub const SYS_PIDFD_OPEN: c_long = 434;
    pub const POLLIN: c_short = 1;
}

/// A blocking inotify instance.
pub struct Inotify {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fd: OwnedFd,
}

impl Inotify {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn new() -> io::Result<Self> {
        // SAFETY: plain syscall wrapper, no pointers; result checked.
        let fd = unsafe { ffi::inotify_init1(ffi::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is a fresh descriptor we own.
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn new() -> io::Result<Self> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Add a watch; returns the watch descriptor.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn add_watch(&self, path: &std::path::Path, mask: u32) -> io::Result<i32> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: c is a valid NUL-terminated string that outlives the call.
        let wd = unsafe { ffi::inotify_add_watch(self.fd.as_raw_fd(), c.as_ptr(), mask) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(wd)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn add_watch(&self, _p: &std::path::Path, _m: u32) -> io::Result<i32> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Block until at least one record is available, then return them.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn read_blocking(&self) -> io::Result<Vec<InotifyRec>> {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            // SAFETY: buf is valid for buf.len() bytes for the whole call.
            let n = unsafe { ffi::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            return Ok(decode_inotify(&buf[..n as usize]));
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn read_blocking(&self) -> io::Result<Vec<InotifyRec>> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// A pidfd: becomes readable when the process exits.
pub struct PidFd {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fd: OwnedFd,
    pub pid: u32,
}

impl PidFd {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn open(pid: u32) -> io::Result<Self> {
        if pid == 0 || pid > i32::MAX as u32 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        // SAFETY: pidfd_open(pid, 0) takes two integers; result checked.
        let fd = unsafe {
            ffi::syscall(
                ffi::SYS_PIDFD_OPEN,
                pid as std::ffi::c_long,
                0 as std::ffi::c_long,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is a fresh descriptor we own (fits c_int by contract).
        Ok(Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd as i32) },
            pid,
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn open(_pid: u32) -> io::Result<Self> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    /// Block (no timeout) until the process has exited.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub fn wait_exit(&self) -> io::Result<()> {
        loop {
            let mut p = ffi::PollFd {
                fd: self.fd.as_raw_fd(),
                events: ffi::POLLIN,
                revents: 0,
            };
            // SAFETY: p is a valid pollfd for the duration of the call.
            let r = unsafe { ffi::poll(&mut p, 1, -1) };
            if r < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if r > 0 {
                return Ok(());
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    pub fn wait_exit(&self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(wd: i32, mask: u32, name: &str, pad: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&wd.to_ne_bytes());
        v.extend_from_slice(&mask.to_ne_bytes());
        v.extend_from_slice(&0u32.to_ne_bytes());
        let len = name.len() + pad;
        v.extend_from_slice(&(len as u32).to_ne_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend(std::iter::repeat_n(0u8, pad));
        v
    }

    #[test]
    fn decode_two_records() {
        let mut b = rec(1, IN_CLOSE_WRITE, "slot-1.done", 5);
        b.extend(rec(2, IN_MOVED_TO, "", 0));
        let r = decode_inotify(&b);
        assert_eq!(r.len(), 2);
        assert_eq!(
            r[0],
            InotifyRec {
                wd: 1,
                mask: IN_CLOSE_WRITE,
                name: "slot-1.done".into()
            }
        );
        assert_eq!(r[1].name, "");
        assert!(decode_inotify(&b[..10]).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn inotify_sees_write_and_pidfd_sees_exit() {
        let d = std::env::temp_dir().join(format!("nerve-evt-sys-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let ino = Inotify::new().unwrap();
        ino.add_watch(&d, IN_CLOSE_WRITE | IN_MOVED_TO).unwrap();
        std::fs::write(d.join("x.state"), b"done\n").unwrap();
        let r = ino.read_blocking().unwrap();
        assert!(r
            .iter()
            .any(|x| x.name == "x.state" && x.mask & IN_CLOSE_WRITE != 0));
        let _ = std::fs::remove_dir_all(&d);

        let mut child = std::process::Command::new("sleep")
            .arg("0.2")
            .spawn()
            .unwrap();
        let p = PidFd::open(child.id()).unwrap();
        let t = std::time::Instant::now();
        p.wait_exit().unwrap();
        assert!(t.elapsed() >= std::time::Duration::from_millis(100));
        let _ = child.wait();
    }
}
