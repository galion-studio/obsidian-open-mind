// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Payload helpers that run *inside* the sandbox during real tests. They report what the
//! payload can see and do as `key=value` lines; the real test asserts on them.

use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

fn errno_name(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(libc::EPERM) => "EPERM".into(),
        Some(libc::EACCES) => "EACCES".into(),
        Some(libc::EROFS) => "EROFS".into(),
        Some(libc::ENOENT) => "ENOENT".into(),
        Some(libc::ENETUNREACH) => "ENETUNREACH".into(),
        Some(libc::ECONNREFUSED) => "ECONNREFUSED".into(),
        Some(libc::EAGAIN) => "EAGAIN".into(),
        Some(libc::ENOMEM) => "ENOMEM".into(),
        Some(libc::EINVAL) => "EINVAL".into(),
        Some(n) => format!("errno{n}"),
        None => format!("{:?}", e.kind()),
    }
}

fn rc(r: libc::c_int) -> String {
    if r == -1 {
        format!("denied:{}", errno_name(&io::Error::last_os_error()))
    } else {
        "ALLOWED".into()
    }
}

fn cap_eff() -> String {
    #[repr(C)]
    struct Hdr {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    let mut h = Hdr {
        version: 0x2008_0522,
        pid: 0,
    };
    let mut d = [Data {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    // SAFETY: header + two data structs, as _LINUX_CAPABILITY_VERSION_3 requires.
    let r = unsafe { libc::syscall(libc::SYS_capget, &mut h as *mut Hdr, d.as_mut_ptr()) };
    if r == -1 {
        return format!("err:{}", errno_name(&io::Error::last_os_error()));
    }
    format!("{:08x}{:08x}", d[1].effective, d[0].effective)
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: valid buffer.
    if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } != 0 {
        return "err".into();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

fn interfaces() -> String {
    // Without /sys or /proc, ask the kernel directly: probe indexes 1..64.
    let mut names = Vec::new();
    for idx in 1..64u32 {
        let mut buf = [0 as libc::c_char; libc::IF_NAMESIZE];
        // SAFETY: buffer is IF_NAMESIZE long.
        let p = unsafe { libc::if_indextoname(idx, buf.as_mut_ptr()) };
        if !p.is_null() {
            // SAFETY: if_indextoname wrote a NUL-terminated name.
            let n = unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned();
            names.push(n);
        }
    }
    if names.is_empty() {
        "none".into()
    } else {
        names.join(",")
    }
}

fn proc_pids() -> String {
    match std::fs::read_dir("/proc") {
        Err(e) => format!("err:{}", errno_name(&e)),
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .count()
            .to_string(),
    }
}

fn status_line(key: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| crate::probe::status_field(&s, key))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unreadable".into())
}

fn no_new_privs() -> String {
    // SAFETY: prctl getter.
    unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) }.to_string()
}

fn try_mount() -> String {
    let _ = std::fs::create_dir_all("/tmp/m");
    let t = crate::sys::cs("tmpfs");
    let d = crate::sys::cs("/tmp/m");
    // SAFETY: valid strings.
    rc(unsafe { libc::mount(t.as_ptr(), d.as_ptr(), t.as_ptr(), 0, std::ptr::null()) })
}

fn try_write(path: &str) -> String {
    match std::fs::write(path, b"x") {
        Ok(()) => {
            let _ = std::fs::remove_file(path);
            "ALLOWED".into()
        }
        Err(e) => format!("denied:{}", errno_name(&e)),
    }
}

fn try_connect(target: &str) -> String {
    let addr: SocketAddr = match target.parse() {
        Ok(a) => a,
        Err(_) => return "badaddr".into(),
    };
    match TcpStream::connect_timeout(&addr, Duration::from_millis(1500)) {
        Ok(mut s) => {
            let _ = s.write_all(b"galion-box escape probe\n");
            "OPEN".into()
        }
        Err(e) => format!("blocked:{}", errno_name(&e)),
    }
}

/// `inside-check [--connect ADDR]... [--path P]...`
pub fn inside_check(args: &[String]) -> i32 {
    let mut out = String::new();
    // SAFETY: trivial getters.
    let (uid, gid, euid, pid, ppid) = unsafe {
        (
            libc::getuid(),
            libc::getgid(),
            libc::geteuid(),
            libc::getpid(),
            libc::getppid(),
        )
    };
    let mut push = |k: &str, v: String| out.push_str(&format!("{k}={v}\n"));
    push("uid", uid.to_string());
    push("gid", gid.to_string());
    push("euid", euid.to_string());
    push("pid", pid.to_string());
    push("ppid", ppid.to_string());
    push("hostname", hostname());
    push("cap_eff", cap_eff());
    push("no_new_privs", no_new_privs());
    push("seccomp", status_line("Seccomp"));
    push(
        "proc_state",
        std::env::var("GALION_BOX_PROC").unwrap_or_else(|_| "unset".into()),
    );
    push("proc_pids", proc_pids());
    push("interfaces", interfaces());
    push("env_count", std::env::vars().count().to_string());
    // SAFETY: setuid(0) attempt, expected to fail.
    push("setuid0", rc(unsafe { libc::setuid(0) }));
    push("mount_tmpfs", try_mount());
    let dot = crate::sys::cs(".");
    // SAFETY: valid string.
    push("chroot", rc(unsafe { libc::chroot(dot.as_ptr()) }));
    // SAFETY: kill(1, 0) only checks existence/permission; pid 1 is our init.
    push("kill_init", rc(unsafe { libc::kill(1, 0) }));
    push("write_root", try_write("/galion-box-write-test"));
    push("write_tmp", try_write("/tmp/galion-box-write-test"));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--connect", Some(t)) => push(&format!("connect:{t}"), try_connect(t)),
            ("--write", Some(p)) => push(&format!("write:{p}"), try_write(p)),
            ("--path", Some(p)) => push(
                &format!("path:{p}"),
                match std::fs::symlink_metadata(p) {
                    Ok(_) => "VISIBLE".into(),
                    Err(e) => format!("hidden:{}", errno_name(&e)),
                },
            ),
            _ => {}
        }
    }
    // Last: a nested user namespace is allowed by design (not an escape); recorded only.
    // SAFETY: plain syscall.
    push(
        "nested_userns",
        rc(unsafe { libc::unshare(libc::CLONE_NEWUSER) }),
    );
    print!("{out}");
    0
}

