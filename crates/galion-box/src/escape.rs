// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Escape probes that run *inside* the sandbox (wave-4 slot 164). Each family tries the
//! classic ways out of a namespace sandbox and reports `key=value` lines; the real test
//! (`tests/real_escape.rs`, VPS only) asserts every way out stays shut.
//!
//! Families: ptrace, mount (incl. pivot_root), userns (nesting), bpf (incl. perf),
//! iouring, keyctl, sockets. `escape all` runs every family; `escape <family>` runs one.
//!
//! Nothing here is dangerous to the host by construction: the probes run as the sandbox
//! payload (uid 1000 inside, no capabilities, no_new_privs), every fd is closed, and any
//! state they create (a key, a ring) dies with the sandbox.

use crate::inside::errno_name;
use std::ffi::CString;
use std::io;

// UAPI constants libc does not always export for every target (musl / Android).
const AF_VSOCK: libc::c_int = 40;
const AF_BLUETOOTH: libc::c_int = 31;
const AF_XDP: libc::c_int = 44;
const NETLINK_ROUTE: libc::c_int = 0;
const KEYCTL_GET_KEYRING_ID: libc::c_long = 0;
const KEYCTL_JOIN_SESSION_KEYRING: libc::c_long = 1;
const KEYCTL_READ: libc::c_long = 11;
const KEY_SPEC_SESSION_KEYRING: libc::c_long = -3;
const KEY_SPEC_USER_KEYRING: libc::c_long = -4;
const BPF_MAP_CREATE: libc::c_long = 0;
const BPF_PROG_LOAD: libc::c_long = 5;
const BPF_MAP_TYPE_ARRAY: u32 = 2;
const BPF_PROG_TYPE_SOCKET_FILTER: u32 = 1;
const PERF_TYPE_SOFTWARE: u32 = 1;
const PERF_COUNT_SW_TASK_CLOCK: u64 = 1;
/// perf_event_attr bits: disabled | exclude_kernel | exclude_hv.
const PERF_FLAGS_USER_ONLY: u64 = 1 | (1 << 5) | (1 << 6);
/// A pid that cannot belong to this sandbox's pid namespace (init is 1, payload is 2).
const HOST_LOOKALIKE_PID: libc::pid_t = 9_001;
#[cfg(target_os = "android")]
const PTRACE_SEIZE: libc::c_int = 0x4206;
#[cfg(not(target_os = "android"))]
use libc::PTRACE_SEIZE;

fn denied() -> String {
    format!("denied:{}", errno_name(&io::Error::last_os_error()))
}

fn rc(r: libc::c_long) -> String {
    if r < 0 {
        denied()
    } else {
        "ALLOWED".into()
    }
}

fn rc_i(r: libc::c_int) -> String {
    rc(r as libc::c_long)
}

fn close_fd(fd: libc::c_int) {
    if fd >= 0 {
        // SAFETY: fd is ours from a just-succeeded open/socket/syscall.
        unsafe { libc::close(fd) };
    }
}

/// `escape [all|ptrace|mount|userns|bpf|iouring|keyctl|sockets] [--host-pid N] [--abstract NAME]`
pub fn dispatch(args: &[String]) -> i32 {
    let mut family = "all";
    let mut host_pid: Option<libc::pid_t> = None;
    let mut abstract_name = "galion-box-escape-164".to_string();
    let mut it = args.iter();
    if let Some(first) = it.next() {
        match first.as_str() {
            "all" | "ptrace" | "mount" | "userns" | "bpf" | "iouring" | "keyctl" | "sockets" => {
                family = first.as_str();
            }
            "--host-pid" | "--abstract" => {
                // No family token; re-parse from the start.
                it = args.iter();
            }
            _ => {
                eprintln!("galion-box: unknown escape family {first:?}");
                return 2;
            }
        }
    }
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--host-pid", Some(v)) => host_pid = v.parse().ok(),
            ("--abstract", Some(v)) => abstract_name = v.clone(),
            _ => {}
        }
    }

    let mut out = String::new();
    let mut push = |k: &str, v: String| out.push_str(&format!("{k}={v}\n"));
    push("family", family.to_string());

    let run_all = family == "all";
    if run_all || family == "ptrace" {
        run_ptrace(&mut push, host_pid);
    }
    if run_all || family == "mount" {
        run_mount(&mut push);
    }
    if run_all || family == "bpf" {
        run_bpf(&mut push);
    }
    if run_all || family == "iouring" {
        run_iouring(&mut push);
    }
    if run_all || family == "keyctl" {
        run_keyctl(&mut push);
    }
    if run_all || family == "sockets" {
        run_sockets(&mut push, &abstract_name);
    }
    if run_all || family == "userns" {
        run_userns(&mut push);
    }
    print!("{out}");
    0
}

