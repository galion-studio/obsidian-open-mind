// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! The rootless launcher. Process tree:
//!
//! ```text
//! caller (host namespaces, supervises + timeout)
//!   └─ init (pid 1): clone(NEWUSER|NEWNS|NEWPID|NEWNET|NEWUTS|NEWIPC), own id maps,
//!        │           mounts, pivot_root, reaps
//!        └─ payload: limits, no caps, no_new_privs, execve
//! ```
//! If the caller dies, PR_SET_PDEATHSIG takes init down, and the kernel kills everything
//! left in the pid namespace: nothing outlives the launcher.

use crate::policy::{id_map_line, NetMode, Policy};
use crate::sys::{self, check, cs, cstr, mount};
use std::ffi::CString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const EXIT_SETUP_FAILED: i32 = 125;
pub const EXIT_EXEC_FAILED: i32 = 127;
pub const EXIT_TIMEOUT: i32 = 124;

fn die(stage: &str, e: impl std::fmt::Display) -> ! {
    eprintln!("galion-box: {stage}: {e}");
    // SAFETY: _exit is always safe; skips atexit handlers of the forked copy.
    unsafe { libc::_exit(EXIT_SETUP_FAILED) }
}

fn fork() -> io::Result<libc::pid_t> {
    // SAFETY: the launcher is single-threaded when it forks (checked by run()).
    let pid = unsafe { libc::fork() };
    check(pid, "fork")?;
    Ok(pid)
}

fn decode(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        EXIT_SETUP_FAILED
    }
}

fn wait_blocking(pid: libc::pid_t) -> i32 {
    let mut st = 0;
    loop {
        // SAFETY: valid out pointer.
        let r = unsafe { libc::waitpid(pid, &mut st, 0) };
        if r == pid {
            return decode(st);
        }
        if r == -1 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return EXIT_SETUP_FAILED;
        }
    }
}

/// Run the payload under `p`. Returns the payload's exit code (or 124 timeout / 125 setup / 127 exec).
pub fn run(p: &Policy) -> io::Result<i32> {
    let rootfs = std::fs::canonicalize(&p.rootfs)?;
    if rootfs == Path::new("/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rootfs resolves to /",
        ));
    }
    // SAFETY: trivial getters.
    let (euid, uid, gid) = unsafe { (libc::geteuid(), libc::getuid(), libc::getgid()) };
    if euid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to run as root: this prototype is rootless only",
        ));
    }
    let started = Instant::now();
    let init = clone_into_namespaces()?;
    if init == 0 {
        init_child(p, &rootfs, uid, gid);
    }
    // Supervisor (host namespaces). Without a timeout: plain blocking wait.
    if p.timeout.is_none() {
        return Ok(wait_blocking(init));
    }
    let t = p.timeout.unwrap_or_default();
    let mut st = 0;
    loop {
        // SAFETY: valid out pointer.
        let r = unsafe { libc::waitpid(init, &mut st, libc::WNOHANG) };
        if r == init {
            return Ok(decode(st));
        }
        if r == -1 && io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return Err(io::Error::last_os_error());
        }
        if started.elapsed() >= t {
            // init is pid 1 of the sandbox's pid namespace: SIGKILL to it makes the kernel
            // kill every process left in that namespace.
            // SAFETY: init is our child.
            unsafe { libc::kill(init, libc::SIGKILL) };
            wait_blocking(init);
            return Ok(EXIT_TIMEOUT);
        }
        // 20 ms poll: cheap, no busy loop.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// fork-like clone(2) straight into new user/mount/pid/net/uts/ipc namespaces.
/// Unlike unshare(CLONE_NEWUSER) this works from a multi-threaded caller (an Android app
/// process, or qemu-user with its helper thread), because the new child is single-threaded.
fn clone_into_namespaces() -> io::Result<libc::pid_t> {
    let flags = libc::CLONE_NEWUSER
        | libc::CLONE_NEWNS
        | libc::CLONE_NEWPID
        | libc::CLONE_NEWNET
        | libc::CLONE_NEWUTS
        | libc::CLONE_NEWIPC
        | libc::SIGCHLD;
    // SAFETY: raw clone with a NULL stack behaves like fork (child continues on a copy of
    // the stack). The remaining args (tids, tls) are unused; their order differs between
    // x86_64 and aarch64, but all are 0 here.
    let pid = unsafe {
        libc::syscall(
            libc::SYS_clone,
            flags as libc::c_ulong,
            0usize,
            0usize,
            0usize,
            0usize,
        )
    };
    if pid == -1 {
        let e = io::Error::last_os_error();
        return Err(io::Error::new(e.kind(), format!("clone(namespaces): {e}")));
    }
    Ok(pid as libc::pid_t)
}

