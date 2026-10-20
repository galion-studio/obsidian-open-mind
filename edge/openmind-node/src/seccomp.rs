// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Seccomp-BPF deny-list for the agent sandbox, written with raw libc (no
//! extra crate). Default action is ALLOW; a short list of syscalls that let a
//! process escape, inspect other processes, or change the kernel/host returns
//! EPERM. A foreign syscall ABI (e.g. x32 or 32-bit on x86_64) kills the
//! process, so the list cannot be bypassed by switching architecture.
//!
//! The program is built in the parent; after fork only `install()` runs, which
//! is two `prctl` calls and allocates nothing.

/// One classic-BPF instruction (layout of `struct sock_filter`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockFilter {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

/// `struct sock_fprog`.
#[repr(C)]
struct SockFprog {
    len: u16,
    filter: *const SockFilter,
}

// BPF opcodes.
const LD_W_ABS: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const JGE_K: u16 = 0x35;
const RET_K: u16 = 0x06;

// seccomp return actions.
const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_ALLOW: u32 = 0x7fff_0000;

// Offsets into `struct seccomp_data`.
const OFF_NR: u32 = 0;
const OFF_ARCH: u32 = 4;

const SECCOMP_MODE_FILTER: libc::c_ulong = 2;

#[cfg(target_arch = "x86_64")]
pub const AUDIT_ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
pub const AUDIT_ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64

// ---- Denied syscall numbers per architecture ----
// Goal: block escape / host change / process inspection. Everyday shell
// tools (read, write, open, mmap, clone for threads, socket inside a netns
// that is already empty, etc.) stay allowed. Numbers from the Linux UAPI.

#[cfg(target_arch = "x86_64")]
pub const DENIED: &[u32] = &[
    101, // ptrace
    165, // mount
    166, // umount2
    155, // pivot_root
    169, // reboot
    272, // unshare (namespaces already set up by the parent)
    167, // swapon
    168, // swapoff
    170, // sethostname
    171, // setdomainname
    172, // iopl
    173, // ioperm
    175, // init_module
    176, // delete_module
    246, // kexec_load
    320, // kexec_file_load
    250, // keyctl
    248, // add_key
    249, // request_key
    321, // bpf
    323, // userfaultfd
    298, // perf_event_open
    313, // finit_module
    179, // quotactl
    163, // acct
    310, // process_vm_readv
    311, // process_vm_writev
];

#[cfg(target_arch = "aarch64")]
pub const DENIED: &[u32] = &[
    117, // ptrace
    40,  // mount
    39,  // umount2
    41,  // pivot_root
    142, // reboot
    97,  // unshare
    224, // swapon
    225, // swapoff
    161, // sethostname
    162, // setdomainname
    105, // init_module
    106, // delete_module
    104, // kexec_load
    294, // kexec_file_load
    219, // keyctl
    217, // add_key
    218, // request_key
    280, // bpf
    282, // userfaultfd
    241, // perf_event_open
    273, // finit_module
    60,  // quotactl
    89,  // acct
    270, // process_vm_readv
    271, // process_vm_writev
];

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const DENIED: &[u32] = &[];

const EPERM: u32 = 1;

// Syscalls added after Linux 5.1 share one number on every architecture we
// build for (x86_64, aarch64, 32-bit ARM EABI; see the unified table in
// include/uapi/asm-generic/unistd.h and arch/*/syscall.tbl). An earlier list
// carried 431 labelled "process_vm_readv-like"; 431 is really `fsconfig`, so
// it is kept here under its real name together with the rest of the new
// mount API (an alternative to the denied `mount`), io_uring (its requests
// are not seen by seccomp) and pidfd_getfd (steals another process's fds).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "arm"))]
pub const DENIED_UNIFIED: &[u32] = &[
    425, // io_uring_setup
    426, // io_uring_enter
    427, // io_uring_register
    428, // open_tree
    429, // move_mount
    430, // fsopen
    431, // fsconfig
    432, // fsmount
    433, // fspick
    438, // pidfd_getfd
    442, // mount_setattr
];

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "arm")))]
pub const DENIED_UNIFIED: &[u32] = &[];

/// Every denied number for this architecture: the per-arch table plus the
/// unified post-5.1 numbers.
pub fn denied() -> impl Iterator<Item = u32> {
    DENIED.iter().chain(DENIED_UNIFIED).copied()
}

fn insn(code: u16, jt: u8, jf: u8, k: u32) -> SockFilter {
    SockFilter { code, jt, jf, k }
}