fn run_ptrace(push: &mut dyn FnMut(&str, String), host_pid: Option<libc::pid_t>) {
    // Try a pid that cannot exist in this pid namespace (escape would be reaching the host).
    let target = host_pid.unwrap_or(HOST_LOOKALIKE_PID);
    // SAFETY: PTRACE_SEIZE / PTRACE_ATTACH on a foreign pid; read-only attach attempt.
    let seize = unsafe { libc::ptrace(PTRACE_SEIZE, target, 0, 0) };
    push("ptrace.seize_hostlike", rc(seize as libc::c_long));
    let attach = unsafe { libc::ptrace(libc::PTRACE_ATTACH, target, 0, 0) };
    push("ptrace.attach_hostlike", rc(attach as libc::c_long));
    // Attach to our own init (pid 1). Same-uid ptrace inside the sandbox is not a host escape,
    // but is recorded: with CapEff=0 + yama it often still fails; if it succeeds it is MEASURED.
    let seize_init = unsafe { libc::ptrace(PTRACE_SEIZE, 1, 0, 0) } as libc::c_long;
    if seize_init == 0 {
        // SAFETY: detach so we do not leave init stopped.
        unsafe { libc::ptrace(libc::PTRACE_DETACH, 1, 0, 0) };
        push("ptrace.seize_init", "ALLOWED".into());
    } else {
        push("ptrace.seize_init", denied());
    }
    // SAFETY: kill(0) probes reachability of a host-like pid.
    let kill_h = unsafe { libc::kill(target, 0) };
    push("ptrace.kill_hostlike", rc_i(kill_h));
}

