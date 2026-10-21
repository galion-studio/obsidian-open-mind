// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! openmind-sandbox v1: run one command with rlimits, a cleared environment,
//! a fixed working directory, a wall-clock timeout, `PR_SET_NO_NEW_PRIVS`,
//! and, when the platform allows it without root or seccomp:
//!
//! - unprivileged user namespace (uid/gid mapped 1:1),
//! - empty network namespace (when `net = false`),
//! - PID namespace (the command is PID 1 of its own namespace; it cannot see
//!   or signal anything outside, and everything it started dies with it),
//! - mount namespace with a read-only view of every mount except the work dir
//!   and `fs_write`, the invoking user's home hidden behind an empty tmpfs,
//!   and a private `/proc` when the kernel allows it,
//! - Landlock filesystem rules (ABI 1-3) and Landlock TCP deny (ABI >= 4),
//! - all capabilities dropped (bounding set emptied) before exec,
//! - every inherited file descriptor above stderr marked close-on-exec, so a
//!   descriptor the host process leaked (a socket, a directory outside the
//!   sandbox) never reaches the command,
//! - optionally a per-run cgroup v2 child (pids.max, memory.max, swap off,
//!   group OOM kill) under a delegated parent. The whole cgroup is killed
//!   with `cgroup.kill` at the end, so even processes that left the process
//!   group with their own `setsid` die.
//! - optionally a total size cap for the work dir (`workdir_tmpfs_bytes`):
//!   a private, size-limited tmpfs is mounted over the work dir inside the
//!   sandbox mount namespace (no host root needed). The host work dir is
//!   copied in before the command starts and every change is copied back
//!   after it ends, so writes past the cap fail with ENOSPC instead of
//!   filling the host disk.
//!
//! Linux and Android (Termux) only. No seccomp, no root needed, libc only.
//! Kernel features that a seccomp filter might kill us for (Android apps run
//! under one) are first probed in a throwaway child, so a filtered syscall
//! shows up as "not enforced" instead of crashing.
//!
//! Every layer is reported in [`Outcome::enforced`]; nothing is claimed that
//! was not applied. The timeout only ever signals the child this crate
//! spawned (and the process group it created with `setsid`).
//!
//! Not a hard security boundary against a determined attacker (no seccomp,
//! no disk quota outside the opt-in work dir tmpfs). It keeps agent tool runs bounded and contained.

use std::ffi::CString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

mod cgroup;
mod preset;
mod treecopy;
pub use preset::Preset;

/// Default maximum bytes kept per output stream.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// Layer names, in report order.
pub const LAYERS: [&str; 16] = [
    "rlimits",
    "env",
    "workdir",
    "workdir_quota",
    "no_new_privs",
    "userns",
    "net",
    "pidns",
    "mountns",
    "ro_root",
    "hide_home",
    "proc",
    "landlock",
    "caps",
    "fds",
    "cgroup",
];

/// What to enforce for one run.
#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    pub workdir: PathBuf,
    /// Names copied from the parent env; everything else is dropped.
    pub env_allowlist: Vec<String>,
    /// Variables set explicitly after clearing (e.g. HOME=workdir).
    pub env_set: Vec<(String, String)>,
    pub wall_timeout: Duration,
    /// RLIMIT_CPU seconds.
    pub cpu_secs: u64,
    /// RLIMIT_AS bytes.
    pub mem_bytes: u64,
    /// RLIMIT_NOFILE.
    pub nofile: u64,
    /// RLIMIT_FSIZE bytes (0 = leave unchanged).
    pub fsize_bytes: u64,
    /// Bytes kept per output stream (the rest is drained and dropped).
    pub output_cap: usize,
    /// false = deny network (empty netns, else Landlock TCP deny).
    pub net: bool,
    /// Fail with `Status::Error` instead of running when `net = false` but
    /// neither a network namespace nor Landlock TCP deny could be applied.
    pub require_net_isolation: bool,
    /// Try Landlock when the kernel has it.
    pub landlock: bool,
    /// Landlock read roots (workdir is always added).
    pub fs_read: Vec<PathBuf>,
    /// Writable roots (Landlock + kept writable under `ro_root`).
    pub fs_write: Vec<PathBuf>,
    /// Try unprivileged user/net/pid/mount namespaces.
    pub namespaces: bool,
    /// In a mount namespace: everything read-only except workdir + fs_write.
    pub ro_root: bool,
    /// In a mount namespace: cover these dirs with an empty read-only tmpfs.
    /// Paths that contain the workdir, a read/write root or the program are
    /// skipped (and reported).
    pub hide_paths: Vec<PathBuf>,
    /// Delegated cgroup v2 directory to create a per-run child cgroup in
    /// (the `cgroup-detect` crate finds one). The calling process must be
    /// allowed to move itself there, i.e. it already lives inside the same
    /// delegated subtree. When set and the child cgroup cannot be created or
    /// joined, the run fails closed with `Status::Error`.
    pub cgroup_parent: Option<PathBuf>,
    /// `pids.max` of the per-run cgroup (processes and threads together).
    pub cgroup_pids_max: u64,
    /// `memory.max` of the per-run cgroup in bytes, for the whole process
    /// tree (RLIMIT_AS is per process). Swap is set to 0 and an OOM kills the
    /// whole group.
    pub cgroup_mem_bytes: u64,
    /// `cpu.max` of the per-run cgroup as a percentage of one CPU (100 = one
    /// full core, 200 = two). 0 leaves `cpu.max` alone. When set and the
    /// parent does not delegate the `cpu` controller, the run fails closed.
    pub cgroup_cpu_pct: u32,
    /// Total size cap of the work dir in bytes (0 = off; RLIMIT_FSIZE then
    /// caps single files only). When set, a private tmpfs with `size=` this
    /// value is mounted over the work dir inside the sandbox mount namespace.
    /// The host work dir is copied in first (a work dir already larger than
    /// the cap fails the run closed), and after the command ends every new or
    /// changed file, directory and symlink is copied back (deletions are not;
    /// nothing is copied back when the run is killed at the wall timeout).
    /// tmpfs pages are memory: under a per-run cgroup they count toward
    /// `cgroup_mem_bytes`.
    pub workdir_tmpfs_bytes: u64,
    /// Inode cap of that tmpfs (0 = `workdir_tmpfs_bytes` / 16 KiB, at least
    /// 1024). Also bounds how many entries are copied back.
    pub workdir_tmpfs_inodes: u64,
    /// What to show at /proc when the kernel refuses a fresh proc mount for
    /// the PID namespace (unprivileged LXC with lxcfs, see [`ProcFallback`]).
    pub proc_fallback: ProcFallback,
}

/// What the sandbox shows at `/proc` when a fresh proc mount for its PID
/// namespace is refused. In a user namespace the kernel refuses it when the
/// existing /proc has files mounted over it (`mount_too_revealing`), which is
/// every LXC container with lxcfs (/proc/cpuinfo, meminfo, stat, ... are FUSE
/// overmounts). The host /proc, with every process of the host, would then
/// stay visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProcFallback {
    /// Keep the host /proc (the behaviour before this option existed): every
    /// host process is listed and its `cmdline` and `status` are readable.
    Host,
    /// Replace /proc with a read-only tmpfs that holds only a few static,
    /// container-scoped files (cpuinfo, meminfo, stat, loadavg, uptime,
    /// version) when they can be cloned. No pid directories, so no
    /// `/proc/self`: a tool that needs it fails instead of seeing the host.
    /// The view is built in a recursive copy of the sandbox's mount tree and
    /// entered with pivot_root, because AppArmor in LXC refuses a mount on
    /// `/proc` itself. When the mask cannot be built the run keeps the host
    /// /proc and the `proc` layer says so.
    #[default]
    Mask,
    /// Fail the run closed instead of running with the host /proc.
    Refuse,
}

/// What the command saw at `/proc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcView {
    /// A fresh proc of the sandbox's PID namespace.
    Private,
    /// A fresh proc with `subset=pid` (only the sandbox's pid directories).
    PidOnly,
    /// The host /proc behind a read-only tmpfs with `static_files` files.
    Masked { static_files: u32 },
    /// The host /proc (every host process visible).
    Host,
}