/// Build the classic BPF program. Layout:
///   1. load arch; if != AUDIT_ARCH -> KILL_PROCESS
///   2. load nr; if >= 0x40000000 (x32) -> KILL_PROCESS
///   3. for each denied nr: if equal -> RET_ERRNO|EPERM
///   4. RET_ALLOW
pub fn build() -> Vec<SockFilter> {
    let n = DENIED.len() + DENIED_UNIFIED.len();
    let mut p = Vec::with_capacity(6 + n * 2 + 1);
    // load arch
    p.push(insn(LD_W_ABS, 0, 0, OFF_ARCH));
    // if arch == AUDIT_ARCH jump over KILL (jt=1), else fall through to RET_KILL
    p.push(insn(JEQ_K, 1, 0, AUDIT_ARCH));
    p.push(insn(RET_K, 0, 0, RET_KILL_PROCESS));
    // load nr
    p.push(insn(LD_W_ABS, 0, 0, OFF_NR));
    // if nr >= 0x40000000 (x32 on x86_64) kill; otherwise fall through.
    p.push(insn(JGE_K, 0, 1, 0x4000_0000));
    p.push(insn(RET_K, 0, 0, RET_KILL_PROCESS));
    // deny list
    for nr in denied() {
        // JEQ: if equal fall into RET_ERRNO; else skip it (jf=1).
        p.push(insn(JEQ_K, 0, 1, nr));
        p.push(insn(RET_K, 0, 0, RET_ERRNO | EPERM));
    }
    p.push(insn(RET_K, 0, 0, RET_ALLOW));
    p
}

/// A filter ready to install. Built in the parent; install() is signal-safe.
pub struct Filter {
    prog: Vec<SockFilter>,
}

impl Filter {
    pub fn new() -> Filter {
        Filter { prog: build() }
    }

    /// Install NO_NEW_PRIVS then the filter. Returns Ok(()) on success.
    /// Only two prctl calls; no allocation.
    pub fn install(&self) -> Result<(), i32> {
        // SAFETY: PR_SET_NO_NEW_PRIVS with the documented args; required before
        // an unprivileged process can load a seccomp filter.
        let r = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1i64, 0, 0, 0) };
        if r != 0 {
            return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1));
        }
        let fprog = SockFprog {
            len: self.prog.len() as u16,
            filter: self.prog.as_ptr(),
        };
        // SAFETY: fprog points at our owned Vec for the duration of the call.
        let r = unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                SECCOMP_MODE_FILTER as i64,
                &fprog as *const _ as i64,
                0,
                0,
            )
        };
        if r != 0 {
            return Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1));
        }
        Ok(())
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self::new()
    }
}

/// Probe whether this process can install a filter (does not leave one on).
/// Forks a child that tries to install, then exits. Used by `omnode doctor`.
pub fn available() -> bool {
    let f = Filter::new();
    // SAFETY: the child only runs `install` (two prctl calls on a program
    // built before the fork), which allocates nothing.
    unsafe { crate::util::fork_probe(|| f.install().is_ok()) }
}