fn run_mount(push: &mut dyn FnMut(&str, String)) {
    let _ = std::fs::create_dir_all("/tmp/esc-mnt");
    let tmp = CString::new("tmpfs").unwrap();
    let tgt = CString::new("/tmp/esc-mnt").unwrap();
    // SAFETY: mount of a private tmpfs; expected EPERM without CAP_SYS_ADMIN.
    let r = unsafe {
        libc::mount(
            tmp.as_ptr(),
            tgt.as_ptr(),
            tmp.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    push("mount.tmpfs", rc_i(r));

    let proc = CString::new("proc").unwrap();
    let r = unsafe {
        libc::mount(
            proc.as_ptr(),
            tgt.as_ptr(),
            proc.as_ptr(),
            0,
            std::ptr::null(),
        )
    };
    push("mount.proc", rc_i(r));

    // Bind-mount / onto a tmp dir would expose the host if pivot_root had failed.
    let slash = CString::new("/").unwrap();
    let r = unsafe {
        libc::mount(
            slash.as_ptr(),
            tgt.as_ptr(),
            std::ptr::null(),
            libc::MS_BIND,
            std::ptr::null(),
        )
    };
    push("mount.bind_root", rc_i(r));

    // Remount / read-write (escape from --ro-root).
    let r = unsafe {
        libc::mount(
            std::ptr::null(),
            slash.as_ptr(),
            std::ptr::null(),
            libc::MS_REMOUNT,
            std::ptr::null(),
        )
    };
    push("mount.remount_root_rw", rc_i(r));

    // pivot_root back onto a fresh tmpfs would undo the sandbox root.
    let new_root = CString::new("/tmp/esc-mnt").unwrap();
    let put_old = CString::new("/tmp/esc-mnt").unwrap();
    // SAFETY: pivot_root(2); both args valid C strings. Expected EPERM/EINVAL.
    let r = unsafe { libc::syscall(libc::SYS_pivot_root, new_root.as_ptr(), put_old.as_ptr()) };
    push("mount.pivot_root", rc(r));

    // Open the old host root if pivot_root left a leak.
    push(
        "mount.oldroot_path",
        match std::fs::symlink_metadata("/.oldroot") {
            Ok(_) => "VISIBLE".into(),
            Err(e) => format!("hidden:{}", errno_name(&e)),
        },
    );
}

fn run_userns(push: &mut dyn FnMut(&str, String)) {
    // Nested user namespace is allowed by the kernel for an unprivileged process that already
    // lives in a userns. Escape would be writing a uid_map that reaches a host uid we do not own
    // (especially 0), or setns into a host namespace via a leaked /proc.
    // SAFETY: unshare of NEWUSER only; process stays single-threaded.
    let r = unsafe { libc::unshare(libc::CLONE_NEWUSER) };
    push("userns.unshare", rc_i(r));
    if r == 0 {
        // Try to claim host root in the map: must fail (we are not CAP_SETUID in the parent).
        let map_root = std::fs::write("/proc/self/uid_map", "0 0 1\n");
        push(
            "userns.map_host_root",
            match map_root {
                Ok(()) => "ALLOWED".into(),
                Err(e) => format!("denied:{}", errno_name(&e)),
            },
        );
        // Mapping our own inside-id is the benign case; whether it works is recorded only.
        let _ = std::fs::write("/proc/self/setgroups", "deny");
        // SAFETY: trivial getter.
        let self_uid = unsafe { libc::getuid() };
        let line = format!("0 {self_uid} 1\n");
        let map_self = std::fs::write("/proc/self/uid_map", &line);
        push(
            "userns.map_self_as_root_inside",
            match map_self {
                Ok(()) => "ALLOWED".into(),
                Err(e) => format!("denied:{}", errno_name(&e)),
            },
        );
    }
    // Host userns via /proc/1/ns/user — /proc is hidden or is our own pid-1, never the host's.
    push(
        "userns.host_proc1_ns",
        match std::fs::symlink_metadata("/proc/1/ns/user") {
            Ok(_) => {
                // Opening and setns: if this is our sandbox init, setns is a no-op re-entry.
                match std::fs::File::open("/proc/1/ns/user") {
                    Ok(f) => {
                        use std::os::unix::io::AsRawFd;
                        let fd = f.as_raw_fd();
                        // SAFETY: setns into a user ns fd we just opened.
                        let r = unsafe { libc::setns(fd, libc::CLONE_NEWUSER) };
                        rc_i(r)
                    }
                    Err(e) => format!("denied:{}", errno_name(&e)),
                }
            }
            Err(e) => format!("hidden:{}", errno_name(&e)),
        },
    );
}

fn run_bpf(push: &mut dyn FnMut(&str, String)) {
    // Minimal bpf_attr for BPF_MAP_CREATE / BPF_PROG_LOAD (first 32 bytes used).
    #[repr(C)]
    struct BpfAttrMap {
        map_type: u32,
        key_size: u32,
        value_size: u32,
        max_entries: u32,
        map_flags: u32,
        _pad: [u32; 3],
    }
    let mut map = BpfAttrMap {
        map_type: BPF_MAP_TYPE_ARRAY,
        key_size: 4,
        value_size: 8,
        max_entries: 1,
        map_flags: 0,
        _pad: [0; 3],
    };
    // SAFETY: bpf(BPF_MAP_CREATE); attr size is the struct we pass.
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_MAP_CREATE,
            &mut map as *mut BpfAttrMap,
            std::mem::size_of::<BpfAttrMap>(),
        )
    };
    if r >= 0 {
        close_fd(r as libc::c_int);
        push("bpf.map_create", "ALLOWED".into());
    } else {
        push("bpf.map_create", denied());
    }

    // BPF_PROG_LOAD with an empty/invalid program: must not succeed as a usable prog.
    #[repr(C)]
    struct BpfAttrProg {
        prog_type: u32,
        insn_cnt: u32,
        insns: u64,
        license: u64,
        log_level: u32,
        log_size: u32,
        log_buf: u64,
        kern_version: u32,
        prog_flags: u32,
    }
    let license = CString::new("GPL").unwrap();
    // One invalid insn is enough to force a load failure even if the call were permitted.
    let insn: [u64; 1] = [0];
    let mut prog = BpfAttrProg {
        prog_type: BPF_PROG_TYPE_SOCKET_FILTER,
        insn_cnt: 1,
        insns: insn.as_ptr() as u64,
        license: license.as_ptr() as u64,
        log_level: 0,
        log_size: 0,
        log_buf: 0,
        kern_version: 0,
        prog_flags: 0,
    };
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            BPF_PROG_LOAD,
            &mut prog as *mut BpfAttrProg,
            std::mem::size_of::<BpfAttrProg>(),
        )
    };
    if r >= 0 {
        close_fd(r as libc::c_int);
        push("bpf.prog_load", "ALLOWED".into());
    } else {
        push("bpf.prog_load", denied());
    }

    // perf_event_open on self, user-only software event.
    #[repr(C)]
    struct PerfEventAttr {
        type_: u32,
        size: u32,
        config: u64,
        sample_period: u64,
        sample_type: u64,
        read_format: u64,
        bits: u64,
        _rest: [u64; 11],
    }
    let mut attr = PerfEventAttr {
        type_: PERF_TYPE_SOFTWARE,
        size: std::mem::size_of::<PerfEventAttr>() as u32,
        config: PERF_COUNT_SW_TASK_CLOCK,
        sample_period: 0,
        sample_type: 0,
        read_format: 0,
        bits: PERF_FLAGS_USER_ONLY,
        _rest: [0; 11],
    };
    // SAFETY: perf_event_open on pid 0 (self), cpu -1.
    let r = unsafe {
        libc::syscall(
            libc::SYS_perf_event_open,
            &mut attr as *mut PerfEventAttr,
            0 as libc::c_int,
            -1 as libc::c_int,
            -1 as libc::c_int,
            0 as libc::c_ulong,
        )
    };
    if r >= 0 {
        close_fd(r as libc::c_int);
        push("bpf.perf_event_open", "ALLOWED".into());
    } else {
        push("bpf.perf_event_open", denied());
    }
}