impl SandboxPolicy {
    /// Conservative default for a given workdir.
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        let p = |s: &str| PathBuf::from(s);
        let mut hide = vec![p("/root")];
        if let Some(h) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            hide.insert(0, PathBuf::from(h));
        }
        SandboxPolicy {
            workdir: workdir.into(),
            env_allowlist: vec!["PATH".into(), "LANG".into()],
            env_set: vec![],
            wall_timeout: Duration::from_secs(10),
            cpu_secs: 5,
            mem_bytes: 512 * 1024 * 1024,
            nofile: 64,
            fsize_bytes: 256 * 1024 * 1024,
            output_cap: OUTPUT_CAP,
            net: false,
            require_net_isolation: false,
            landlock: true,
            // /proc is needed by most tools (ps, /proc/self/*); with a PID
            // namespace and private /proc it only shows the sandbox.
            fs_read: [
                "/usr",
                "/bin",
                "/sbin",
                "/lib",
                "/lib64",
                "/etc",
                "/proc",
                "/dev/zero",
                "/dev/urandom",
                "/dev/random",
            ]
            .iter()
            .map(|s| p(s))
            .collect(),
            fs_write: vec![],
            namespaces: true,
            ro_root: true,
            hide_paths: hide,
            cgroup_parent: None,
            cgroup_pids_max: 64,
            cgroup_mem_bytes: 256 * 1024 * 1024,
            cgroup_cpu_pct: 0,
            workdir_tmpfs_bytes: 0,
            workdir_tmpfs_inodes: 0,
            proc_fallback: ProcFallback::Mask,
        }
    }

    /// Inode cap actually used for the work dir tmpfs.
    pub fn workdir_tmpfs_inode_cap(&self) -> u64 {
        if self.workdir_tmpfs_inodes > 0 {
            self.workdir_tmpfs_inodes
        } else {
            (self.workdir_tmpfs_bytes / (16 * 1024)).max(1024)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enforcement {
    Enforced,
    NotEnforced(String),
}

impl Enforcement {
    pub fn is_enforced(&self) -> bool {
        matches!(self, Enforcement::Enforced)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Exited(i32),
    Signaled(i32),
    /// Could not spawn or wait; see `Outcome::error`.
    Error,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub status: Status,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub wall: Duration,
    pub error: Option<String>,
    /// (layer, state) for every name in [`LAYERS`].
    pub enforced: Vec<(String, Enforcement)>,
    /// Counters of the per-run cgroup, when one was used.
    pub cgroup: Option<CgroupStats>,
    /// What the work dir tmpfs saw, when one was mounted.
    pub workdir_quota: Option<WorkdirQuota>,
    /// What the command saw at /proc.
    pub proc_view: ProcView,
}

/// Counters of the private work dir tmpfs (`workdir_tmpfs_bytes`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkdirQuota {
    /// tmpfs `size=` in bytes.
    pub limit_bytes: u64,
    /// tmpfs `nr_inodes=`.
    pub limit_inodes: u64,
    /// Bytes copied from the host work dir before the command ran.
    pub in_bytes: u64,
    /// Files, directories and symlinks copied in.
    pub in_entries: u64,
    /// Special files (FIFOs, sockets, devices) not copied in.
    pub in_skipped: u64,
    /// Bytes in use on the tmpfs when the command ended (statfs).
    pub used_bytes: u64,
    /// The copy back ran (false: the supervisor was killed, e.g. at the wall
    /// timeout, and nothing was copied back).
    pub copied_out: bool,
    /// The copy back finished without an error.
    pub out_complete: bool,
    /// Bytes written back to the host work dir.
    pub out_bytes: u64,
    /// Entries created or rewritten in the host work dir.
    pub out_entries: u64,
    /// Entries the host already had unchanged (same size and mtime).
    pub out_unchanged: u64,
    /// Entries not copied back (special files, host entry of another type).
    pub out_skipped: u64,
    /// errno of the copy failure (0 = none).
    pub errno: i32,
}

/// What the per-run cgroup saw, read just before it was removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CgroupStats {
    /// Path of the per-run cgroup that was created.
    pub path: PathBuf,
    /// `pids.events` max: forks refused because of `pids.max`.
    pub pids_max_hits: u64,
    /// `memory.events` oom_kill: processes killed by the cgroup OOM killer.
    pub oom_kills: u64,
    /// `memory.events` max: times usage hit `memory.max`.
    pub mem_max_hits: u64,
    /// `memory.peak` in bytes (0 when the kernel has no such file).
    pub mem_peak: u64,
    /// `pids.peak` (0 when the kernel has no such file).
    pub pids_peak: u64,
    /// The cgroup was empty afterwards and has been removed.
    pub removed: bool,
}

impl Outcome {
    pub fn layer(&self, name: &str) -> Option<&Enforcement> {
        self.enforced
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, e)| e)
    }
    pub fn enforced(&self, name: &str) -> bool {
        self.layer(name).is_some_and(Enforcement::is_enforced)
    }
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

// ---- syscall numbers (asm-generic numbering, same on x86_64 and aarch64) ----
const SYS_MOUNT_SETATTR: libc::c_long = 442;
// open_tree(2): same number on every Linux arch (new-style syscall table).
const SYS_OPEN_TREE: libc::c_long = 428;
const OPEN_TREE_CLONE: libc::c_uint = 1;
const SYS_LL_CREATE: libc::c_long = 444;
const SYS_LL_ADD: libc::c_long = 445;
const SYS_LL_RESTRICT: libc::c_long = 446;
const MOUNT_ATTR_RDONLY: u64 = 1;
const AT_RECURSIVE: libc::c_uint = 0x8000;

#[repr(C)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

// ---- kernel feature probes, each in a throwaway child ----

/// What this kernel/platform offers. Each risky syscall is tried in a forked
/// child first, so a seccomp filter (Android) that kills with SIGSYS only
/// kills that child.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Features {
    /// Landlock ABI version, 0 = unavailable or filtered.
    pub landlock_abi: i32,
    /// unshare(CLONE_NEWUSER) works for this process.
    pub userns: bool,
    /// unshare(CLONE_NEWUSER|CLONE_NEWNS|CLONE_NEWNET|CLONE_NEWPID) works.
    pub userns_full: bool,
    /// mount_setattr(2) exists (Linux >= 5.12) and is not filtered.
    pub mount_setattr: bool,
    /// capget/capset and PR_CAPBSET_* are usable (not filtered).
    pub caps: bool,
    /// Running on Android (Termux or adb shell).
    pub android: bool,
}

/// fork, run `f` in the child, `_exit` with its result. None = child died
/// from a signal (e.g. SIGSYS from seccomp) or fork failed.
fn probe_in_child(f: fn() -> i32) -> Option<i32> {
    // SAFETY: the child only calls `f` (raw syscalls, no allocation) and _exit.
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            return None;
        }
        if pid == 0 {
            libc::_exit(f().clamp(0, 125));
        }
        let mut st = 0;
        while libc::waitpid(pid, &mut st, 0) < 0 {
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return None;
            }
        }
        if libc::WIFEXITED(st) {
            Some(libc::WEXITSTATUS(st))
        } else {
            None
        }
    }
}

fn p_landlock() -> i32 {
    // SAFETY: flag-only query; null attr with size 0 is the documented form.
    let r = unsafe { libc::syscall(SYS_LL_CREATE, std::ptr::null::<u8>(), 0usize, 1u32) };
    if r < 0 {
        0
    } else {
        r.min(100) as i32
    }
}
fn p_userns() -> i32 {
    // SAFETY: affects only this throwaway child.
    (unsafe { libc::unshare(libc::CLONE_NEWUSER) } == 0) as i32
}
fn p_userns_full() -> i32 {
    let f = libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWNET | libc::CLONE_NEWPID;
    // SAFETY: affects only this throwaway child.
    (unsafe { libc::unshare(f) } == 0) as i32
}
fn p_mount_setattr() -> i32 {
    // Bad fd on purpose: EBADF/EINVAL = present, ENOSYS = absent.
    // SAFETY: invalid dirfd, null attr, size 0; the kernel rejects it.
    let r = unsafe {
        libc::syscall(
            SYS_MOUNT_SETATTR,
            -1,
            c"".as_ptr(),
            0u32,
            std::ptr::null::<u8>(),
            0usize,
        )
    };
    let e = std::io::Error::last_os_error().raw_os_error();
    (r < 0 && e != Some(libc::ENOSYS)) as i32
}

fn p_caps() -> i32 {
    let mut hdr = [0x2008_0522u32, 0];
    let mut data = [0u32; 6];
    // SAFETY: v3 header and two data structs (6 u32), valid pointers.
    let g = unsafe { libc::syscall(libc::SYS_capget, hdr.as_mut_ptr(), data.as_mut_ptr()) };
    // SAFETY: read-only query.
    let b = unsafe { libc::prctl(libc::PR_CAPBSET_READ, 0, 0, 0, 0) };
    (g == 0 && b >= 0) as i32
}

/// Probe once per process (cached).
pub fn features() -> Features {
    static F: OnceLock<Features> = OnceLock::new();
    *F.get_or_init(|| Features {
        landlock_abi: probe_in_child(p_landlock).unwrap_or(0),
        userns: probe_in_child(p_userns) == Some(1),
        userns_full: probe_in_child(p_userns_full) == Some(1),
        mount_setattr: probe_in_child(p_mount_setattr) == Some(1),
        caps: probe_in_child(p_caps) == Some(1),
        android: is_android(),
    })
}

/// True on Android (Termux, adb shell).
pub fn is_android() -> bool {
    cfg!(target_os = "android")
        || std::env::var_os("ANDROID_ROOT").is_some()
        || Path::new("/system/build.prop").exists()
}

/// Landlock ABI version of the running kernel, or None if unavailable.
pub fn landlock_abi() -> Option<i64> {
    match features().landlock_abi {
        0 => None,
        n => Some(n as i64),
    }
}