/// Result of [`self_test`]: the filter is installed in throw-away children
/// and real syscalls are tried under it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct SelfTest {
    /// The filter installed in a child.
    pub installed: bool,
    /// `ptrace(PTRACE_TRACEME)` failed with EPERM under the filter.
    pub ptrace_denied: bool,
    /// `fsconfig` (431) failed with EPERM under the filter, not EBADF/ENOSYS.
    pub fsconfig_denied: bool,
    /// `unshare(CLONE_NEWUSER)` failed with EPERM under the filter.
    pub unshare_denied: bool,
    /// `getppid` and `write` still work under the filter.
    pub everyday_allowed: bool,
    /// All of the above.
    pub enforced: bool,
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Install the filter in forked children and check that it really denies
/// (EPERM) and really allows. Never changes this process. Fail-closed: any
/// fork or install failure reads as not enforced.
pub fn self_test() -> SelfTest {
    let f = Filter::new();
    // SAFETY (all probes): the child runs only `install` and plain syscalls,
    // no allocation, locks or unwinding; see `util::fork_probe`.
    let installed = unsafe { crate::util::fork_probe(|| f.install().is_ok()) };
    if !installed {
        return SelfTest::default();
    }
    let ptrace_denied = unsafe {
        crate::util::fork_probe(|| {
            f.install().is_ok()
                && libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) < 0
                && errno() == libc::EPERM
        })
    };
    let fsconfig_denied = unsafe {
        crate::util::fork_probe(|| {
            f.install().is_ok()
                && {
                    let bad_fd: libc::c_long = -1;
                    libc::syscall(libc::SYS_fsconfig, bad_fd, 0, 0, 0, 0) < 0
                }
                && errno() == libc::EPERM
        })
    };
    let unshare_denied = unsafe {
        crate::util::fork_probe(|| {
            f.install().is_ok() && libc::unshare(libc::CLONE_NEWUSER) < 0 && errno() == libc::EPERM
        })
    };
    let everyday_allowed = unsafe {
        crate::util::fork_probe(|| {
            let msg = b"";
            f.install().is_ok()
                && libc::getppid() > 0
                && libc::write(2, msg.as_ptr().cast(), 0) == 0
        })
    };
    SelfTest {
        installed,
        ptrace_denied,
        fsconfig_denied,
        unshare_denied,
        everyday_allowed,
        enforced: ptrace_denied && fsconfig_denied && unshare_denied && everyday_allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_ends_in_allow_and_denies_ptrace() {
        let p = build();
        assert_eq!(p.last().unwrap().k, RET_ALLOW);
        #[cfg(target_arch = "x86_64")]
        assert!(p.iter().any(|i| i.code == JEQ_K && i.k == 101));
        #[cfg(target_arch = "aarch64")]
        assert!(p.iter().any(|i| i.code == JEQ_K && i.k == 117));
        assert_eq!(p[0].code, LD_W_ABS);
        assert_eq!(p[0].k, OFF_ARCH);
        assert_eq!(p[1].code, JEQ_K);
        assert_eq!(p[1].k, AUDIT_ARCH);
    }

    #[test]
    fn install_in_child_denies_ptrace() {
        if !available() {
            eprintln!("seccomp unavailable here; skipping enforce test");
            return;
        }
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            let f = Filter::new();
            if f.install().is_err() {
                unsafe { libc::_exit(2) };
            }
            // ptrace(PTRACE_TRACEME) must fail with EPERM under the filter.
            let r = unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) };
            let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            unsafe { libc::_exit(if r < 0 && err == libc::EPERM { 0 } else { 3 }) };
        }
        let mut st = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut st, 0) }, pid);
        assert!(libc::WIFEXITED(st), "child status {st:#x}");
        assert_eq!(
            libc::WEXITSTATUS(st),
            0,
            "child exited {}",
            libc::WEXITSTATUS(st)
        );
    }

    #[test]
    fn install_still_allows_write() {
        if !available() {
            return;
        }
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            let f = Filter::new();
            if f.install().is_err() {
                unsafe { libc::_exit(2) };
            }
            let msg = b"ok\n";
            let r = unsafe { libc::write(2, msg.as_ptr().cast(), msg.len()) };
            unsafe { libc::_exit(if r == msg.len() as isize { 0 } else { 3 }) };
        }
        let mut st = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut st, 0) }, pid);
        assert_eq!(libc::WEXITSTATUS(st), 0);
    }

    /// Every denied number must be the libc `SYS_*` constant of the syscall
    /// it claims to be, on the architecture being built (431 was once listed
    /// as "process_vm_readv-like"; it is fsconfig).
    #[test]
    fn denied_numbers_match_libc_for_this_arch() {
        #[cfg(target_arch = "x86_64")]
        let arch_only: &[libc::c_long] = &[libc::SYS_iopl, libc::SYS_ioperm];
        #[cfg(not(target_arch = "x86_64"))]
        let arch_only: &[libc::c_long] = &[];
        let common: &[libc::c_long] = &[
            libc::SYS_ptrace,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_reboot,
            libc::SYS_unshare,
            libc::SYS_swapon,
            libc::SYS_swapoff,
            libc::SYS_sethostname,
            libc::SYS_setdomainname,
            libc::SYS_init_module,
            libc::SYS_delete_module,
            libc::SYS_kexec_load,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_request_key,
            libc::SYS_bpf,
            libc::SYS_userfaultfd,
            libc::SYS_perf_event_open,
            libc::SYS_finit_module,
            libc::SYS_quotactl,
            libc::SYS_acct,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
            libc::SYS_open_tree,
            libc::SYS_move_mount,
            libc::SYS_fsopen,
            libc::SYS_fsconfig,
            libc::SYS_fsmount,
            libc::SYS_fspick,
            libc::SYS_pidfd_getfd,
            libc::SYS_mount_setattr,
        ];
        #[cfg(target_arch = "x86_64")]
        let kexec_file: &[libc::c_long] = &[libc::SYS_kexec_file_load];
        // asm-generic __NR_kexec_file_load; libc 0.2.191 has no constant for
        // aarch64 musl.
        #[cfg(target_arch = "aarch64")]
        let kexec_file: &[libc::c_long] = &[294];
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let kexec_file: &[libc::c_long] = &[];
        let mut want: Vec<u32> = arch_only
            .iter()
            .chain(common)
            .chain(kexec_file)
            .map(|&n| n as u32)
            .collect();
        let mut have: Vec<u32> = denied().collect();
        want.sort_unstable();
        have.sort_unstable();
        let dup = have.windows(2).any(|w| w[0] == w[1]);
        assert!(!dup, "duplicate number in deny list: {have:?}");
        assert_eq!(have, want, "deny list does not match libc SYS_* numbers");
        assert!(have.contains(&(libc::SYS_fsconfig as u32)));
    }

    #[test]
    fn fsconfig_is_denied_in_the_program() {
        let p = build();
        assert!(p
            .iter()
            .any(|i| i.code == JEQ_K && i.k == libc::SYS_fsconfig as u32));
        // Each denied nr is one JEQ + one RET_ERRNO; 6 header insns + ALLOW.
        assert_eq!(p.len(), 6 + denied().count() * 2 + 1);
    }

    #[test]
    fn self_test_enforces_when_available() {
        let t = self_test();
        if !available() {
            assert!(!t.enforced, "unavailable seccomp must not read as enforced");
            eprintln!("SKIP: seccomp unavailable here");
            return;
        }
        assert!(t.installed);
        assert!(t.ptrace_denied, "{t:?}");
        assert!(t.fsconfig_denied, "{t:?}");
        assert!(t.unshare_denied, "{t:?}");
        assert!(t.everyday_allowed, "{t:?}");
        assert!(t.enforced);
    }
}
