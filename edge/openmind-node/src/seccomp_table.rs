// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Cross-architecture view of the seccomp deny-list (security plan S4.3).
//!
//! The per-arch deny arrays live here, compiled on every target, so one host
//! can check all three of them. [`CATALOG`] maps every syscall the deny-list
//! cares about to its number on x86_64, aarch64 and 32-bit ARM EABI (armv7),
//! taken from the kernel's own tables:
//!   - x86_64:  arch/x86/entry/syscalls/syscall_64.tbl (abi `common` / `64`)
//!   - aarch64: scripts/syscall.tbl (asm-generic; abi `common` / `64`)
//!   - armv7:   arch/arm/tools/syscall.tbl (abi `common` / `oabi`)
//!
//! Parity rule: when a syscall is denied on one architecture it must be denied
//! on every other one, or there must be a written [`EXCEPTIONS`] entry saying
//! why (normally: the syscall does not exist there). `omnode sandbox denylist
//! --markdown` renders the table checked in at docs/security/denylist-parity.md
//! and a unit test fails when the checked-in copy is stale.

/// The three architectures omnode ships a deny-list for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Armv7,
}

pub const ARCHES: [Arch; 3] = [Arch::X86_64, Arch::Aarch64, Arch::Armv7];

impl Arch {
    pub fn name(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Armv7 => "armv7",
        }
    }

    /// The per-arch deny array (without the unified post-5.1 numbers).
    pub fn denied_own(self) -> &'static [u32] {
        match self {
            Arch::X86_64 => DENIED_X86_64,
            Arch::Aarch64 => DENIED_AARCH64,
            Arch::Armv7 => DENIED_ARMV7,
        }
    }

    /// Every number the filter for this arch denies.
    pub fn denied(self) -> impl Iterator<Item = u32> {
        self.denied_own().iter().chain(DENIED_UNIFIED).copied()
    }

    /// The architecture this binary was compiled for, if it is one of ours.
    pub fn current() -> Option<Arch> {
        if cfg!(target_arch = "x86_64") {
            Some(Arch::X86_64)
        } else if cfg!(target_arch = "aarch64") {
            Some(Arch::Aarch64)
        } else if cfg!(target_arch = "arm") {
            Some(Arch::Armv7)
        } else {
            None
        }
    }
}

// ---- Denied syscall numbers per architecture ----
// Goal: block escape / host change / process inspection. Everyday shell
// tools (read, write, open, mmap, clone for threads, socket inside a netns
// that is already empty, etc.) stay allowed. Numbers from the kernel tables
// named in the module docs; every one is checked against CATALOG.