// ---- Landlock via raw syscalls ----
const LL_EXEC: u64 = 1;
const LL_WRITE_FILE: u64 = 1 << 1;
const LL_READ_FILE: u64 = 1 << 2;
const LL_READ_DIR: u64 = 1 << 3;
const LL_ABI1_ALL: u64 = 0x1FFF; // 13 ABI-1 rights
const LL_REFER: u64 = 1 << 13; // ABI 2
const LL_TRUNCATE: u64 = 1 << 14; // ABI 3
const LL_READ: u64 = LL_EXEC | LL_READ_FILE | LL_READ_DIR;
const LL_FILE_ONLY: u64 = LL_EXEC | LL_WRITE_FILE | LL_READ_FILE | LL_TRUNCATE;
const LL_NET_BIND_TCP: u64 = 1;
const LL_NET_CONNECT_TCP: u64 = 2;

#[repr(C)]
struct LlRulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
}
#[repr(C, packed)]
struct LlPathBeneath {
    allowed_access: u64,
    parent_fd: i32,
}

#[derive(Clone)]
struct Landlock {
    ruleset_fd: i32,
    path_fds: Vec<(i32, u64)>,
    net: bool,
    /// Rights to grant on a freshly mounted private /proc (0 = none).
    proc_acc: u64,
    /// Rights of the work dir, granted again on a work dir tmpfs.
    wd_acc: u64,
}

impl Landlock {
    /// Built in the parent (allocation and open() allowed here).
    fn prepare(policy: &SandboxPolicy, abi: i32) -> Result<Landlock, String> {
        let mut handled = LL_ABI1_ALL;
        if abi >= 2 {
            handled |= LL_REFER;
        }
        if abi >= 3 {
            handled |= LL_TRUNCATE;
        }
        let net = abi >= 4 && !policy.net;
        let attr = LlRulesetAttr {
            handled_access_fs: handled,
            handled_access_net: if net {
                LL_NET_BIND_TCP | LL_NET_CONNECT_TCP
            } else {
                0
            },
        };
        let size = if net {
            std::mem::size_of::<LlRulesetAttr>()
        } else {
            8 // ABI-1 struct: handled_access_fs only
        };
        // SAFETY: attr is a valid repr(C) struct at least `size` bytes long.
        let fd = unsafe { libc::syscall(SYS_LL_CREATE, &attr as *const _, size, 0u32) };
        if fd < 0 {
            return Err(format!(
                "create_ruleset: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut ll = Landlock {
            ruleset_fd: fd as i32,
            path_fds: vec![],
            net,
            proc_acc: if policy.fs_read.iter().any(|p| p == Path::new("/proc")) {
                LL_READ & handled
            } else {
                0
            },
            wd_acc: handled,
        };
        let mut roots: Vec<(PathBuf, u64)> = vec![];
        for r in &policy.fs_read {
            roots.push((r.clone(), LL_READ));
        }
        for w in &policy.fs_write {
            roots.push((w.clone(), handled));
        }
        roots.push((policy.workdir.clone(), handled));
        roots.push((
            PathBuf::from("/dev/null"),
            LL_READ_FILE | LL_WRITE_FILE | LL_TRUNCATE,
        ));
        for (path, acc) in roots {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue; // missing roots are skipped: they grant nothing
            };
            let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
                continue;
            };
            // Rules on a non-directory may only carry file rights.
            let acc = if meta.is_dir() {
                acc
            } else {
                acc & LL_FILE_ONLY
            } & handled;
            // SAFETY: c is a valid NUL-terminated string.
            let pfd = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if pfd >= 0 {
                ll.path_fds.push((pfd, acc));
            }
        }
        Ok(ll)
    }