fn run_iouring(push: &mut dyn FnMut(&str, String)) {
    #[repr(C)]
    struct IoUringParams {
        sq_entries: u32,
        cq_entries: u32,
        flags: u32,
        sq_thread_cpu: u32,
        sq_thread_idle: u32,
        features: u32,
        wq_fd: u32,
        resv: [u32; 3],
        // sq/cq offsets follow; we do not read them.
        _off: [u8; 120],
    }
    let mut params: IoUringParams = unsafe { std::mem::zeroed() };
    // SAFETY: io_uring_setup with 2 entries; params is a valid out struct.
    let r = unsafe { libc::syscall(libc::SYS_io_uring_setup, 2u32, &mut params as *mut _) };
    if r >= 0 {
        let fd = r as libc::c_int;
        push("iouring.setup", "ALLOWED".into());
        // Escape would be opening a host-only path through the ring. After pivot_root those
        // paths are absent, so a successful open would mean the rootfs leaked the host.
        push(
            "iouring.host_etc_passwd",
            match std::fs::File::open("/etc/passwd") {
                Ok(_) => "VISIBLE".into(),
                Err(e) => format!("hidden:{}", errno_name(&e)),
            },
        );
        push(
            "iouring.host_opt_galion",
            match std::fs::symlink_metadata("/opt/galion") {
                Ok(_) => "VISIBLE".into(),
                Err(e) => format!("hidden:{}", errno_name(&e)),
            },
        );
        close_fd(fd);
    } else {
        push("iouring.setup", denied());
        push("iouring.host_etc_passwd", "skipped".into());
        push("iouring.host_opt_galion", "skipped".into());
    }
}