pub const DENIED_X86_64: &[u32] = &[
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

pub const DENIED_AARCH64: &[u32] = &[
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

// armv7 (32-bit ARM EABI) numbers from arch/arm/tools/syscall.tbl. Same set
// as aarch64 plus the OABI-only `umount` (22). kexec_file_load (401) was
// missing here until slot 220 found it with the parity table.
pub const DENIED_ARMV7: &[u32] = &[
    26,  // ptrace
    21,  // mount
    52,  // umount2
    22,  // umount (OABI only; EABI kernels answer ENOSYS anyway)
    218, // pivot_root
    88,  // reboot
    337, // unshare
    87,  // swapon
    115, // swapoff
    74,  // sethostname
    121, // setdomainname
    128, // init_module
    129, // delete_module
    347, // kexec_load
    401, // kexec_file_load
    311, // keyctl
    309, // add_key
    310, // request_key
    386, // bpf
    388, // userfaultfd
    364, // perf_event_open
    379, // finit_module
    131, // quotactl
    51,  // acct
    376, // process_vm_readv
    377, // process_vm_writev
];

// Syscalls added after Linux 5.1 share one number on every architecture we
// build for (x86_64, aarch64, 32-bit ARM EABI; see the unified table in
// scripts/syscall.tbl and arch/*/syscall.tbl). An earlier list carried 431
// labelled "process_vm_readv-like"; 431 is really `fsconfig`, so it is kept
// here under its real name together with the rest of the new mount API (an
// alternative to the denied `mount`), io_uring (its requests are not seen by
// seccomp) and pidfd_getfd (steals another process's fds). quotactl_fd and
// open_tree_attr were added by slot 220: they are the fd-based twins of the
// denied quotactl and open_tree.
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
    443, // quotactl_fd
    467, // open_tree_attr
];

/// One syscall and its number on each architecture (`None`: not in that
/// architecture's table).
#[derive(Clone, Copy, Debug)]
pub struct Sys {
    pub name: &'static str,
    pub x86_64: Option<u32>,
    pub aarch64: Option<u32>,
    pub armv7: Option<u32>,
    /// Why it is in the table: what it lets a process do.
    pub what: &'static str,
}

impl Sys {
    pub fn nr(&self, a: Arch) -> Option<u32> {
        match a {
            Arch::X86_64 => self.x86_64,
            Arch::Aarch64 => self.aarch64,
            Arch::Armv7 => self.armv7,
        }
    }

    pub fn denied_on(&self, a: Arch) -> bool {
        self.nr(a).is_some_and(|n| a.denied().any(|d| d == n))
    }
}

const fn s(
    name: &'static str,
    x86_64: Option<u32>,
    aarch64: Option<u32>,
    armv7: Option<u32>,
    what: &'static str,
) -> Sys {
    Sys {
        name,
        x86_64,
        aarch64,
        armv7,
        what,
    }
}

const fn u(name: &'static str, nr: u32, what: &'static str) -> Sys {
    s(name, Some(nr), Some(nr), Some(nr), what)
}

/// Every denied syscall plus the escape-adjacent ones that are deliberately
/// allowed (listed so a reviewer sees them, and so a later deny on one arch
/// only is caught).
pub const CATALOG: &[Sys] = &[
    s(
        "ptrace",
        Some(101),
        Some(117),
        Some(26),
        "inspect/control another process",
    ),
    s("mount", Some(165), Some(40), Some(21), "mount a filesystem"),
    s("umount2", Some(166), Some(39), Some(52), "unmount"),
    s("umount", None, None, Some(22), "unmount (old ABI)"),
    s(
        "pivot_root",
        Some(155),
        Some(41),
        Some(218),
        "change the root mount",
    ),
    s(
        "reboot",
        Some(169),
        Some(142),
        Some(88),
        "reboot / kexec the host",
    ),
    s("unshare", Some(272), Some(97), Some(337), "new namespaces"),
    s("swapon", Some(167), Some(224), Some(87), "host swap"),
    s("swapoff", Some(168), Some(225), Some(115), "host swap"),
    s(
        "sethostname",
        Some(170),
        Some(161),
        Some(74),
        "change host name",
    ),
    s(
        "setdomainname",
        Some(171),
        Some(162),
        Some(121),
        "change domain name",
    ),
    s("iopl", Some(172), None, None, "raw I/O port access"),
    s("ioperm", Some(173), None, None, "raw I/O port access"),
    s(
        "init_module",
        Some(175),
        Some(105),
        Some(128),
        "load a kernel module",
    ),
    s(
        "finit_module",
        Some(313),
        Some(273),
        Some(379),
        "load a kernel module",
    ),
    s(
        "delete_module",
        Some(176),
        Some(106),
        Some(129),
        "unload a kernel module",
    ),
    s(
        "kexec_load",
        Some(246),
        Some(104),
        Some(347),
        "boot another kernel",
    ),
    s(
        "kexec_file_load",
        Some(320),
        Some(294),
        Some(401),
        "boot another kernel",
    ),
    s("keyctl", Some(250), Some(219), Some(311), "kernel keyrings"),
    s(
        "add_key",
        Some(248),
        Some(217),
        Some(309),
        "kernel keyrings",
    ),
    s(
        "request_key",
        Some(249),
        Some(218),
        Some(310),
        "kernel keyrings",
    ),
    s("bpf", Some(321), Some(280), Some(386), "load eBPF programs"),
    s(
        "userfaultfd",
        Some(323),
        Some(282),
        Some(388),
        "kernel-exploit primitive",
    ),
    s(
        "perf_event_open",
        Some(298),
        Some(241),
        Some(364),
        "kernel/host profiling",
    ),
    s(
        "quotactl",
        Some(179),
        Some(60),
        Some(131),
        "filesystem quotas",
    ),
    s(
        "acct",
        Some(163),
        Some(89),
        Some(51),
        "process accounting file",
    ),
    s(
        "process_vm_readv",
        Some(310),
        Some(270),
        Some(376),
        "read another process's memory",
    ),
    s(
        "process_vm_writev",
        Some(311),
        Some(271),
        Some(377),
        "write another process's memory",
    ),
    u("io_uring_setup", 425, "io_uring (requests bypass seccomp)"),
    u("io_uring_enter", 426, "io_uring (requests bypass seccomp)"),
    u(
        "io_uring_register",
        427,
        "io_uring (requests bypass seccomp)",
    ),
    u("open_tree", 428, "new mount API"),
    u("move_mount", 429, "new mount API"),
    u("fsopen", 430, "new mount API"),
    u("fsconfig", 431, "new mount API"),
    u("fsmount", 432, "new mount API"),
    u("fspick", 433, "new mount API"),
    u("pidfd_getfd", 438, "steal another process's fd"),
    u("mount_setattr", 442, "new mount API"),
    u("quotactl_fd", 443, "filesystem quotas (fd form)"),
    u(
        "open_tree_attr",
        467,
        "new mount API (open_tree + attrs, Linux 6.15)",
    ),
    // Deliberately allowed (watch list).
    s(
        "setns",
        Some(308),
        Some(268),
        Some(375),
        "join a namespace by fd",
    ),
    s(
        "chroot",
        Some(161),
        Some(51),
        Some(61),
        "change root directory",
    ),
    s(
        "kcmp",
        Some(312),
        Some(272),
        Some(378),
        "compare kernel objects of two processes",
    ),
    s(
        "syslog",
        Some(103),
        Some(116),
        Some(103),
        "read the kernel log",
    ),
    s(
        "uselib",
        Some(134),
        None,
        Some(86),
        "legacy a.out library loader",
    ),
    s(
        "modify_ldt",
        Some(154),
        None,
        None,
        "x86 local descriptor table",
    ),
    s(
        "_sysctl",
        Some(156),
        None,
        Some(149),
        "old sysctl (removed in Linux 5.5)",
    ),
    u("process_madvise", 440, "advise on another process's memory"),
    // Raw numbers the openmind-sandbox crate calls itself (it has no
    // seccomp filter of its own; checked against its source in tests).
    u("close_range", 436, "used by openmind-sandbox"),
    u("landlock_create_ruleset", 444, "used by openmind-sandbox"),
    u("landlock_add_rule", 445, "used by openmind-sandbox"),
    u("landlock_restrict_self", 446, "used by openmind-sandbox"),
];

/// Deliberately allowed escape-adjacent syscalls and why.
pub const ALLOWED_NOTES: &[(&str, &str)] = &[
    ("setns", "needs a namespace fd and CAP_SYS_ADMIN over its owner; the child has none after unshare is denied"),
    ("chroot", "needs CAP_SYS_CHROOT; harmless without it; some tools probe it"),
    ("kcmp", "ptrace-access check on both pids; ptrace itself is denied"),
    ("syslog", "kernel.dmesg_restrict governs it; denying breaks nothing but is not needed for escape"),
    ("uselib", "legacy a.out loader, usually compiled out (CONFIG_USELIB); same on both arches that have it"),
    ("modify_ldt", "x86 only; candidate for D-220-3 (deny on x86_64)"),
    ("_sysctl", "removed in Linux 5.5 (ENOSYS)"),
    ("process_madvise", "needs ptrace-access (PTRACE_MODE_READ) plus CAP_SYS_NICE over the target"),
    ("close_range", "openmind-sandbox closes inherited fds with it; must stay allowed"),
    ("landlock_create_ruleset", "self-restriction only; omnode's own sandbox uses Landlock"),
    ("landlock_add_rule", "self-restriction only"),
    ("landlock_restrict_self", "self-restriction only"),
];

/// A written reason why a syscall denied elsewhere is not denied on `arch`.
#[derive(Clone, Copy, Debug)]
pub struct Exception {
    pub name: &'static str,
    pub arch: Arch,
    pub reason: &'static str,
}

const NOT_ARM: &str = "x86-only syscall; not in the arm64/arm tables";
const OABI: &str = "32-bit ARM OABI-only call; no such syscall on this arch";

pub const EXCEPTIONS: &[Exception] = &[
    Exception {
        name: "iopl",
        arch: Arch::Aarch64,
        reason: NOT_ARM,
    },
    Exception {
        name: "iopl",
        arch: Arch::Armv7,
        reason: NOT_ARM,
    },
    Exception {
        name: "ioperm",
        arch: Arch::Aarch64,
        reason: NOT_ARM,
    },
    Exception {
        name: "ioperm",
        arch: Arch::Armv7,
        reason: NOT_ARM,
    },
    Exception {
        name: "umount",
        arch: Arch::X86_64,
        reason: OABI,
    },
    Exception {
        name: "umount",
        arch: Arch::Aarch64,
        reason: OABI,
    },
];

/// One parity problem.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Violation {
    pub syscall: String,
    pub arch: &'static str,
    pub problem: String,
}