/// `spawn N`: try to fork N children that each sleep 1 s; print how many forks succeeded.
fn spawn(n: u32) -> i32 {
    let mut ok = 0u32;
    let mut first_err = String::from("none");
    for _ in 0..n {
        // SAFETY: the helper is single-threaded.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            std::thread::sleep(Duration::from_secs(1));
            // SAFETY: _exit in child.
            unsafe { libc::_exit(0) };
        }
        if pid > 0 {
            ok += 1;
        } else if first_err == "none" {
            first_err = errno_name(&io::Error::last_os_error());
        }
    }
    let mut st = 0;
    // SAFETY: reap all children.
    while unsafe { libc::wait(&mut st) } > 0 {}
    println!("spawn_requested={n}\nspawn_ok={ok}\nspawn_first_err={first_err}");
    0
}

/// `alloc MB`: try to map and touch MB MiB.
fn alloc(mb: usize) -> i32 {
    let len = mb * 1024 * 1024;
    // SAFETY: anonymous private mapping; checked below.
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        println!(
            "alloc_mb={mb}\nalloc=denied:{}",
            errno_name(&io::Error::last_os_error())
        );
        return 0;
    }
    // SAFETY: p..p+len is our mapping.
    unsafe {
        let s = std::slice::from_raw_parts_mut(p as *mut u8, len);
        for i in (0..len).step_by(4096) {
            s[i] = 1;
        }
        libc::munmap(p, len);
    }
    println!("alloc_mb={mb}\nalloc=ALLOWED");
    0
}

pub fn dispatch(args: &[String]) -> i32 {
    let num = |i: usize| args.get(i).and_then(|s| s.parse::<u64>().ok());
    match args.first().map(String::as_str) {
        Some("inside-check") => inside_check(&args[1..]),
        Some("spawn") => spawn(num(1).unwrap_or(10) as u32),
        Some("alloc") => alloc(num(1).unwrap_or(64) as usize),
        Some("sleep") => {
            std::thread::sleep(Duration::from_secs(num(1).unwrap_or(3600)));
            0
        }
        Some("exit") => num(1).unwrap_or(0) as i32,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_helper_returns_code() {
        assert_eq!(dispatch(&["exit".into(), "7".into()]), 7);
        assert_eq!(dispatch(&["nope".into()]), 2);
    }

    #[test]
    fn errno_names() {
        assert_eq!(
            errno_name(&io::Error::from_raw_os_error(libc::EPERM)),
            "EPERM"
        );
        assert_eq!(errno_name(&io::Error::from_raw_os_error(9999)), "errno9999");
    }

    #[test]
    fn hostname_and_caps_readable_without_sandbox() {
        assert_ne!(hostname(), "err");
        assert!(!cap_eff().is_empty());
    }
}
