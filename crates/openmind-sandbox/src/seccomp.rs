// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Optional seccomp-BPF deny-list for the sandboxed command (opt-in with
//! [`crate::SandboxPolicy::seccomp`]). It works without Landlock, so it is
//! the syscall layer on hosts such as an LXC guest whose kernel has no
//! Landlock (EOPNOTSUPP).
//!
//! The filter is built before the fork (no allocation after it) and loaded
//! as the very last step before `execve`, after `PR_SET_NO_NEW_PRIVS`, so
//! the sandbox's own setup (namespaces, mounts) is not affected. It is
//! inherited by every process the command starts and cannot be removed.
//!
//! Policy (deny-list; everything else is allowed so compilers, test runners
//! and a loopback service still work):
//! - wrong architecture (and x32 syscalls on x86_64) -> kill the process;
//! - kernel/host control: ptrace, process_vm_*, mount family, pivot_root,
//!   chroot, setns, unshare, namespace flags on clone, module load, kexec,
//!   bpf, perf_event_open, keyrings, userfaultfd, io_uring, swap, reboot,
//!   clock/host name changes, handle-based opens, acct, quotactl, syslog
//!   -> EPERM;
//! - clone3 -> ENOSYS (its flags live in memory a filter cannot read; libc
//!   falls back to clone, whose flags are checked);
//! - socket(2) families other than AF_UNIX, AF_INET, AF_INET6 and
//!   AF_NETLINK -> EAFNOSUPPORT (no raw packet sockets). Network reach is
//!   still bounded by the empty network namespace, not by this filter.
//!
//! A denied call returns an errno; it is not logged by the kernel. The
//! filter cannot read pointer arguments, so it cannot check a connect(2)
//! address: per-port loopback rules need Landlock (ABI >= 4) or the private
//! network namespace (the only interface there is the sandbox's own `lo`).

const BPF_LD_W_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
const BPF_JEQ_K: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
const BPF_JGE_K: u16 = 0x35; // BPF_JMP | BPF_JGE | BPF_K
const BPF_JSET_K: u16 = 0x45; // BPF_JMP | BPF_JSET | BPF_K
const BPF_RET_K: u16 = 0x06; // BPF_RET | BPF_K

const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_ALLOW: u32 = 0x7fff_0000;

const OFF_NR: u32 = 0;
const OFF_ARCH: u32 = 4;
const OFF_ARG0_LO: u32 = 16;

#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xC000_00B7;

/// Namespace flags refused on clone(2).
const CLONE_NS_MASK: u32 = (libc::CLONE_NEWNS
    | libc::CLONE_NEWUSER
    | libc::CLONE_NEWNET
    | libc::CLONE_NEWPID
    | libc::CLONE_NEWIPC
    | libc::CLONE_NEWUTS
    | libc::CLONE_NEWCGROUP) as u32;

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn denied() -> Vec<libc::c_long> {
    let mut v = vec![
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_setns,
        libc::SYS_unshare,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_move_mount,
        libc::SYS_open_tree,
        libc::SYS_mount_setattr,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_kexec_load,
        libc::SYS_kexec_file_load,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_userfaultfd,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        libc::SYS_reboot,
        libc::SYS_settimeofday,
        libc::SYS_clock_settime,
        libc::SYS_clock_adjtime,
        libc::SYS_adjtimex,
        libc::SYS_sethostname,
        libc::SYS_setdomainname,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
        libc::SYS_acct,
        libc::SYS_quotactl,
        libc::SYS_syslog,
    ];
    #[cfg(target_arch = "x86_64")]
    v.extend([libc::SYS_iopl, libc::SYS_ioperm]);
    v
}

/// One BPF instruction with symbolic jump targets, resolved in [`program`].
#[derive(Clone, Copy)]
enum T {
    Next,
    Allow,
    Eperm,
    Enosys,
    Eafnosupport,
    Kill,
}