fn v(syscall: &str, arch: Arch, problem: String) -> Violation {
    Violation {
        syscall: syscall.to_string(),
        arch: arch.name(),
        problem,
    }
}

#[cfg(test)]
fn exception(name: &str, a: Arch) -> Option<&'static Exception> {
    EXCEPTIONS.iter().find(|e| e.name == name && e.arch == a)
}

/// Check the three deny arrays against the catalog and each other. Empty
/// result = parity holds.
pub fn check() -> Vec<Violation> {
    check_with(CATALOG, EXCEPTIONS, &|a| a.denied().collect())
}

/// [`check`] over explicit data (tests feed broken tables through this).
pub fn check_with(
    cat: &[Sys],
    exc: &[Exception],
    denied: &dyn Fn(Arch) -> Vec<u32>,
) -> Vec<Violation> {
    let mut out = Vec::new();
    for a in ARCHES {
        let d = denied(a);
        // Every denied number is a known syscall on that arch, once.
        for (i, n) in d.iter().enumerate() {
            if d[..i].contains(n) {
                out.push(v("?", a, format!("number {n} listed twice")));
            }
            if !cat.iter().any(|s| s.nr(a) == Some(*n)) {
                out.push(v(
                    "?",
                    a,
                    format!("number {n} is not in the catalog for this arch"),
                ));
            }
        }
        // A catalog number is unique per arch (catches typos in the table).
        for (i, s) in cat.iter().enumerate() {
            if let Some(n) = s.nr(a) {
                if let Some(o) = cat[..i].iter().find(|o| o.nr(a) == Some(n)) {
                    out.push(v(
                        s.name,
                        a,
                        format!("catalog number {n} also used by {}", o.name),
                    ));
                }
            }
        }
    }
    for s in cat {
        let on: Vec<Arch> = ARCHES
            .into_iter()
            .filter(|&a| s.nr(a).is_some_and(|n| denied(a).contains(&n)))
            .collect();
        for a in ARCHES {
            let denied_here = on.contains(&a);
            let exc_here = exc.iter().find(|e| e.name == s.name && e.arch == a);
            if denied_here {
                if exc_here.is_some() {
                    out.push(v(s.name, a, "stale exception: denied here anyway".into()));
                }
                continue;
            }
            if on.is_empty() {
                if exc_here.is_some() {
                    out.push(v(
                        s.name,
                        a,
                        "stale exception: not denied on any arch".into(),
                    ));
                }
                continue;
            }
            match exc_here {
                Some(e) if !e.reason.trim().is_empty() => {}
                Some(_) => out.push(v(s.name, a, "exception without a written reason".into())),
                None => {
                    let others: Vec<&str> = on.iter().map(|a| a.name()).collect();
                    let what = match s.nr(a) {
                        Some(n) => {
                            format!("allowed here (nr {n}) but denied on {}", others.join(", "))
                        }
                        None => format!("missing here but denied on {}", others.join(", ")),
                    };
                    out.push(v(s.name, a, what));
                }
            }
        }
    }
    for e in exc {
        if !cat.iter().any(|s| s.name == e.name) {
            out.push(v(
                e.name,
                e.arch,
                "exception names a syscall not in the catalog".into(),
            ));
        }
    }
    out
}