    /// Async-signal-safe: syscalls only, no allocation. Runs after fork.
    /// `new_proc`: a private /proc was mounted after the rules were prepared;
    /// the old /proc rule points at the host proc, so add one for the new one.
    /// `wd_fd`: open fd of a work dir tmpfs mounted after the rules were
    /// prepared (-1 = none); it gets the work dir rights explicitly.
    fn apply(&self, new_proc: bool, wd_fd: i32) -> std::io::Result<()> {
        if wd_fd >= 0 {
            let rule = LlPathBeneath {
                allowed_access: self.wd_acc,
                parent_fd: wd_fd,
            };
            // SAFETY: valid packed struct; type 1 = PATH_BENEATH.
            let r = unsafe {
                libc::syscall(SYS_LL_ADD, self.ruleset_fd, 1u32, &rule as *const _, 0u32)
            };
            if r != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        if new_proc && self.proc_acc != 0 {
            // SAFETY: open of a constant path; the fd is passed to the kernel only.
            let pfd = unsafe { libc::open(c"/proc".as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if pfd >= 0 {
                let rule = LlPathBeneath {
                    allowed_access: self.proc_acc,
                    parent_fd: pfd,
                };
                // SAFETY: valid packed struct; type 1 = PATH_BENEATH.
                let r = unsafe {
                    libc::syscall(SYS_LL_ADD, self.ruleset_fd, 1u32, &rule as *const _, 0u32)
                };
                // SAFETY: closing the fd opened above.
                unsafe { libc::close(pfd) };
                if r != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        for (pfd, acc) in &self.path_fds {
            let rule = LlPathBeneath {
                allowed_access: *acc,
                parent_fd: *pfd,
            };
            // SAFETY: rule is a valid packed struct; type 1 = PATH_BENEATH.
            let r = unsafe {
                libc::syscall(SYS_LL_ADD, self.ruleset_fd, 1u32, &rule as *const _, 0u32)
            };
            if r != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        // SAFETY: plain syscall on our ruleset fd.
        let r = unsafe { libc::syscall(SYS_LL_RESTRICT, self.ruleset_fd, 0u32) };
        if r != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn close(&self) {
        // SAFETY: closing fds this struct opened.
        unsafe {
            libc::close(self.ruleset_fd);
            for (fd, _) in &self.path_fds {
                libc::close(*fd);
            }
        }
    }
}

#[cfg(all(any(target_os = "linux", target_os = "android"), target_env = "gnu"))]
type Res = libc::__rlimit_resource_t;
#[cfg(not(all(any(target_os = "linux", target_os = "android"), target_env = "gnu")))]
type Res = libc::c_int;

fn setrl(res: Res, v: u64) -> std::io::Result<()> {
    let l = libc::rlimit {
        rlim_cur: v as libc::rlim_t,
        rlim_max: v as libc::rlim_t,
    };
    // SAFETY: setrlimit with a valid pointer is async-signal-safe.
    if unsafe { libc::setrlimit(res, &l) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Write `data` to an existing file (proc maps). Async-signal-safe.
fn write_file(path: &std::ffi::CStr, data: &[u8]) -> bool {
    // SAFETY: open/write/close on a valid C string and buffer.
    unsafe {
        let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
        if fd < 0 {
            return false;
        }
        let ok = libc::write(fd, data.as_ptr().cast(), data.len()) == data.len() as isize;
        libc::close(fd);
        ok
    }
}

fn mount_setattr(path: &std::ffi::CStr, flags: libc::c_uint, set: u64, clr: u64) -> bool {
    let a = MountAttr {
        attr_set: set,
        attr_clr: clr,
        propagation: 0,
        userns_fd: 0,
    };
    // SAFETY: valid path and attr struct of the size passed.
    unsafe {
        libc::syscall(
            SYS_MOUNT_SETATTR,
            libc::AT_FDCWD,
            path.as_ptr(),
            flags,
            &a as *const MountAttr,
            std::mem::size_of::<MountAttr>(),
        ) == 0
    }
}

/// Drop every capability: empty bounding set, then zero eff/perm/inh.
/// Returns true when the process ends up holding no capabilities and cannot
/// regain any through exec. Async-signal-safe.
fn drop_caps(may_hold_caps: bool) -> bool {
    #[repr(C)]
    struct Hdr {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Data {
        eff: u32,
        perm: u32,
        inh: u32,
    }
    const V3: u32 = 0x2008_0522;
    // SAFETY: prctl/capget/capset with valid pointers to local structs.
    unsafe {
        let mut bset_empty = true;
        for cap in 0..64 {
            if libc::prctl(libc::PR_CAPBSET_READ, cap, 0, 0, 0) == 1
                && libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0) != 0
            {
                bset_empty = false;
            }
        }
        // ambient set: clear all
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        );
        let mut h = Hdr {
            version: V3,
            pid: 0,
        };
        let zero = [Data {
            eff: 0,
            perm: 0,
            inh: 0,
        }; 2];
        libc::syscall(libc::SYS_capset, &mut h as *mut Hdr, zero.as_ptr());
        let mut now = [Data {
            eff: 1,
            perm: 1,
            inh: 1,
        }; 2];
        let mut h = Hdr {
            version: V3,
            pid: 0,
        };
        if libc::syscall(libc::SYS_capget, &mut h as *mut Hdr, now.as_mut_ptr()) != 0 {
            return false;
        }
        let none_held = now.iter().all(|d| d.eff == 0 && d.perm == 0 && d.inh == 0);
        // A process whose (namespace) uid is 0 regains its bounding set on
        // exec, so for it the bounding set must be empty too. Everyone else
        // gains nothing on exec under no_new_privs.
        none_held && (bset_empty || !may_hold_caps)
    }
}

fn capped_reader<R: Read + Send + 'static>(
    mut r: R,
    cap: usize,
) -> thread::JoinHandle<(Vec<u8>, bool)> {
    thread::spawn(move || {
        let mut keep = Vec::new();
        let mut truncated = false;
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // keep draining after the cap so the child never blocks
                    let room = cap.saturating_sub(keep.len());
                    keep.extend_from_slice(&buf[..n.min(room)]);
                    truncated |= n > room;
                }
            }
        }
        (keep, truncated)
    })
}

fn fail(msg: String, start: Instant, enforced: Vec<(String, Enforcement)>) -> Outcome {
    Outcome {
        status: Status::Error,
        stdout: vec![],
        stderr: vec![],
        stdout_truncated: false,
        stderr_truncated: false,
        timed_out: false,
        wall: start.elapsed(),
        error: Some(msg),
        enforced,
        cgroup: None,
        workdir_quota: None,
        proc_view: ProcView::Host,
    }
}

// Status bits written by the process that execs, right before exec.
const B_RLIMIT: u32 = 1;
const B_NNP: u32 = 1 << 1;
const B_USERNS: u32 = 1 << 2;
const B_NETNS: u32 = 1 << 3;
const B_PIDNS: u32 = 1 << 4;
const B_MNTNS: u32 = 1 << 5;
const B_RO_ROOT: u32 = 1 << 6;
const B_HIDE: u32 = 1 << 7;
const B_PROC: u32 = 1 << 8;
const B_LANDLOCK: u32 = 1 << 9;
const B_LL_NET: u32 = 1 << 10;
const B_CAPS: u32 = 1 << 11;
const B_UIDMAP: u32 = 1 << 12;
const B_FDS: u32 = 1 << 13;
const B_CGROUP: u32 = 1 << 14;
const B_WDQ: u32 = 1 << 15;
// A proc mount with `subset=pid` (Linux >= 7.2 allows it in a rootless
// container whose /proc is overmounted).
const B_PROC_PIDONLY: u32 = 1 << 16;
// Host /proc hidden behind a read-only tmpfs (proc_fallback = Mask).
const B_PROC_MASK: u32 = 1 << 17;
// Bits 18..23: errno (clamped to 1..63) of a /proc mask that failed cleanly.
const PROC_MASK_ERR_SHIFT: u32 = 18;
// Bits 24..31: how many static files the /proc mask kept.
const PROC_MASK_SHIFT: u32 = 24;
// Private errno values the pre-exec code hands to `spawn` to fail closed with
// a precise message (execve itself never returns these two).
const E_UIDMAP: i32 = libc::ENOTRECOVERABLE;
const E_CGROUP: i32 = libc::ESTALE;
const E_WDQ: i32 = libc::ENOTUNIQ;
const E_PROC: i32 = libc::ENOMEDIUM;
// Work dir tmpfs report, written by the pre-exec child (copy in failed) or by
// the PID-namespace supervisor (after the copy back): 11 x u64, one write.
const WDQ_MAGIC: u64 = 0x5744_5131_3531_0001;
const WDQ_IN_FAILED: u64 = 1;
const WDQ_OUT_DONE: u64 = 2;
const WDQ_WORDS: usize = 11;

/// Async-signal-safe: one write of the fixed report.
fn wdq_report(
    fd: i32,
    phase: u64,
    errno: i32,
    i: &treecopy::Stats,
    used: u64,
    o: &treecopy::Stats,
) {
    if fd < 0 {
        return;
    }
    let w: [u64; WDQ_WORDS] = [
        WDQ_MAGIC,
        phase,
        errno as u64,
        i.bytes,
        i.entries,
        i.skipped,
        used,
        o.bytes,
        o.entries,
        o.unchanged,
        o.skipped,
    ];
    // SAFETY: writing a stack array of the size passed (< PIPE_BUF: atomic).
    unsafe { libc::write(fd, w.as_ptr().cast(), std::mem::size_of_val(&w)) };
}

/// Read the report left in the pipe (non-blocking; None when there is none).
fn wdq_read(fd: i32) -> Option<[u64; WDQ_WORDS]> {
    let mut w = [0u64; WDQ_WORDS];
    // SAFETY: fcntl/read on our own pipe end into a stack array.
    let n = unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        libc::read(fd, w.as_mut_ptr().cast(), std::mem::size_of_val(&w))
    };
    (n == std::mem::size_of_val(&w) as isize && w[0] == WDQ_MAGIC).then_some(w)
}
const SYS_CLOSE_RANGE: libc::c_long = 436;
const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;

fn cstr(p: &Path) -> Option<CString> {
    CString::new(p.as_os_str().as_bytes()).ok()
}

fn resolve_program(prog: &str, env_path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if prog.contains('/') {
        return std::fs::canonicalize(prog).ok();
    }
    std::env::split_paths(env_path?)
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}

// move_mount(2): same number on every Linux arch (new-style syscall table).
const SYS_MOVE_MOUNT: libc::c_long = 429;
const MOVE_MOUNT_F_EMPTY_PATH: libc::c_uint = 0x4;

/// Static /proc files the mask keeps (container-scoped under lxcfs).
const PROC_MASK_STATIC: [&str; 6] = ["cpuinfo", "meminfo", "stat", "loadavg", "uptime", "version"];

/// Paths for the /proc mask, built before the fork (no allocation after it).
struct ProcMaskPaths {
    /// The work dir: a scratch tmpfs goes over it for the copy of the tree.
    stage: CString,
    /// `<stage>/r`: the recursive copy of `/` (the new root).
    root: CString,
    /// `<stage>/r<stage>`: the scratch tmpfs as seen inside the copy.
    inner: CString,
    /// `<stage>/r/proc`: where the mask tmpfs goes.
    proc_dir: CString,
    /// (host /proc file, its place in the mask).
    statics: Vec<(CString, CString)>,
}

impl ProcMaskPaths {
    fn new(workdir: &Path) -> Option<Self> {
        if workdir == Path::new("/") || !workdir.is_absolute() {
            return None;
        }
        let root = workdir.join("r");
        let mut inner = root.clone().into_os_string();
        inner.push(workdir.as_os_str());
        let proc_dir = root.join("proc");
        let statics = PROC_MASK_STATIC
            .iter()
            .filter_map(|n| Some((cstr(&Path::new("/proc").join(n))?, cstr(&proc_dir.join(n))?)))
            .collect();
        Some(ProcMaskPaths {
            stage: cstr(workdir)?,
            root: cstr(&root)?,
            inner: cstr(Path::new(&inner))?,
            proc_dir: cstr(&proc_dir)?,
            statics,
        })
    }
}

/// Hide the host /proc from this process (PID 1 of the sandbox, still holding
/// all capabilities of its user namespace) when no fresh proc can be mounted.
///
/// A mount on `/proc` itself is refused in an LXC container (AppArmor), but a
/// mount on the `proc` directory of a copy of the tree is not. So: a scratch
/// tmpfs over the work dir, a recursive bind of `/` into it, the scratch
/// tmpfs dropped from the copy (the copy then shows the real work dir), a
/// read-only tmpfs over the copy's `/proc` with a few cloned static files,
/// then `pivot_root` into the copy and the old root detached.
///
/// Ok(n): mask in place with n static files. Err((errno, clean)): not in
/// place; `clean` = every step was undone and the host view is unchanged.
///
/// # Safety
/// Only async-signal-safe raw syscalls on data prepared before the fork.
unsafe fn proc_mask(m: &ProcMaskPaths) -> Result<u32, (i32, bool)> {
    let errno = || {
        std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO)
    };
    let fl = libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC;
    if libc::mount(
        c"none".as_ptr(),
        m.stage.as_ptr(),
        c"tmpfs".as_ptr(),
        fl,
        c"size=16k,nr_inodes=16,mode=700".as_ptr().cast(),
    ) != 0
    {
        return Err((errno(), true));
    }
    let undo = |e: i32, rec: bool| -> Result<u32, (i32, bool)> {
        let mut clean = true;
        if rec {
            clean &= libc::umount2(m.root.as_ptr(), libc::MNT_DETACH) == 0;
        }
        clean &= libc::umount2(m.stage.as_ptr(), libc::MNT_DETACH) == 0;
        Err((e, clean))
    };
    if libc::mkdir(m.root.as_ptr(), 0o700) != 0 {
        return undo(errno(), false);
    }
    if libc::mount(
        c"/".as_ptr(),
        m.root.as_ptr(),
        std::ptr::null(),
        libc::MS_BIND | libc::MS_REC,
        std::ptr::null(),
    ) != 0
    {
        return undo(errno(), false);
    }
    // The copy holds the scratch tmpfs too; drop it there so the new root
    // shows the work dir (and its own mounts) underneath.
    if libc::umount2(m.inner.as_ptr(), libc::MNT_DETACH) != 0 {
        return undo(errno(), true);
    }
    // Clone the static files while the host /proc is still reachable.
    let mut fds = [-1i32; PROC_MASK_STATIC.len()];
    for (i, (src, _)) in m.statics.iter().enumerate() {
        fds[i] = libc::syscall(
            SYS_OPEN_TREE,
            libc::AT_FDCWD,
            src.as_ptr(),
            OPEN_TREE_CLONE | libc::O_CLOEXEC as libc::c_uint,
        ) as i32;
    }
    let close_fds = |fds: &[i32]| {
        for &f in fds {
            if f >= 0 {
                libc::close(f);
            }
        }
    };
    if libc::mount(
        c"none".as_ptr(),
        m.proc_dir.as_ptr(),
        c"tmpfs".as_ptr(),
        fl,
        c"size=16k,nr_inodes=32,mode=755".as_ptr().cast(),
    ) != 0
    {
        let e = errno();
        close_fds(&fds);
        return undo(e, true);
    }
    let mut kept = 0u32;
    for (i, (_, dst)) in m.statics.iter().enumerate() {
        if fds[i] < 0 {
            continue;
        }
        let f = libc::open(
            dst.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_CLOEXEC,
            0o444,
        );
        if f < 0 {
            continue;
        }
        libc::close(f);
        if libc::syscall(
            SYS_MOVE_MOUNT,
            fds[i],
            c"".as_ptr(),
            libc::AT_FDCWD,
            dst.as_ptr(),
            MOVE_MOUNT_F_EMPTY_PATH,
        ) == 0
        {
            kept += 1;
        } else {
            libc::unlink(dst.as_ptr());
        }
    }
    close_fds(&fds);
    if libc::mount(
        std::ptr::null(),
        m.proc_dir.as_ptr(),
        std::ptr::null(),
        libc::MS_REMOUNT | libc::MS_BIND | libc::MS_RDONLY | fl,
        std::ptr::null(),
    ) != 0
    {
        return undo(errno(), true);
    }
    if libc::chdir(m.root.as_ptr()) != 0 {
        return undo(errno(), true);
    }
    if libc::syscall(libc::SYS_pivot_root, c".".as_ptr(), c".".as_ptr()) != 0 {
        let e = errno();
        libc::chdir(c"/".as_ptr());
        return undo(e, true);
    }
    // The old root now sits on top of the new one; detach it. From here on
    // there is no way back: a failure fails the run closed.
    if libc::umount2(c".".as_ptr(), libc::MNT_DETACH) != 0 {
        return Err((errno(), false));
    }
    if libc::chdir(c"/".as_ptr()) != 0 {
        return Err((errno(), false));
    }
    Ok(kept)
}

/// Run `cmd` (program + args) under `policy`. Never panics on bad input;
/// failures come back as `Status::Error` with `error` set.
pub fn run(cmd: &[String], policy: &SandboxPolicy) -> Outcome {
    let start = Instant::now();
    let ne = |s: &str| Enforcement::NotEnforced(s.to_string());
    let mut enforced: Vec<(String, Enforcement)> = LAYERS
        .iter()
        .map(|l| (l.to_string(), ne("not started")))
        .collect();
    let set = |e: &mut Vec<(String, Enforcement)>, k: &str, v: Enforcement| {
        if let Some(s) = e.iter_mut().find(|(n, _)| n == k) {
            s.1 = v;
        }
    };
    let Some((prog, args)) = cmd.split_first() else {
        return fail("empty command".into(), start, enforced);
    };
    if !policy.workdir.is_dir() {
        return fail(
            format!("workdir is not a directory: {}", policy.workdir.display()),
            start,
            enforced,
        );
    }
    let workdir = policy
        .workdir
        .canonicalize()
        .unwrap_or_else(|_| policy.workdir.clone());
    let feat = features();

    // Landlock: only when wanted and the kernel has it.
    let ll = if !policy.landlock {
        set(&mut enforced, "landlock", ne("disabled by policy"));
        None
    } else if feat.landlock_abi == 0 {
        set(&mut enforced, "landlock", ne("kernel has no landlock"));
        None
    } else {
        let mut p2 = policy.clone();
        p2.workdir = workdir.clone();
        match Landlock::prepare(&p2, feat.landlock_abi) {
            Ok(l) => Some(l),
            Err(e) => {
                set(&mut enforced, "landlock", Enforcement::NotEnforced(e));
                None
            }
        }
    };

    // ---- everything the forked child needs, prepared here ----
    // SAFETY: plain id getters.
    let (uid, gid, euid) = unsafe { (libc::getuid(), libc::getgid(), libc::geteuid()) };
    let uid_map = format!("{uid} {uid} 1");
    let gid_map = format!("{gid} {gid} 1");
    let want_ns = policy.namespaces && feat.userns;
    let mut full = libc::CLONE_NEWUSER | libc::CLONE_NEWPID;
    if !policy.net {
        full |= libc::CLONE_NEWNET;
    }
    let wdq = policy.workdir_tmpfs_bytes;
    let wdq_inodes = policy.workdir_tmpfs_inode_cap();
    if policy.ro_root || !policy.hide_paths.is_empty() || wdq > 0 {
        full |= libc::CLONE_NEWNS;
    }
    // tmpfs options and the copy buffer, prepared before the fork.
    let wdq_opts =
        CString::new(format!("size={wdq},nr_inodes={wdq_inodes},mode=700")).unwrap_or_default();
    let mut wdq_buf: Vec<u8> = if wdq > 0 {
        vec![0u8; treecopy::BUF]
    } else {
        vec![]
    };
    let mut attempts: [libc::c_int; 5] = [0; 5];
    if want_ns {
        attempts[0] = full;
        attempts[1] = full & !libc::CLONE_NEWNS;
        attempts[2] = full & !(libc::CLONE_NEWNS | libc::CLONE_NEWPID);
        attempts[3] = libc::CLONE_NEWUSER;
    }
    if policy.namespaces && euid == 0 && !policy.net {
        // root without user namespaces: plain netns still works
        attempts[4] = libc::CLONE_NEWNET;
    }
    // Writable binds (workdir + fs_write) and paths to hide.
    let prog_path = resolve_program(
        prog,
        std::env::var_os("PATH")
            .as_deref()
            .filter(|_| !prog.contains('/')),
    );
    let mut binds: Vec<CString> = vec![];
    for w in std::iter::once(&workdir).chain(policy.fs_write.iter()) {
        if let Some(c) = w.canonicalize().ok().as_deref().and_then(cstr) {
            binds.push(c);
        }
    }
    let mut hide: Vec<CString> = vec![];
    let mut hide_skipped: Vec<String> = vec![];
    for h in &policy.hide_paths {
        let Ok(hc) = h.canonicalize() else { continue };
        if !hc.is_dir() || hc == Path::new("/") {
            continue;
        }
        // Needed if the work dir, a root or the program lies inside it; a
        // read root that *contains* the hide path (e.g. /) is fine.
        let needed = std::iter::once(&workdir)
            .chain(policy.fs_write.iter())
            .chain(prog_path.iter())
            .chain(policy.fs_read.iter())
            .any(|p| {
                p.canonicalize()
                    .unwrap_or_else(|_| p.clone())
                    .starts_with(&hc)
            });
        if needed {
            hide_skipped.push(hc.display().to_string());
        } else if let Some(c) = cstr(&hc) {
            hide.push(c);
        }
    }
    let can_ro = policy.ro_root && feat.mount_setattr;
    // Highest fd number to close in the PID-namespace supervisor.
    let max_fd: i32 = {
        let mut l = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: valid pointer.
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) };
        // rlim_t is not u64 on every target
        let cur: u64 = l.rlim_cur as _;
        cur.clamp(1024, 65536) as i32
    };
    // Without close_range(2) the fallback loop only covers fds < max_fd.
    let fd_loop_complete = {
        let mut l = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: valid pointer.
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut l) };
        let cur: u64 = l.rlim_cur as _;
        cur <= max_fd as u64
    };

    // Per-run cgroup: created before the fork, joined by the child first.
    let cg = match &policy.cgroup_parent {
        None => None,
        Some(parent) => {
            match cgroup::RunCgroup::create(
                parent,
                policy.cgroup_pids_max,
                policy.cgroup_mem_bytes,
                policy.cgroup_cpu_pct,
            ) {
                Ok(c) => Some(c),
                Err(e) => {
                    if let Some(l) = &ll {
                        l.close();
                    }
                    return fail(format!("cgroup: {e}"), start, enforced);
                }
            }
        }
    };
    let cg_procs: Option<CString> = cg.as_ref().map(|c| c.procs.clone());

    // Status pipe: the exec'ing process writes 4 bytes of flags before exec.
    let mut fds = [0i32; 2];
    // SAFETY: valid array of two fds.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        if let Some(l) = &ll {
            l.close();
        }
        if let Some(c) = &cg {
            c.finish();
        }
        return fail("pipe2 failed".into(), start, enforced);
    }
    let (rfd, wfd) = (fds[0], fds[1]);
    // Work dir tmpfs report pipe (only when a tmpfs is wanted).
    let (qr, qw) = if wdq > 0 {
        let mut q = [0i32; 2];
        // SAFETY: valid array of two fds.
        if unsafe { libc::pipe2(q.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            // SAFETY: closing the status pipe created above.
            unsafe {
                libc::close(rfd);
                libc::close(wfd);
            }
            if let Some(l) = &ll {
                l.close();
            }
            if let Some(c) = &cg {
                c.finish();
            }
            return fail("pipe2 failed".into(), start, enforced);
        }
        (q[0], q[1])
    } else {
        (-1, -1)
    };

    let mut c = Command::new(prog);
    c.args(args)
        .env_clear()
        .current_dir(&workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for k in &policy.env_allowlist {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    for (k, v) in &policy.env_set {
        c.env(k, v);
    }
    let (cpu, mem, nofile, fsize) = (
        policy.cpu_secs,
        policy.mem_bytes,
        policy.nofile,
        policy.fsize_bytes,
    );
    let require_net = policy.require_net_isolation && !policy.net;
    let llown = ll.clone();
    let caps_ok = feat.caps;
    // std chdirs before pre_exec; re-enter the workdir after mounts so the
    // cwd is the writable bind, not the (now read-only) mount below it.
    let workdir_c = cstr(&workdir).unwrap_or_default();
    let hide_count = hide.len();
    let proc_fb = policy.proc_fallback;
    let proc_mask_paths = if proc_fb == ProcFallback::Mask {
        ProcMaskPaths::new(&workdir)
    } else {
        None
    };
    // SAFETY: the closure runs between fork and exec and only calls
    // async-signal-safe functions (setsid, setrlimit, prctl, unshare, mount,
    // fork, waitpid, write, close, raw syscalls) on data prepared before the
    // fork. It allocates nothing; errors are errno wrappers.
    unsafe {
        c.pre_exec(move || {
            let mut bits: u32 = 0;
            // Work dir tmpfs: host dir fd, tmpfs root fd, copy-in counters.
            let (mut wd_host, mut wd_tmp) = (-1i32, -1i32);
            let mut wd_in = treecopy::Stats::default();
            // Join the per-run cgroup first, so every process created below
            // (the PID-namespace supervisor, PID 1, its children) is inside.
            if let Some(p) = &cg_procs {
                if !write_file(p, b"0") {
                    return Err(std::io::Error::from_raw_os_error(E_CGROUP));
                }
                bits |= B_CGROUP;
            }
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Leaked descriptors: whatever the (multi-threaded) host process
            // had open without O_CLOEXEC at fork time must not reach the
            // command. Mark all of them close-on-exec; std's own exec-error
            // pipe and our status pipe are close-on-exec already.
            if libc::syscall(SYS_CLOSE_RANGE, 3u32, u32::MAX, CLOSE_RANGE_CLOEXEC) == 0 {
                bits |= B_FDS;
            } else {
                for fd in 3..max_fd {
                    let fl = libc::fcntl(fd, libc::F_GETFD);
                    if fl >= 0 {
                        libc::fcntl(fd, libc::F_SETFD, fl | libc::FD_CLOEXEC);
                    }
                }
                if fd_loop_complete {
                    bits |= B_FDS;
                }
            }
            // RLIMIT_NOFILE is set last (below): the setup opens files
            // (uid_map, gid_map, /proc for Landlock) while the fd table
            // inherited from a busy multi-threaded parent can already hold
            // more descriptors than the limit, which made those opens fail
            // with EMFILE (the old 1-in-5 "/proc: Permission denied" flake).
            setrl(libc::RLIMIT_CPU, cpu)?;
            setrl(libc::RLIMIT_AS, mem)?;
            setrl(libc::RLIMIT_CORE, 0)?;
            // RLIMIT_FSIZE is set after the mounts: copying the host work dir
            // into its tmpfs may write input files larger than the limit.
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            bits |= B_NNP;

            // Namespaces, best effort, biggest set first.
            let mut got: libc::c_int = 0;
            for f in attempts {
                if f != 0 && libc::unshare(f) == 0 {
                    got = f;
                    break;
                }
            }
            if got & libc::CLONE_NEWUSER != 0 {
                bits |= B_USERNS;
                // setgroups only exists on Linux >= 3.19; absent is fine.
                let a = write_file(c"/proc/self/setgroups", b"deny")
                    || libc::access(c"/proc/self/setgroups".as_ptr(), libc::F_OK) != 0;
                let b = write_file(c"/proc/self/uid_map", uid_map.as_bytes());
                let g = write_file(c"/proc/self/gid_map", gid_map.as_bytes());
                if !(a && b && g) {
                    // Unmapped, the command would run as the overflow uid
                    // with an unreadable /proc/self: refuse instead.
                    return Err(std::io::Error::from_raw_os_error(E_UIDMAP));
                }
                bits |= B_UIDMAP;
            }
            if got & libc::CLONE_NEWNET != 0 {
                bits |= B_NETNS;
            }
            if got & libc::CLONE_NEWNS != 0
                && libc::mount(
                    std::ptr::null(),
                    c"/".as_ptr(),
                    std::ptr::null(),
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                ) == 0
            {
                bits |= B_MNTNS;
                // Hide home dirs behind an empty, read-only tmpfs.
                let mut hid = !hide.is_empty();
                for h in &hide {
                    if libc::mount(
                        c"none".as_ptr(),
                        h.as_ptr(),
                        c"tmpfs".as_ptr(),
                        libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                        c"size=4k,mode=555".as_ptr().cast(),
                    ) != 0
                    {
                        hid = false;
                    }
                }
                if hid {
                    bits |= B_HIDE;
                }
                // Work dir size cap: a private tmpfs over the work dir. Only
                // with a PID namespace, whose supervisor copies the result
                // back; the host dir stays reachable through `wd_host` only.
                if wdq > 0 && got & libc::CLONE_NEWPID != 0 {
                    // The host dir fd the supervisor copies back through.
                    // With ro_root the whole tree goes read-only below, and
                    // the host dir mount ends up under the tmpfs, so the
                    // re-enabled write bind never reaches it (EROFS on copy
                    // back, VPS run 08:03). A detached clone of its mount
                    // (open_tree, same kernel family as mount_setattr) is
                    // outside the tree: it keeps its own rw flag and is
                    // reachable only through this fd.
                    let host = if can_ro {
                        let t = libc::syscall(
                            SYS_OPEN_TREE,
                            libc::AT_FDCWD,
                            workdir_c.as_ptr(),
                            OPEN_TREE_CLONE | AT_RECURSIVE | libc::O_CLOEXEC as libc::c_uint,
                        ) as i32;
                        if t < 0 {
                            -1
                        } else {
                            let d = libc::openat(
                                t,
                                c".".as_ptr(),
                                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                            );
                            libc::close(t);
                            d
                        }
                    } else {
                        libc::open(
                            workdir_c.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                        )
                    };
                    if host < 0 {
                        let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                        let z = treecopy::Stats::default();
                        wdq_report(qw, WDQ_IN_FAILED, e, &z, 0, &z);
                        return Err(std::io::Error::from_raw_os_error(E_WDQ));
                    }
                    if libc::mount(
                        c"none".as_ptr(),
                        workdir_c.as_ptr(),
                        c"tmpfs".as_ptr(),
                        libc::MS_NOSUID | libc::MS_NODEV,
                        wdq_opts.as_ptr().cast(),
                    ) == 0
                    {
                        let tmp = libc::open(
                            workdir_c.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                        );
                        let mut w = treecopy::Walk::new(
                            treecopy::Mode::In,
                            u64::MAX,
                            u64::MAX,
                            &mut wdq_buf[..],
                        );
                        let r = if tmp < 0 {
                            Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
                        } else {
                            w.copy_dir(host, tmp, 0)
                        };
                        if let Err(e) = r {
                            let z = treecopy::Stats::default();
                            wdq_report(qw, WDQ_IN_FAILED, e, &w.stats, 0, &z);
                            return Err(std::io::Error::from_raw_os_error(E_WDQ));
                        }
                        wd_in = w.stats;
                        wd_host = host;
                        wd_tmp = tmp;
                        bits |= B_WDQ;
                    } else {
                        libc::close(host);
                    }
                }
                if can_ro {
                    let mut ok = true;
                    for b in &binds {
                        ok &= libc::mount(
                            b.as_ptr(),
                            b.as_ptr(),
                            std::ptr::null(),
                            libc::MS_BIND | libc::MS_REC,
                            std::ptr::null(),
                        ) == 0;
                    }
                    if ok && mount_setattr(c"/", AT_RECURSIVE, MOUNT_ATTR_RDONLY, 0) {
                        for b in &binds {
                            if !mount_setattr(b, AT_RECURSIVE, 0, MOUNT_ATTR_RDONLY) {
                                // fail closed: the work dir must stay writable
                                return Err(std::io::Error::from_raw_os_error(libc::EROFS));
                            }
                        }
                        bits |= B_RO_ROOT;
                    }
                }
            }

            if bits & B_MNTNS != 0 && libc::chdir(workdir_c.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if fsize > 0 {
                setrl(libc::RLIMIT_FSIZE, fsize)?;
            }

            if got & libc::CLONE_NEWPID != 0 {
                let child = libc::fork();
                if child < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if child > 0 {
                    // Supervisor outside the PID namespace: drop every fd but
                    // stdio (incl. std's exec-error pipe and our status pipe),
                    // wait, and pass on the exit status or signal.
                    for fd in 3..max_fd {
                        if fd != wd_host && fd != wd_tmp && fd != qw {
                            libc::close(fd);
                        }
                    }
                    let mut st = 0;
                    loop {
                        if libc::waitpid(child, &mut st, 0) == child {
                            break;
                        }
                        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                            libc::_exit(127);
                        }
                    }
                    if bits & B_WDQ != 0 {
                        // PID 1 is gone, so is every process of its namespace:
                        // nothing writes the tmpfs any more. Copy back within
                        // the tmpfs size, never more (sparse files included).
                        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                        let mut sv: libc::statvfs = std::mem::zeroed();
                        let used = if libc::fstatvfs(wd_tmp, &mut sv) == 0 {
                            (sv.f_blocks as u64).saturating_sub(sv.f_bfree as u64)
                                * sv.f_frsize as u64
                        } else {
                            0
                        };
                        let mut w = treecopy::Walk::new(
                            treecopy::Mode::Out,
                            wdq,
                            wdq_inodes,
                            &mut wdq_buf[..],
                        );
                        let e = match w.copy_dir(wd_tmp, wd_host, 0) {
                            Ok(()) => 0,
                            Err(e) => e,
                        };
                        wdq_report(qw, WDQ_OUT_DONE, e, &wd_in, used, &w.stats);
                    }
                    if libc::WIFEXITED(st) {
                        libc::_exit(libc::WEXITSTATUS(st));
                    }
                    let sig = libc::WTERMSIG(st);
                    libc::signal(sig, libc::SIG_DFL);
                    let mut set: libc::sigset_t = std::mem::zeroed();
                    libc::sigemptyset(&mut set);
                    libc::sigaddset(&mut set, sig);
                    libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
                    libc::kill(libc::getpid(), sig);
                    libc::_exit(128 + sig);
                }
                // PID 1 of the new namespace.
                bits |= B_PIDNS;
                if bits & B_MNTNS != 0 {
                    let pfl = libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC;
                    if libc::mount(
                        c"proc".as_ptr(),
                        c"/proc".as_ptr(),
                        c"proc".as_ptr(),
                        pfl,
                        std::ptr::null(),
                    ) == 0
                    {
                        bits |= B_PROC;
                    } else if libc::mount(
                        c"proc".as_ptr(),
                        c"/proc".as_ptr(),
                        c"proc".as_ptr(),
                        pfl,
                        c"subset=pid".as_ptr().cast(),
                    ) == 0
                    {
                        // Linux >= 7.2: only pid dirs, allowed under overmounts.
                        bits |= B_PROC_PIDONLY;
                    } else if let Some(m) = &proc_mask_paths {
                        match proc_mask(m) {
                            Ok(n) => {
                                bits |= B_PROC_MASK | (n.min(255) << PROC_MASK_SHIFT);
                                if libc::chdir(workdir_c.as_ptr()) != 0 {
                                    return Err(std::io::Error::last_os_error());
                                }
                            }
                            // Not undone: the view is unknown, fail closed.
                            Err((_, false)) => {
                                return Err(std::io::Error::from_raw_os_error(E_PROC))
                            }
                            Err((e, true)) => {
                                bits |= (e.clamp(1, 63) as u32) << PROC_MASK_ERR_SHIFT;
                            }
                        }
                    }
                }
            }
            if proc_fb == ProcFallback::Refuse
                && bits & (B_PROC | B_PROC_PIDONLY | B_PROC_MASK) == 0
            {
                return Err(std::io::Error::from_raw_os_error(E_PROC));
            }

            if let Some(l) = &llown {
                l.apply(bits & (B_PROC | B_PROC_PIDONLY | B_PROC_MASK) != 0, wd_tmp)?;
                bits |= B_LANDLOCK;
                if l.net {
                    bits |= B_LL_NET;
                }
            }
            let may_hold_caps = euid == 0 || bits & B_USERNS != 0;
            if caps_ok && drop_caps(may_hold_caps) {
                bits |= B_CAPS;
            }
            setrl(libc::RLIMIT_NOFILE, nofile)?;
            bits |= B_RLIMIT;
            let b = bits.to_le_bytes();
            libc::write(wfd, b.as_ptr().cast(), 4);
            if require_net && bits & (B_NETNS | B_LL_NET) == 0 {
                return Err(std::io::Error::from_raw_os_error(libc::EPERM));
            }
            Ok(())
        });
    }
    let spawned = c.spawn();
    if let Some(l) = &ll {
        l.close();
    }
    let cg_fail = |cg: &Option<cgroup::RunCgroup>| {
        if let Some(c) = cg {
            c.finish();
        }
    };
    // SAFETY: closing our copy of the write end, then reading at most 4 bytes.
    let mut b = [0u8; 4];
    let got_bits = unsafe {
        libc::close(wfd);
        let n = libc::read(rfd, b.as_mut_ptr().cast(), 4);
        libc::close(rfd);
        n == 4
    };
    let bits = if got_bits { u32::from_le_bytes(b) } else { 0 };
    if qw >= 0 {
        // SAFETY: our copy of the report pipe's write end.
        unsafe { libc::close(qw) };
    }
    let close_qr = || {
        if qr >= 0 {
            // SAFETY: our read end of the report pipe, closed once.
            unsafe { libc::close(qr) };
        }
    };
    let mut child = match spawned {
        Ok(ch) => ch,
        Err(e) => {
            let msg = if require_net && e.raw_os_error() == Some(libc::EPERM) {
                "network isolation required but unavailable (no netns, no landlock TCP deny)"
                    .to_string()
            } else if e.raw_os_error() == Some(libc::EROFS) {
                "read-only root applied but the work dir could not be kept writable".to_string()
            } else if e.raw_os_error() == Some(E_UIDMAP) {
                "user namespace entered but the uid/gid map could not be written; refusing to run as the overflow uid".to_string()
            } else if e.raw_os_error() == Some(E_PROC) {
                if proc_fb == ProcFallback::Refuse {
                    "private /proc required (proc_fallback = Refuse) but the kernel refused a proc mount; refusing to run with the host /proc".to_string()
                } else {
                    "/proc mask: pivot_root done but the old root could not be detached; refusing to run".to_string()
                }
            } else if e.raw_os_error() == Some(E_CGROUP) {
                "could not join the per-run cgroup (the caller must live inside the same delegated subtree)".to_string()
            } else if e.raw_os_error() == Some(E_WDQ) {
                let r = wdq_read(qr).unwrap_or_default();
                format!(
                    "work dir tmpfs: copying the work dir in failed ({}; {} bytes, {} entries copied); a work dir larger than workdir_tmpfs_bytes={wdq} / nr_inodes={wdq_inodes} cannot run under this cap",
                    std::io::Error::from_raw_os_error(r[2] as i32),
                    r[3],
                    r[4]
                )
            } else {
                format!("spawn {prog}: {e}")
            };
            close_qr();
            cg_fail(&cg);
            return fail(msg, start, enforced);
        }
    };

    // ---- honest report ----
    let en = |ok: bool, why: &str| {
        if ok {
            Enforcement::Enforced
        } else {
            Enforcement::NotEnforced(why.to_string())
        }
    };
    let ns_why = if !policy.namespaces {
        "disabled by policy"
    } else if !feat.userns {
        "unprivileged user namespaces unavailable (kernel/sysctl/AppArmor/seccomp)"
    } else {
        "unshare failed at run time"
    };
    set(
        &mut enforced,
        "rlimits",
        en(bits & B_RLIMIT != 0, "not applied"),
    );
    set(&mut enforced, "env", Enforcement::Enforced);
    set(&mut enforced, "workdir", Enforcement::Enforced);
    set(
        &mut enforced,
        "workdir_quota",
        if wdq == 0 {
            ne("disabled by policy (workdir_tmpfs_bytes = 0); RLIMIT_FSIZE caps single files only")
        } else if bits & B_WDQ != 0 {
            Enforcement::Enforced
        } else if bits & B_MNTNS == 0 || bits & B_PIDNS == 0 {
            ne("needs mount + pid namespaces; the work dir has no total size cap")
        } else {
            ne("tmpfs mount refused; the work dir has no total size cap")
        },
    );
    set(
        &mut enforced,
        "no_new_privs",
        en(bits & B_NNP != 0, "prctl failed"),
    );
    // (entering a user namespace without writing the map now fails the run)
    set(&mut enforced, "userns", en(bits & B_USERNS != 0, ns_why));
    set(
        &mut enforced,
        "fds",
        en(
            bits & B_FDS != 0,
            "close_range(2) unavailable and the fd limit is above the fallback loop",
        ),
    );
    set(
        &mut enforced,
        "cgroup",
        if cg.is_none() {
            ne("no cgroup_parent in policy; no pids.max / memory.max for the tree")
        } else {
            en(bits & B_CGROUP != 0, "cgroup join failed")
        },
    );
    set(
        &mut enforced,
        "net",
        if policy.net {
            ne("network allowed by policy (net = true)")
        } else if bits & B_NETNS != 0 {
            Enforcement::Enforced
        } else if bits & B_LL_NET != 0 {
            ne("only TCP connect/bind blocked (landlock); UDP and other sockets NOT blocked")
        } else {
            ne("no network namespace and no landlock TCP deny on this platform")
        },
    );
    set(&mut enforced, "pidns", en(bits & B_PIDNS != 0, ns_why));
    let mnt_why = if !(policy.ro_root || !policy.hide_paths.is_empty() || wdq > 0) {
        "not requested by policy"
    } else {
        ns_why
    };
    set(&mut enforced, "mountns", en(bits & B_MNTNS != 0, mnt_why));
    set(
        &mut enforced,
        "ro_root",
        en(
            bits & B_RO_ROOT != 0,
            if !policy.ro_root {
                "disabled by policy"
            } else if bits & B_MNTNS == 0 {
                "needs a mount namespace"
            } else if !feat.mount_setattr {
                "kernel lacks mount_setattr (Linux < 5.12)"
            } else {
                "mount_setattr refused"
            },
        ),
    );
    let hide_state = if bits & B_HIDE != 0 {
        if hide_skipped.is_empty() {
            Enforcement::Enforced
        } else {
            Enforcement::NotEnforced(format!(
                "partial: kept visible because needed: {}",
                hide_skipped.join(", ")
            ))
        }
    } else if hide_count == 0 {
        Enforcement::NotEnforced(if hide_skipped.is_empty() {
            "nothing to hide".into()
        } else {
            format!("kept visible because needed: {}", hide_skipped.join(", "))
        })
    } else if bits & B_MNTNS == 0 {
        ne("needs a mount namespace")
    } else {
        ne("tmpfs mount refused")
    };
    set(&mut enforced, "hide_home", hide_state);
    let mask_err = ((bits >> PROC_MASK_ERR_SHIFT) & 63) as i32;
    let proc_view = if bits & B_PROC != 0 {
        ProcView::Private
    } else if bits & B_PROC_PIDONLY != 0 {
        ProcView::PidOnly
    } else if bits & B_PROC_MASK != 0 {
        ProcView::Masked {
            static_files: bits >> PROC_MASK_SHIFT,
        }
    } else {
        ProcView::Host
    };
    set(
        &mut enforced,
        "proc",
        if proc_view != ProcView::Host {
            Enforcement::Enforced
        } else if bits & B_PIDNS == 0 || bits & B_MNTNS == 0 {
            ne("needs pid + mount namespaces; host /proc stays visible")
        } else if proc_fb == ProcFallback::Host {
            ne("proc mount refused (e.g. container with overmounted /proc) and proc_fallback = Host; host /proc stays visible")
        } else if mask_err != 0 {
            Enforcement::NotEnforced(format!(
                "proc mount refused and the /proc mask failed ({}); host /proc stays visible",
                std::io::Error::from_raw_os_error(mask_err)
            ))
        } else {
            ne("proc mount refused and no /proc mask possible for this work dir; host /proc stays visible")
        },
    );
    if ll.is_some() {
        set(
            &mut enforced,
            "landlock",
            en(bits & B_LANDLOCK != 0, "restrict_self failed"),
        );
    }
    set(
        &mut enforced,
        "caps",
        en(
            bits & B_CAPS != 0,
            if feat.caps {
                "capabilities could not be fully dropped"
            } else {
                "capget/capset unavailable (filtered); process keeps whatever it had"
            },
        ),
    );

    let pid = child.id() as i32; // pid of our own child; setsid made it the pgid
    let out_h = child
        .stdout
        .take()
        .map(|r| capped_reader(r, policy.output_cap));
    let err_h = child
        .stderr
        .take()
        .map(|r| capped_reader(r, policy.output_cap));

    let mut timed_out = false;
    let mut error = None;
    // Wait WITHOUT reaping (WNOWAIT): while our child is a zombie its pid,
    // and so the process group id, cannot be reused by an unrelated process.
    // Only then is the group killed, and only then is the child reaped.
    loop {
        // SAFETY: zeroed siginfo is a valid out-param for waitid.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waitid on our own child pid with a valid out pointer.
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        // SAFETY: si_pid is valid to read after a successful waitid.
        if r != 0 || unsafe { info.si_pid() } != 0 {
            break;
        }
        if start.elapsed() >= policy.wall_timeout {
            timed_out = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if pid > 1 {
        // SAFETY: our child is alive or an unreaped zombie, so pgid == pid is
        // still the group our setsid'd child created; nothing else is hit.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    // Processes that left the group with their own setsid() are still in
    // the cgroup; kill them before joining the output readers.
    if let Some(c) = &cg {
        c.kill();
    }
    let status = match child.wait() {
        Ok(st) => Some(st),
        Err(e) => {
            error = Some(format!("wait: {e}"));
            None
        }
    };
    let cgroup = cg.as_ref().map(cgroup::RunCgroup::finish);
    let workdir_quota = (bits & B_WDQ != 0).then(|| {
        let r = wdq_read(qr).filter(|r| r[1] == WDQ_OUT_DONE);
        let w = r.unwrap_or_default();
        WorkdirQuota {
            limit_bytes: wdq,
            limit_inodes: wdq_inodes,
            in_bytes: w[3],
            in_entries: w[4],
            in_skipped: w[5],
            used_bytes: w[6],
            copied_out: r.is_some(),
            out_complete: r.is_some() && w[2] == 0,
            out_bytes: w[7],
            out_entries: w[8],
            out_unchanged: w[9],
            out_skipped: w[10],
            errno: w[2] as i32,
        }
    });
    close_qr();
    let (stdout, stdout_truncated) = out_h.and_then(|h| h.join().ok()).unwrap_or_default();
    let (stderr, stderr_truncated) = err_h.and_then(|h| h.join().ok()).unwrap_or_default();
    let status = match status {
        Some(st) => match (st.code(), st.signal()) {
            (Some(c), _) => Status::Exited(c),
            (_, Some(s)) => Status::Signaled(s),
            _ => Status::Error,
        },
        None => Status::Error,
    };
    Outcome {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out,
        wall: start.elapsed(),
        error,
        enforced,
        cgroup,
        workdir_quota,
        proc_view,
    }
}

#[cfg(test)]
mod proc_mask_tests {
    use super::*;

    #[test]
    fn mask_paths_for_a_work_dir() {
        let m = ProcMaskPaths::new(Path::new("/var/tmp/x/w")).unwrap();
        assert_eq!(m.stage.as_bytes(), b"/var/tmp/x/w");
        assert_eq!(m.root.as_bytes(), b"/var/tmp/x/w/r");
        assert_eq!(m.inner.as_bytes(), b"/var/tmp/x/w/r/var/tmp/x/w");
        assert_eq!(m.proc_dir.as_bytes(), b"/var/tmp/x/w/r/proc");
        assert_eq!(m.statics.len(), PROC_MASK_STATIC.len());
        assert_eq!(m.statics[1].0.as_bytes(), b"/proc/meminfo");
        assert_eq!(m.statics[1].1.as_bytes(), b"/var/tmp/x/w/r/proc/meminfo");
    }

    #[test]
    fn no_mask_for_root_or_relative_work_dir() {
        assert!(ProcMaskPaths::new(Path::new("/")).is_none());
        assert!(ProcMaskPaths::new(Path::new("w")).is_none());
    }

    #[test]
    fn mask_is_the_default_everywhere() {
        assert_eq!(SandboxPolicy::new("/tmp").proc_fallback, ProcFallback::Mask);
        assert_eq!(ProcFallback::default(), ProcFallback::Mask);
        for p in Preset::all() {
            assert_eq!(
                p.policy(Path::new("/tmp/w")).proc_fallback,
                ProcFallback::Mask
            );
        }
    }

    #[test]
    fn status_bits_fit_the_mask_fields() {
        // errno 1..63 in bits 18..23 and a count up to 255 in 24..31 never
        // touch a layer bit.
        let all_layers = B_WDQ | B_PROC_PIDONLY | B_PROC_MASK | (B_WDQ - 1);
        let fields = (63u32 << PROC_MASK_ERR_SHIFT) | (255u32 << PROC_MASK_SHIFT);
        assert_eq!(all_layers & fields, 0);
        assert_eq!((fields >> PROC_MASK_SHIFT) & 255, 255);
    }
}
