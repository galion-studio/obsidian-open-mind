// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Thin, checked wrappers over the few libc calls the launcher needs.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub fn cstr(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"))
}

pub fn cs(s: &str) -> CString {
    CString::new(s).expect("static string without NUL")
}

pub fn check(rc: libc::c_int, what: &str) -> io::Result<()> {
    if rc == -1 {
        let e = io::Error::last_os_error();
        Err(io::Error::new(e.kind(), format!("{what}: {e}")))
    } else {
        Ok(())
    }
}

/// mount(2). `src`/`fstype`/`data` may be None.
pub fn mount(
    src: Option<&Path>,
    target: &Path,
    fstype: Option<&str>,
    flags: libc::c_ulong,
    data: Option<&str>,
) -> io::Result<()> {
    let src_c = src.map(cstr).transpose()?;
    let tgt = cstr(target)?;
    let fs_c = fstype.map(cs);
    let data_c = data.map(cs);
    // SAFETY: all pointers are valid NUL-terminated strings or null for the call's duration.
    let rc = unsafe {
        libc::mount(
            src_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            tgt.as_ptr(),
            fs_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            flags as _,
            data_c
                .as_ref()
                .map_or(std::ptr::null(), |c| c.as_ptr() as *const libc::c_void),
        )
    };
    check(rc, &format!("mount {}", target.display()))
}

/// Flags that the kernel locks on a mount inherited into a user namespace; a bind remount
/// must repeat them or it fails with EPERM.
/// Linux ST_RELATIME (statfs.h); not exported by libc for musl.
const ST_RELATIME: libc::c_ulong = 0x1000;

pub fn locked_flags(target: &Path) -> io::Result<libc::c_ulong> {
    let c = cstr(target)?;
    // SAFETY: zeroed statvfs is a valid out-parameter.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c is a valid path, st a valid out pointer.
    check(unsafe { libc::statvfs(c.as_ptr(), &mut st) }, "statvfs")?;
    let f = st.f_flag as libc::c_ulong;
    let mut out: libc::c_ulong = 0;
    for (st_bit, ms_bit) in [
        (libc::ST_NOSUID as libc::c_ulong, libc::MS_NOSUID),
        (libc::ST_NODEV as libc::c_ulong, libc::MS_NODEV),
        (libc::ST_NOEXEC as libc::c_ulong, libc::MS_NOEXEC),
        (libc::ST_NOATIME as libc::c_ulong, libc::MS_NOATIME),
        (libc::ST_NODIRATIME as libc::c_ulong, libc::MS_NODIRATIME),
        (ST_RELATIME, libc::MS_RELATIME),
    ] {
        if f & st_bit != 0 {
            out |= ms_bit as libc::c_ulong;
        }
    }
    Ok(out)
}

/// Make an existing mount read-only. Prefers mount_setattr(2) (Linux 5.12+): on the VPS (LXC)
/// the classic MS_REMOUNT|MS_BIND|MS_RDONLY is refused with EACCES (2026-10-21 06:45 CEST)
/// while mount_setattr works. Falls back to the remount on older kernels (ENOSYS).
pub fn remount_ro(target: &Path) -> io::Result<()> {
    #[repr(C)]
    struct MountAttr {
        attr_set: u64,
        attr_clr: u64,
        propagation: u64,
        userns_fd: u64,
    }
    const MOUNT_ATTR_RDONLY: u64 = 0x1;
    let c = cstr(target)?;
    let attr = MountAttr {
        attr_set: MOUNT_ATTR_RDONLY,
        attr_clr: 0,
        propagation: 0,
        userns_fd: 0,
    };
    // SAFETY: valid path and attr struct of the size passed.
    let r = unsafe {
        libc::syscall(
            libc::SYS_mount_setattr,
            libc::AT_FDCWD,
            c.as_ptr(),
            0 as libc::c_uint,
            &attr as *const MountAttr,
            std::mem::size_of::<MountAttr>(),
        )
    };
    if r == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() != Some(libc::ENOSYS) {
        return Err(io::Error::new(
            e.kind(),
            format!("mount_setattr ro {}: {e}", target.display()),
        ));
    }
    let flags = libc::MS_REMOUNT as libc::c_ulong
        | libc::MS_BIND as libc::c_ulong
        | libc::MS_RDONLY as libc::c_ulong
        | locked_flags(target)?;
    mount(None, target, None, flags, None)
        .map_err(|e| io::Error::new(e.kind(), format!("remount ro (flags {flags:#x}): {e}")))
}

pub fn write_file(path: &str, data: &str) -> io::Result<()> {
    std::fs::write(path, data).map_err(|e| io::Error::new(e.kind(), format!("write {path}: {e}")))
}

/// Bring `lo` up inside the (new, empty) network namespace.
pub fn loopback_up() -> io::Result<()> {
    #[repr(C)]
    struct IfReqFlags {
        name: [u8; 16],
        flags: libc::c_short,
        _pad: [u8; 22],
    }
    // SAFETY: plain socket call.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    check(fd, "socket(lo)")?;
    let mut req = IfReqFlags {
        name: [0; 16],
        flags: 0,
        _pad: [0; 22],
    };
    req.name[..2].copy_from_slice(b"lo");
    // SAFETY: req is a valid ifreq-sized buffer for SIOCGIFFLAGS/SIOCSIFFLAGS.
    let r = unsafe {
        let r = libc::ioctl(fd, libc::SIOCGIFFLAGS as _, &mut req);
        if r == -1 {
            r
        } else {
            req.flags |= (libc::IFF_UP | libc::IFF_RUNNING) as libc::c_short;
            libc::ioctl(fd, libc::SIOCSIFFLAGS as _, &req)
        }
    };
    let e = check(r, "ioctl(lo up)");
    // SAFETY: fd is ours.
    unsafe { libc::close(fd) };
    e
}

pub fn set_rlimit(res: libc::c_int, v: u64, what: &str) -> io::Result<()> {
    let l = libc::rlimit {
        rlim_cur: v as _,
        rlim_max: v as _,
    };
    // SAFETY: valid rlimit struct.
    check(unsafe { libc::setrlimit(res as _, &l) }, what)
}