/// The filter program (instructions only; [`install`] loads it).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub fn program() -> Vec<libc::sock_filter> {
    let mut ins: Vec<(u16, u32, T, T)> = vec![];
    let jump = |c, k, t, f| (c, k, t, f);
    ins.push(jump(BPF_LD_W_ABS, OFF_ARCH, T::Next, T::Next));
    ins.push(jump(BPF_JEQ_K, AUDIT_ARCH, T::Next, T::Kill));
    ins.push(jump(BPF_LD_W_ABS, OFF_NR, T::Next, T::Next));
    #[cfg(target_arch = "x86_64")]
    ins.push(jump(BPF_JGE_K, 0x4000_0000, T::Kill, T::Next));
    #[cfg(not(target_arch = "x86_64"))]
    let _ = BPF_JGE_K;
    for nr in denied() {
        ins.push(jump(BPF_JEQ_K, nr as u32, T::Eperm, T::Next));
    }
    ins.push(jump(BPF_JEQ_K, libc::SYS_clone3 as u32, T::Enosys, T::Next));
    // clone(flags, ...): refuse namespace flags (arg0 on x86_64 and aarch64).
    let clone_at = ins.len();
    ins.push(jump(BPF_JEQ_K, libc::SYS_clone as u32, T::Next, T::Next));
    ins.push(jump(BPF_LD_W_ABS, OFF_ARG0_LO, T::Next, T::Next));
    ins.push(jump(BPF_JSET_K, CLONE_NS_MASK, T::Eperm, T::Allow));
    let socket_at = ins.len();
    ins.push(jump(BPF_JEQ_K, libc::SYS_socket as u32, T::Next, T::Allow));
    ins.push(jump(BPF_LD_W_ABS, OFF_ARG0_LO, T::Next, T::Next));
    for af in [
        libc::AF_UNIX,
        libc::AF_INET,
        libc::AF_INET6,
        libc::AF_NETLINK,
    ] {
        ins.push(jump(BPF_JEQ_K, af as u32, T::Allow, T::Next));
    }
    ins.push(jump(BPF_RET_K, 0, T::Eafnosupport, T::Eafnosupport));
    // Return block.
    let base = ins.len();
    let rets = [
        (T::Allow, RET_ALLOW),
        (T::Eperm, RET_ERRNO | libc::EPERM as u32),
        (T::Enosys, RET_ERRNO | libc::ENOSYS as u32),
        (T::Eafnosupport, RET_ERRNO | libc::EAFNOSUPPORT as u32),
        (T::Kill, RET_KILL_PROCESS),
    ];
    let at = |t: T| -> usize {
        base + match t {
            T::Allow => 0,
            T::Eperm => 1,
            T::Enosys => 2,
            T::Eafnosupport => 3,
            T::Kill => 4,
            T::Next => unreachable!(),
        }
    };
    let mut out = vec![];
    for (i, (code, k, t, f)) in ins.iter().enumerate() {
        if *code == BPF_RET_K {
            // the EAFNOSUPPORT tail of the socket block
            out.push(libc::sock_filter {
                code: BPF_RET_K,
                jt: 0,
                jf: 0,
                k: RET_ERRNO | libc::EAFNOSUPPORT as u32,
            });
            continue;
        }
        let rel = |x: T| -> u8 {
            match x {
                T::Next => 0,
                // clone's false branch skips its 2-instruction arg check
                _ => (at(x) - (i + 1)) as u8,
            }
        };
        let mut jf = rel(*f);
        if i == clone_at {
            jf = 2; // not clone: skip LD arg0 + JSET, land on the socket test
        }
        out.push(libc::sock_filter {
            code: *code,
            jt: rel(*t),
            jf,
            k: *k,
        });
    }
    let _ = socket_at;
    for (_, k) in rets {
        out.push(libc::sock_filter {
            code: BPF_RET_K,
            jt: 0,
            jf: 0,
            k,
        });
    }
    out
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn program() -> Vec<libc::sock_filter> {
    vec![]
}

/// Load `prog` into the calling thread. Async-signal-safe (one prctl on
/// memory prepared before the fork). Needs `PR_SET_NO_NEW_PRIVS` first.
/// An empty program (unsupported architecture) is an error.
pub fn install(prog: &[libc::sock_filter]) -> bool {
    if prog.is_empty() || prog.len() > 4096 {
        return false;
    }
    let fprog = libc::sock_fprog {
        len: prog.len() as libc::c_ushort,
        filter: prog.as_ptr() as *mut libc::sock_filter,
    };
    // SAFETY: fprog points at a live, correctly sized instruction array.
    unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER as libc::c_ulong,
            &fprog as *const libc::sock_fprog as libc::c_ulong,
            0,
            0,
        ) == 0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn program_jumps_stay_in_range() {
        let p = super::program();
        assert!(p.len() < 255, "jump offsets are u8");
        for (i, ins) in p.iter().enumerate() {
            if ins.code & 0x07 == 0x05 {
                assert!(i + 1 + (ins.jt as usize) < p.len());
                assert!(i + 1 + (ins.jf as usize) < p.len());
            }
        }
        assert_eq!(p.last().unwrap().code, super::BPF_RET_K);
    }
}