fn run_keyctl(push: &mut dyn FnMut(&str, String)) {
    // Join / read the session and user keyrings. A new user namespace gets its own keyrings;
    // reaching the host's keys would be an escape.
    // SAFETY: keyctl GET_KEYRING_ID; create=0 so we do not allocate if absent.
    let sess = unsafe {
        libc::syscall(
            libc::SYS_keyctl,
            KEYCTL_GET_KEYRING_ID,
            KEY_SPEC_SESSION_KEYRING,
            0 as libc::c_long,
        )
    };
    push("keyctl.session_id", rc(sess));

    let user = unsafe {
        libc::syscall(
            libc::SYS_keyctl,
            KEYCTL_GET_KEYRING_ID,
            KEY_SPEC_USER_KEYRING,
            0 as libc::c_long,
        )
    };
    push("keyctl.user_id", rc(user));

    // Join a fresh named session keyring (dies with the process).
    let name = CString::new("galion-box-escape").unwrap();
    let join =
        unsafe { libc::syscall(libc::SYS_keyctl, KEYCTL_JOIN_SESSION_KEYRING, name.as_ptr()) };
    push("keyctl.join_session", rc(join));

    // Try to READ key id 1 (typically the host's user/.request_key keys start higher; id 1
    // should not be a readable host secret from inside). Success with data would be ESCAPED.
    let mut buf = [0u8; 64];
    let read = unsafe {
        libc::syscall(
            libc::SYS_keyctl,
            KEYCTL_READ,
            1 as libc::c_long,
            buf.as_mut_ptr(),
            buf.len() as libc::c_long,
        )
    };
    if read >= 0 {
        // Do not print key material; only the fact that id=1 was readable.
        push("keyctl.read_id1", format!("ALLOWED:bytes={read}"));
    } else {
        push("keyctl.read_id1", denied());
    }
}

fn run_sockets(push: &mut dyn FnMut(&str, String), abstract_name: &str) {
    // AF_PACKET needs CAP_NET_RAW — must fail with CapEff=0.
    // SAFETY: socket(2) of a privileged family.
    let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, 0) };
    push(
        "sockets.af_packet",
        if fd < 0 { denied() } else { "ALLOWED".into() },
    );
    close_fd(fd);

    // AF_NETLINK inside an empty netns talks only to that ns's kernel view — not a host escape.
    // Record whether the socket opens; the real test treats it as MEASURED, not ESCAPED.
    let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW, NETLINK_ROUTE) };
    push(
        "sockets.af_netlink",
        if fd < 0 { denied() } else { "ALLOWED".into() },
    );
    close_fd(fd);

    for (name, family) in [
        ("af_vsock", AF_VSOCK),
        ("af_bluetooth", AF_BLUETOOTH),
        ("af_xdp", AF_XDP),
    ] {
        let fd = unsafe { libc::socket(family, libc::SOCK_DGRAM, 0) };
        push(
            &format!("sockets.{name}"),
            if fd < 0 { denied() } else { "ALLOWED".into() },
        );
        close_fd(fd);
    }

    // Abstract AF_UNIX is namespaced with NEWNET. Connecting to a host abstract socket must fail.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    // Abstract: sun_path[0] == 0, then the name bytes.
    let bytes = abstract_name.as_bytes();
    if bytes.len() + 1 < addr.sun_path.len() {
        addr.sun_path[0] = 0;
        for (i, b) in bytes.iter().enumerate() {
            addr.sun_path[i + 1] = *b as _;
        }
        let path_len = 1 + bytes.len();
        let len = (std::mem::size_of::<libc::sa_family_t>() + path_len) as libc::socklen_t;
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            push("sockets.abstract_unix", denied());
        } else {
            // SAFETY: connect to the abstract name; len covers family + path bytes.
            let r = unsafe { libc::connect(fd, &addr as *const _ as *const libc::sockaddr, len) };
            if r == 0 {
                push("sockets.abstract_unix", "OPEN".into());
            } else {
                push(
                    "sockets.abstract_unix",
                    format!("blocked:{}", errno_name(&io::Error::last_os_error())),
                );
            }
            close_fd(fd);
        }
    } else {
        push("sockets.abstract_unix", "denied:EINVAL".into());
    }

    // TCP to a well-known public address (octets, never a literal in committed docs).
    let public = std::net::SocketAddr::from((std::net::Ipv4Addr::new(1, 1, 1, 1), 443));
    match std::net::TcpStream::connect_timeout(&public, std::time::Duration::from_millis(800)) {
        Ok(_) => push("sockets.tcp_internet", "OPEN".into()),
        Err(e) => push(
            "sockets.tcp_internet",
            format!("blocked:{}", errno_name(&e)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_rejects_unknown_family() {
        assert_eq!(dispatch(&["nope".into()]), 2);
    }

    #[test]
    fn dispatch_ptrace_runs_without_sandbox() {
        // Outside a sandbox the hostlike pid probe simply reports denied/ALLOWED; must not panic.
        assert_eq!(
            dispatch(&["ptrace".into(), "--host-pid".into(), "1".into()]),
            0
        );
    }
}