fn cell(s: &Sys, a: Arch) -> String {
    match s.nr(a) {
        None => "-".into(),
        Some(n) if s.denied_on(a) => format!("{n} DENY"),
        Some(n) => format!("{n} allow"),
    }
}

/// The checked-in parity table (docs/security/denylist-parity.md). Pure
/// function of the data above, so a stale doc fails the unit test.
pub fn markdown() -> String {
    let mut m = String::new();
    m.push_str("<!-- SPDX-License-Identifier: MIT -->\n");
    m.push_str("<!-- Copyright (c) 3000 Galion / galion-studio -->\n");
    m.push_str("<!-- GENERATED by `omnode sandbox denylist --markdown` from edge/openmind-node/src/seccomp_table.rs. Do not edit by hand; regenerate and commit. -->\n\n");
    m.push_str("# Seccomp deny-list parity: x86_64 / aarch64 / armv7\n\n");
    m.push_str("Security plan S4.3. The only seccomp deny-list in the repository is omnode's\n");
    m.push_str(
        "(`edge/openmind-node/src/seccomp.rs`, default ALLOW, listed syscalls return EPERM,\n",
    );
    m.push_str("foreign ABI killed). `crates/openmind-sandbox` has no seccomp filter by design\n");
    m.push_str("(userns + Landlock + rlimits + caps); the raw syscall numbers it calls itself\n");
    m.push_str("are listed at the bottom of the table and checked against its source.\n\n");
    m.push_str(
        "Rule (unit test `seccomp_table::tests::parity_holds`, run by CI in the omnode job):\n",
    );
    m.push_str(
        "a syscall denied on one architecture must be denied on every other one, unless an\n",
    );
    m.push_str("exception below says why (normally: the syscall does not exist there).\n\n");
    m.push_str(
        "Numbers: x86_64 `arch/x86/entry/syscalls/syscall_64.tbl`, aarch64 `scripts/syscall.tbl`\n",
    );
    m.push_str("(asm-generic), armv7 `arch/arm/tools/syscall.tbl` (Linux master, read 2026-10-21 +02:00).\n");
    m.push_str("`-` = no such syscall on that arch.\n\n");
    m.push_str("| syscall | x86_64 | aarch64 | armv7 | what it allows |\n");
    m.push_str("|---|---|---|---|---|\n");
    for s in CATALOG {
        m.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            s.name,
            cell(s, Arch::X86_64),
            cell(s, Arch::Aarch64),
            cell(s, Arch::Armv7),
            s.what
        ));
    }
    m.push_str("\n## Totals\n\n| arch | denied numbers (own + unified) |\n|---|---|\n");
    for a in ARCHES {
        m.push_str(&format!(
            "| {} | {} ({} + {}) |\n",
            a.name(),
            a.denied().count(),
            a.denied_own().len(),
            DENIED_UNIFIED.len()
        ));
    }
    m.push_str("\n## Written exceptions (denied elsewhere, not here)\n\n| syscall | arch | reason |\n|---|---|---|\n");
    for e in EXCEPTIONS {
        m.push_str(&format!(
            "| `{}` | {} | {} |\n",
            e.name,
            e.arch.name(),
            e.reason
        ));
    }
    m.push_str("\n## Deliberately allowed on every arch\n\n| syscall | why |\n|---|---|\n");
    for (n, why) in ALLOWED_NOTES {
        m.push_str(&format!("| `{n}` | {why} |\n"));
    }
    let viol = check();
    m.push_str("\n## Parity result\n\n");
    if viol.is_empty() {
        m.push_str("PASS: 0 violations.\n");
    } else {
        m.push_str(&format!("FAIL: {} violations.\n\n", viol.len()));
        for x in &viol {
            m.push_str(&format!("- `{}` {}: {}\n", x.syscall, x.arch, x.problem));
        }
    }
    m.push_str("\n## Not covered by the number table\n\n");
    m.push_str(
        "- x86_64 x32 (nr | 0x40000000) and i386 calls: the filter kills any nr >= 0x40000000\n",
    );
    m.push_str("  and any arch other than the build arch, so those tables need no rows.\n");
    m.push_str(
        "- 32-bit ARM private calls (0x0f0000+, e.g. `cacheflush`, `set_tls`) are allowed;\n",
    );
    m.push_str("  none of them changes the host or reaches another process.\n");
    m.push_str(
        "- Enforcement on real aarch64/armv7 kernels is a separate test (sandbox runs only on\n",
    );
    m.push_str(
        "  the VPS; see plans/security.md). This table proves the lists agree, not that a\n",
    );
    m.push_str("  given kernel loads them.\n");
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = include_str!("../../../docs/security/denylist-parity.md");
    const SANDBOX_SRC: &str = include_str!("../../../crates/openmind-sandbox/src/lib.rs");

    #[test]
    fn parity_holds() {
        let viol = check();
        assert!(viol.is_empty(), "deny-list parity violations: {viol:#?}");
    }

    #[test]
    fn checked_in_doc_is_current() {
        assert!(
            DOC == markdown(),
            "docs/security/denylist-parity.md is stale; run \
             `omnode sandbox denylist --markdown > docs/security/denylist-parity.md`"
        );
    }

    #[test]
    fn denied_on_one_arch_only_fails() {
        // Drop kexec_file_load (401) from armv7: the exact gap slot 220 found.
        let viol = check_with(CATALOG, EXCEPTIONS, &|a| {
            a.denied()
                .filter(|&n| !(a == Arch::Armv7 && n == 401))
                .collect()
        });
        assert_eq!(viol.len(), 1, "{viol:#?}");
        assert_eq!(viol[0].syscall, "kexec_file_load");
        assert_eq!(viol[0].arch, "armv7");
        assert!(
            viol[0].problem.contains("allowed here (nr 401)"),
            "{viol:?}"
        );
    }

    #[test]
    fn missing_without_exception_fails() {
        let viol = check_with(CATALOG, &EXCEPTIONS[1..], &|a| a.denied().collect());
        assert_eq!(viol.len(), 1, "{viol:#?}");
        assert_eq!(
            (viol[0].syscall.as_str(), viol[0].arch),
            ("iopl", "aarch64")
        );
        assert!(viol[0].problem.starts_with("missing here"));
    }

    #[test]
    fn stale_or_empty_exception_fails() {
        let mut exc = EXCEPTIONS.to_vec();
        exc.push(Exception {
            name: "ptrace",
            arch: Arch::Armv7,
            reason: "x",
        });
        exc.push(Exception {
            name: "setns",
            arch: Arch::X86_64,
            reason: "x",
        });
        exc[0].reason = " ";
        let viol = check_with(CATALOG, &exc, &|a| a.denied().collect());
        let p: Vec<_> = viol.iter().map(|x| (x.syscall.as_str(), x.arch)).collect();
        assert_eq!(
            p,
            [
                ("ptrace", "armv7"),
                ("iopl", "aarch64"),
                ("setns", "x86_64")
            ],
            "{viol:#?}"
        );
    }

    #[test]
    fn unknown_or_duplicate_number_fails() {
        let viol = check_with(CATALOG, EXCEPTIONS, &|a| {
            let mut d: Vec<u32> = a.denied().collect();
            if a == Arch::Aarch64 {
                d.push(9999);
                d.push(117);
            }
            d
        });
        assert_eq!(viol.len(), 2, "{viol:#?}");
        assert!(viol.iter().all(|x| x.arch == "aarch64"));
    }

    #[test]
    fn unified_rows_share_one_number() {
        for s in CATALOG
            .iter()
            .filter(|s| s.x86_64.is_some_and(|n| n >= 424))
        {
            assert_eq!(s.x86_64, s.aarch64, "{}", s.name);
            assert_eq!(s.x86_64, s.armv7, "{}", s.name);
        }
        for n in DENIED_UNIFIED {
            assert!(*n >= 424, "unified table holds only post-5.1 numbers: {n}");
        }
    }

    /// The live filter for the build arch is exactly the table's array.
    #[test]
    fn compiled_filter_uses_table() {
        let a = Arch::current().expect("tests run on x86_64/aarch64/arm");
        let live: Vec<u32> = crate::seccomp::denied().collect();
        let want: Vec<u32> = a.denied().collect();
        assert_eq!(live, want);
    }

    /// Catalog numbers for the build arch agree with libc's SYS_* constants
    /// (a second, independent source next to the kernel tables).
    #[test]
    fn catalog_matches_libc_for_this_arch() {
        let a = Arch::current().unwrap();
        let by = |n: &str| CATALOG.iter().find(|s| s.name == n).unwrap().nr(a);
        let pairs: &[(&str, libc::c_long)] = &[
            ("ptrace", libc::SYS_ptrace),
            ("mount", libc::SYS_mount),
            ("umount2", libc::SYS_umount2),
            ("pivot_root", libc::SYS_pivot_root),
            ("reboot", libc::SYS_reboot),
            ("unshare", libc::SYS_unshare),
            ("swapon", libc::SYS_swapon),
            ("swapoff", libc::SYS_swapoff),
            ("sethostname", libc::SYS_sethostname),
            ("setdomainname", libc::SYS_setdomainname),
            ("init_module", libc::SYS_init_module),
            ("finit_module", libc::SYS_finit_module),
            ("delete_module", libc::SYS_delete_module),
            ("kexec_load", libc::SYS_kexec_load),
            ("keyctl", libc::SYS_keyctl),
            ("add_key", libc::SYS_add_key),
            ("request_key", libc::SYS_request_key),
            ("bpf", libc::SYS_bpf),
            ("userfaultfd", libc::SYS_userfaultfd),
            ("perf_event_open", libc::SYS_perf_event_open),
            ("quotactl", libc::SYS_quotactl),
            ("acct", libc::SYS_acct),
            ("process_vm_readv", libc::SYS_process_vm_readv),
            ("process_vm_writev", libc::SYS_process_vm_writev),
            ("io_uring_setup", libc::SYS_io_uring_setup),
            ("io_uring_enter", libc::SYS_io_uring_enter),
            ("io_uring_register", libc::SYS_io_uring_register),
            ("open_tree", libc::SYS_open_tree),
            ("move_mount", libc::SYS_move_mount),
            ("fsopen", libc::SYS_fsopen),
            ("fsconfig", libc::SYS_fsconfig),
            ("fsmount", libc::SYS_fsmount),
            ("fspick", libc::SYS_fspick),
            ("pidfd_getfd", libc::SYS_pidfd_getfd),
            ("mount_setattr", libc::SYS_mount_setattr),
            ("quotactl_fd", libc::SYS_quotactl_fd),
            ("setns", libc::SYS_setns),
            ("chroot", libc::SYS_chroot),
            ("kcmp", libc::SYS_kcmp),
            ("syslog", libc::SYS_syslog),
            ("process_madvise", libc::SYS_process_madvise),
            ("close_range", libc::SYS_close_range),
            ("landlock_create_ruleset", libc::SYS_landlock_create_ruleset),
            ("landlock_add_rule", libc::SYS_landlock_add_rule),
            ("landlock_restrict_self", libc::SYS_landlock_restrict_self),
        ];
        for (n, sys) in pairs {
            assert_eq!(by(n), Some(*sys as u32), "{n} on {}", a.name());
        }
        #[cfg(target_arch = "x86_64")]
        {
            assert_eq!(by("iopl"), Some(libc::SYS_iopl as u32));
            assert_eq!(by("ioperm"), Some(libc::SYS_ioperm as u32));
            assert_eq!(
                by("kexec_file_load"),
                Some(libc::SYS_kexec_file_load as u32)
            );
            assert_eq!(by("uselib"), Some(libc::SYS_uselib as u32));
            assert_eq!(by("modify_ldt"), Some(libc::SYS_modify_ldt as u32));
        }
    }

    /// openmind-sandbox (no seccomp filter) calls raw syscall numbers it
    /// declares itself; they must equal the unified numbers on every arch.
    #[test]
    fn openmind_sandbox_raw_numbers_match() {
        let map = [
            ("SYS_MOUNT_SETATTR", "mount_setattr"),
            ("SYS_LL_CREATE", "landlock_create_ruleset"),
            ("SYS_LL_ADD", "landlock_add_rule"),
            ("SYS_LL_RESTRICT", "landlock_restrict_self"),
            ("SYS_CLOSE_RANGE", "close_range"),
        ];
        let mut seen = 0;
        for line in SANDBOX_SRC.lines() {
            let Some(rest) = line.trim().strip_prefix("const ") else {
                continue;
            };
            let Some((name, val)) = rest.split_once(": libc::c_long = ") else {
                continue;
            };
            let Some(&(_, sys)) = map.iter().find(|(c, _)| *c == name) else {
                panic!("openmind-sandbox declares unknown raw syscall {name}; add it to CATALOG");
            };
            let n: u32 = val.trim_end_matches(';').trim().parse().unwrap();
            let row = CATALOG.iter().find(|s| s.name == sys).unwrap();
            for a in ARCHES {
                assert_eq!(row.nr(a), Some(n), "{name} on {}", a.name());
            }
            seen += 1;
        }
        assert_eq!(
            seen,
            map.len(),
            "openmind-sandbox raw syscall constants moved"
        );
    }

    #[test]
    fn exceptions_have_reasons_and_known_names() {
        for e in EXCEPTIONS {
            assert!(!e.reason.trim().is_empty());
            assert!(exception(e.name, e.arch).is_some());
            let row = CATALOG.iter().find(|s| s.name == e.name).unwrap();
            assert!(
                row.nr(e.arch).is_none(),
                "exceptions are only for absent syscalls today"
            );
        }
        for (n, _) in ALLOWED_NOTES {
            let row = CATALOG.iter().find(|s| s.name == *n).expect(n);
            assert!(ARCHES.iter().all(|&a| !row.denied_on(a)), "{n} is denied");
        }
    }
}