fn set_pdeathsig(expected_parent: libc::pid_t) {
    // SAFETY: prctl with integer args.
    unsafe {
        libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGKILL as libc::c_ulong,
            0,
            0,
            0,
        )
    };
    // SAFETY: trivial getter.
    if unsafe { libc::getppid() } != expected_parent {
        die("pdeathsig", "parent exited before setup");
    }
}

fn init_child(p: &Policy, rootfs: &Path, uid: u32, gid: u32) -> ! {
    // Fail closed if something (an emulator, a seccomp filter) ignored the namespace flags.
    // SAFETY: trivial getter.
    if unsafe { libc::getpid() } != 1 {
        die(
            "namespaces",
            "clone did not create a new pid namespace (emulator ignored flags?)",
        );
    }
    // pid 1 of a new pid namespace: the supervisor lives outside, so getppid() is 0.
    set_pdeathsig(0);
    let maps = sys::write_file("/proc/self/setgroups", "deny")
        .and_then(|_| sys::write_file("/proc/self/uid_map", &id_map_line(p.inside_id, uid)))
        .and_then(|_| sys::write_file("/proc/self/gid_map", &id_map_line(p.inside_id, gid)));
    if let Err(e) = maps {
        die("id maps", e);
    }
    init_main(p, rootfs);
}

fn ensure_dir(p: &Path) -> io::Result<()> {
    if !p.is_dir() {
        std::fs::create_dir_all(p)
            .map_err(|e| io::Error::new(e.kind(), format!("mkdir {}: {e}", p.display())))?;
    }
    Ok(())
}

fn ensure_file(p: &Path) -> io::Result<()> {
    if !p.exists() {
        std::fs::File::create(p)
            .map_err(|e| io::Error::new(e.kind(), format!("create {}: {e}", p.display())))?;
    }
    Ok(())
}

fn inside(rootfs: &Path, dst: &Path) -> PathBuf {
    rootfs.join(dst.strip_prefix("/").unwrap_or(dst))
}

/// Mount setup in the new mount namespace. Returns the /proc state for the payload env.
fn setup_mounts(p: &Policy, rootfs: &Path) -> io::Result<&'static str> {
    let nosuid_nodev = (libc::MS_NOSUID as libc::c_ulong) | libc::MS_NODEV as libc::c_ulong;
    mount(
        None,
        Path::new("/"),
        None,
        libc::MS_REC as libc::c_ulong | libc::MS_PRIVATE as libc::c_ulong,
        None,
    )?;
    mount(Some(rootfs), rootfs, None, libc::MS_BIND as _, None)?;

    let proc_dir = rootfs.join("proc");
    ensure_dir(&proc_dir)?;
    let proc_state = match mount(
        Some(Path::new("proc")),
        &proc_dir,
        Some("proc"),
        nosuid_nodev | libc::MS_NOEXEC as libc::c_ulong,
        None,
    ) {
        Ok(()) => "fresh",
        Err(e) if p.require_proc => return Err(e),
        Err(_) => {
            // Never fall back to the host /proc: hide it behind an empty read-only tmpfs.
            mount(
                Some(Path::new("tmpfs")),
                &proc_dir,
                Some("tmpfs"),
                nosuid_nodev | libc::MS_NOEXEC as libc::c_ulong | libc::MS_RDONLY as libc::c_ulong,
                Some("size=4k,mode=555"),
            )?;
            "hidden"
        }
    };

    let tmp = rootfs.join("tmp");
    ensure_dir(&tmp)?;
    mount(
        Some(Path::new("tmpfs")),
        &tmp,
        Some("tmpfs"),
        nosuid_nodev,
        Some("size=64m,mode=1777"),
    )?;

    let dev = rootfs.join("dev");
    ensure_dir(&dev)?;
    mount(
        Some(Path::new("tmpfs")),
        &dev,
        Some("tmpfs"),
        (libc::MS_NOSUID as libc::c_ulong) | libc::MS_NOEXEC as libc::c_ulong,
        Some("size=64k,mode=755"),
    )?;
    for node in ["null", "zero", "full", "random", "urandom"] {
        let host = PathBuf::from(format!("/dev/{node}"));
        if host.exists() {
            let t = dev.join(node);
            ensure_file(&t)?;
            mount(Some(&host), &t, None, libc::MS_BIND as _, None)?;
        }
    }

    for b in &p.binds_ro {
        let t = inside(rootfs, &b.dst);
        if b.src.is_dir() {
            ensure_dir(&t)?;
        } else {
            if let Some(par) = t.parent() {
                ensure_dir(par)?;
            }
            ensure_file(&t)?;
        }
        mount(Some(&b.src), &t, None, libc::MS_BIND as _, None).map_err(|e| {
            std::io::Error::new(e.kind(), format!("bind-ro {}: {e}", b.src.display()))
        })?;
        sys::remount_ro(&t)?;
    }

    let old = rootfs.join(".oldroot");
    ensure_dir(&old)?;
    let root_c = cstr(rootfs)?;
    // SAFETY: valid C strings; pivot_root has no libc wrapper.
    unsafe {
        check(libc::chdir(root_c.as_ptr()), "chdir rootfs")?;
        let dot = cs(".");
        let oldr = cs(".oldroot");
        check(
            libc::syscall(libc::SYS_pivot_root, dot.as_ptr(), oldr.as_ptr()) as libc::c_int,
            "pivot_root",
        )?;
        let slash = cs("/");
        check(libc::chdir(slash.as_ptr()), "chdir /")?;
        let o = cs("/.oldroot");
        check(
            libc::umount2(o.as_ptr(), libc::MNT_DETACH),
            "umount old root",
        )?;
    }
    std::fs::remove_dir("/.oldroot")?;
    if p.ro_root {
        sys::remount_ro(Path::new("/"))?;
    }
    Ok(proc_state)
}

