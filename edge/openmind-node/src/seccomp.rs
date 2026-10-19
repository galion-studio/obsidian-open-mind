// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

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
    250, // keyctl
    248, // add_key
    249, // request_key
    321, // bpf
    323, // userfaultfd
    298, // perf_event_open
    313, // finit_module
    179, // quotactl
    163, // acct
    431, // process_vm_readv-like: openat2 is fine; keep 310 process_vm_readv
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

fn insn(code: u16, jt: u8, jf: u8, k: u32) -> SockFilter {
    SockFilter { code, jt, jf, k }
}

/// Build the classic BPF program. Layout:
///   1. load arch; if != AUDIT_ARCH -> KILL_PROCESS
///   2. load nr; if >= 0x40000000 (x32) -> KILL_PROCESS
///   3. for each denied nr: if equal -> RET_ERRNO|EPERM
///   4. RET_ALLOW
pub fn build() -> Vec<SockFilter> {
    let deny = DENIED;
    let mut p = Vec::with_capacity(6 + deny.len() * 2 + 1);
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
    for &nr in deny {
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
    // SAFETY: a simple fork; child never returns to Rust.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return false;
    }
    if pid == 0 {
        let f = Filter::new();
        let ok = f.install().is_ok();
        // SAFETY: _exit so we do not run atexit handlers of the parent.
        unsafe { libc::_exit(if ok { 0 } else { 1 }) };
    }
    let mut st = 0;
    // SAFETY: waitpid for our own child.
    let r = unsafe { libc::waitpid(pid, &mut st, 0) };
    r == pid && libc::WIFEXITED(st) && libc::WEXITSTATUS(st) == 0
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
}