fn init_main(p: &Policy, rootfs: &Path) -> ! {
    let proc_state = match setup_mounts(p, rootfs) {
        Ok(s) => s,
        Err(e) => die("mounts", e),
    };
    let host = cs(&p.hostname);
    // SAFETY: valid buffer + length.
    if let Err(e) = check(
        unsafe { libc::sethostname(host.as_ptr(), p.hostname.len() as _) },
        "sethostname",
    ) {
        die("uts", e);
    }
    if p.net == NetMode::Loopback {
        if let Err(e) = sys::loopback_up() {
            die("net", e);
        }
    }
    let payload = match fork() {
        Ok(pid) => pid,
        Err(e) => die("fork payload", e),
    };
    if payload == 0 {
        exec_payload(p, proc_state);
    }
    // pid 1: reap everything; report the payload's status.
    let mut code = EXIT_SETUP_FAILED;
    loop {
        let mut st = 0;
        // SAFETY: valid out pointer.
        let r = unsafe { libc::waitpid(-1, &mut st, 0) };
        if r == payload {
            code = decode(st);
        } else if r == -1 {
            match io::Error::last_os_error().raw_os_error() {
                Some(libc::EINTR) => continue,
                _ => break, // ECHILD: all reaped
            }
        }
    }
    // SAFETY: _exit in forked child.
    unsafe { libc::_exit(code) }
}

fn exec_payload(p: &Policy, proc_state: &str) -> ! {
    let r: io::Result<()> = (|| {
        // SAFETY: integer-only libc calls.
        unsafe {
            check(
                libc::setpriority(libc::PRIO_PROCESS as _, 0, p.nice),
                "setpriority",
            )?;
        }
        sys::set_rlimit(libc::RLIMIT_CORE as _, 0, "RLIMIT_CORE")?;
        if let Some(n) = p.nproc {
            sys::set_rlimit(libc::RLIMIT_NPROC as _, n, "RLIMIT_NPROC")?;
        }
        if let Some(n) = p.nofile {
            sys::set_rlimit(libc::RLIMIT_NOFILE as _, n, "RLIMIT_NOFILE")?;
        }
        if let Some(mb) = p.as_mb {
            sys::set_rlimit(libc::RLIMIT_AS as _, mb * 1024 * 1024, "RLIMIT_AS")?;
        }
        // SAFETY: prctl with integer args.
        unsafe {
            for cap in 0..64 {
                if libc::prctl(libc::PR_CAPBSET_DROP, cap as libc::c_ulong, 0, 0, 0) == -1 {
                    break; // EINVAL past the last capability
                }
            }
            libc::prctl(
                libc::PR_CAP_AMBIENT,
                libc::PR_CAP_AMBIENT_CLEAR_ALL as libc::c_ulong,
                0,
                0,
                0,
            );
            check(
                libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0),
                "no_new_privs",
            )?;
        }
        Ok(())
    })();
    if let Err(e) = r {
        die("payload limits", e);
    }
    let argv: Vec<CString> = p
        .argv
        .iter()
        .map(|a| CString::new(a.as_str()).unwrap_or_else(|_| cs("?")))
        .collect();
    let env: Vec<CString> = p
        .payload_env(proc_state)
        .iter()
        .map(|a| CString::new(a.as_str()).unwrap_or_else(|_| cs("X=?")))
        .collect();
    let mut argv_p: Vec<*const libc::c_char> = argv.iter().map(|c| c.as_ptr()).collect();
    argv_p.push(std::ptr::null());
    let mut env_p: Vec<*const libc::c_char> = env.iter().map(|c| c.as_ptr()).collect();
    env_p.push(std::ptr::null());
    // SAFETY: NULL-terminated arrays of valid C strings.
    unsafe { libc::execve(argv[0].as_ptr(), argv_p.as_ptr(), env_p.as_ptr()) };
    eprintln!(
        "galion-box: exec {:?}: {}",
        p.argv[0],
        io::Error::last_os_error()
    );
    // SAFETY: _exit in forked child.
    unsafe { libc::_exit(EXIT_EXEC_FAILED) }
}
