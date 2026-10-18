// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! openmind-sandbox v0: run one command with rlimits, a cleared environment,
//! a fixed working directory, a wall-clock timeout and, when the kernel
//! supports it, Landlock filesystem restrictions. Linux only.
//!
//! Every restriction is reported in [`Outcome::enforced`]; nothing is claimed
//! that was not applied. The timeout only ever signals the child this crate
//! spawned (and the process group it created with `setsid`).

use std::ffi::CString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Maximum bytes kept per output stream.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// What to enforce for one run.
#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    pub workdir: PathBuf,
    /// Names copied from the parent env; everything else is dropped.
    pub env_allowlist: Vec<String>,
    pub wall_timeout: Duration,
    /// RLIMIT_CPU seconds.
    pub cpu_secs: u64,
    /// RLIMIT_AS bytes.
    pub mem_bytes: u64,
    /// RLIMIT_NOFILE.
    pub nofile: u64,
    /// false = network should be denied (not enforceable in v0, reported).
    pub net: bool,
    /// Landlock read roots (workdir is always added).
    pub fs_read: Vec<PathBuf>,
    /// Landlock write roots (workdir is always added).
    pub fs_write: Vec<PathBuf>,
    /// Device files granted read+write file access under Landlock (shells
    /// need `/dev/null` for redirects and `&`; pipelines read `/dev/zero`).
    /// Only file rights are granted, never directory rights on `/dev`.
    pub fs_dev: Vec<PathBuf>,
}

impl SandboxPolicy {
    /// Conservative default for a given workdir.
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        let p = |s: &str| PathBuf::from(s);
        SandboxPolicy {
            workdir: workdir.into(),
            env_allowlist: vec!["PATH".into(), "LANG".into()],
            wall_timeout: Duration::from_secs(10),
            cpu_secs: 5,
            mem_bytes: 512 * 1024 * 1024,
            nofile: 64,
            net: false,
            fs_read: vec![p("/usr"), p("/bin"), p("/lib"), p("/lib64"), p("/etc")],
            fs_write: vec![],
            fs_dev: vec![
                p("/dev/null"),
                p("/dev/zero"),
                p("/dev/random"),
                p("/dev/urandom"),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Enforcement {
    Enforced,
    NotEnforced(String),
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
    pub timed_out: bool,
    pub wall: Duration,
    pub error: Option<String>,
    /// (layer, state) for: rlimits, env, workdir, landlock, net.
    pub enforced: Vec<(String, Enforcement)>,
}

impl Outcome {
    pub fn layer(&self, name: &str) -> Option<&Enforcement> {
        self.enforced
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, e)| e)
    }
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

// ---- Landlock via raw syscalls (ABI v1 filesystem rights) ----
const SYS_LL_CREATE: libc::c_long = 444;
const SYS_LL_ADD: libc::c_long = 445;
const SYS_LL_RESTRICT: libc::c_long = 446;
const LL_FS_ALL: u64 = 0x1FFF; // 13 ABI-1 rights
const LL_READ: u64 = 1 | (1 << 2) | (1 << 3); // exec, read file, read dir
/// Rights that apply to a non-directory (exec, write file, read file); the
/// kernel rejects a path-beneath rule on a file that carries directory rights.
const LL_FILE_MASK: u64 = 1 | (1 << 1) | (1 << 2);
const LL_DEV: u64 = (1 << 1) | (1 << 2); // write file, read file

#[repr(C)]
struct LlRulesetAttr {
    handled_access_fs: u64,
}
#[repr(C, packed)]
struct LlPathBeneath {
    allowed_access: u64,
    parent_fd: i32,
}

/// Landlock ABI version of the running kernel, or None if unavailable.
pub fn landlock_abi() -> Option<i64> {
    // SAFETY: flag-only query; null attr with size 0 is the documented form.
    let r = unsafe { libc::syscall(SYS_LL_CREATE, std::ptr::null::<u8>(), 0usize, 1u32) };
    if r >= 1 {
        Some(r as i64)
    } else {
        None
    }
}

#[derive(Clone)]
struct Landlock {
    ruleset_fd: i32,
    path_fds: Vec<(i32, u64)>,
}

impl Landlock {
    /// Built in the parent (allocation and open() allowed here).
    fn prepare(policy: &SandboxPolicy) -> Result<Landlock, String> {
        let attr = LlRulesetAttr {
            handled_access_fs: LL_FS_ALL,
        };
        // SAFETY: attr is a valid repr(C) struct of the size we pass.
        let fd = unsafe {
            libc::syscall(
                SYS_LL_CREATE,
                &attr as *const _,
                std::mem::size_of::<LlRulesetAttr>(),
                0u32,
            )
        };
        if fd < 0 {
            return Err(format!(
                "create_ruleset: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut ll = Landlock {
            ruleset_fd: fd as i32,
            path_fds: vec![],
        };
        let mut roots: Vec<(PathBuf, u64)> = vec![];
        for r in &policy.fs_read {
            roots.push((r.clone(), LL_READ));
        }
        for w in &policy.fs_write {
            roots.push((w.clone(), LL_FS_ALL));
        }
        for d in &policy.fs_dev {
            roots.push((d.clone(), LL_DEV));
        }
        roots.push((policy.workdir.clone(), LL_FS_ALL));
        for (path, acc) in roots {
            // Files (devices included) only take file rights.
            let acc = if path.is_dir() {
                acc
            } else {
                acc & LL_FILE_MASK
            };
            let c = match CString::new(path.as_os_str().as_bytes()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // SAFETY: c is a valid NUL-terminated string.
            let pfd = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if pfd >= 0 {
                ll.path_fds.push((pfd, acc));
            } // missing roots are skipped: they grant nothing
        }
        Ok(ll)
    }

    /// Async-signal-safe: syscalls only, no allocation. Runs after fork.
    fn apply(&self) -> std::io::Result<()> {
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

/// glibc types the resource as `__rlimit_resource_t`; musl (tier-1) uses c_int.
#[cfg(target_env = "gnu")]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(not(target_env = "gnu"))]
type RlimitResource = libc::c_int;

fn setrl(res: RlimitResource, v: u64) -> std::io::Result<()> {
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

fn capped_reader<R: Read + Send + 'static>(mut r: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut keep = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // keep draining after the cap so the child never blocks
                    let room = OUTPUT_CAP.saturating_sub(keep.len());
                    keep.extend_from_slice(&buf[..n.min(room)]);
                }
            }
        }
        keep
    })
}

fn fail(msg: String, start: Instant, enforced: Vec<(String, Enforcement)>) -> Outcome {
    Outcome {
        status: Status::Error,
        stdout: vec![],
        stderr: vec![],
        timed_out: false,
        wall: start.elapsed(),
        error: Some(msg),
        enforced,
    }
}

/// Run `cmd` (program + args) under `policy`. Never panics on bad input;
/// failures come back as `Status::Error` with `error` set.
pub fn run(cmd: &[String], policy: &SandboxPolicy) -> Outcome {
    let start = Instant::now();
    let ne = |s: &str| Enforcement::NotEnforced(s.to_string());
    let mut enforced: Vec<(String, Enforcement)> = vec![
        ("rlimits".into(), ne("not started")),
        ("env".into(), ne("not started")),
        ("workdir".into(), ne("not started")),
        ("landlock".into(), ne("not started")),
        ("net".into(), ne("not started")),
    ];
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

    // Landlock: only when the kernel has it and there is something to say.
    let ll = match landlock_abi() {
        None => {
            set(&mut enforced, "landlock", ne("kernel has no landlock"));
            None
        }
        Some(_) => match Landlock::prepare(policy) {
            Ok(l) => Some(l),
            Err(e) => {
                set(&mut enforced, "landlock", Enforcement::NotEnforced(e));
                None
            }
        },
    };

    let mut c = Command::new(prog);
    c.args(args)
        .env_clear()
        .current_dir(&policy.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for k in &policy.env_allowlist {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    let (cpu, mem, nofile) = (policy.cpu_secs, policy.mem_bytes, policy.nofile);
    let llown = ll.clone();
    // SAFETY: the closure runs between fork and exec and only calls
    // async-signal-safe functions: setsid, setrlimit, prctl, and raw
    // landlock syscalls on fds prepared before the fork. It allocates nothing
    // (the error path returns io::Error::last_os_error, an errno wrapper).
    // The closure owns a clone of the prepared fd list.
    unsafe {
        c.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            setrl(libc::RLIMIT_CPU, cpu)?;
            setrl(libc::RLIMIT_AS, mem)?;
            setrl(libc::RLIMIT_NOFILE, nofile)?;
            setrl(libc::RLIMIT_CORE, 0)?;
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(l) = &llown {
                l.apply()?;
            }
            Ok(())
        });
    }
    let spawned = c.spawn();
    if let Some(l) = &ll {
        l.close();
    }
    let mut child = match spawned {
        Ok(ch) => ch,
        Err(e) => return fail(format!("spawn {prog}: {e}"), start, enforced),
    };
    set(&mut enforced, "rlimits", Enforcement::Enforced);
    set(&mut enforced, "env", Enforcement::Enforced);
    set(&mut enforced, "workdir", Enforcement::Enforced);
    if ll.is_some() {
        set(&mut enforced, "landlock", Enforcement::Enforced);
    }
    set(
        &mut enforced,
        "net",
        if policy.net {
            Enforcement::Enforced
        } else {
            ne("net=false cannot be enforced in v0 (no netns/seccomp)")
        },
    );

    let pid = child.id() as i32; // pid of our own child; setsid made it the pgid
    let out_h = child.stdout.take().map(capped_reader);
    let err_h = child.stderr.take().map(capped_reader);

    let mut timed_out = false;
    let mut error = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(e) => {
                error = Some(format!("wait: {e}"));
                break None;
            }
        }
        if start.elapsed() >= policy.wall_timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        thread::sleep(Duration::from_millis(50));
    };
    // Clean up leftovers in the group we created (pgid == our child's pid).
    if pid > 1 {
        // SAFETY: signals only the process group created by our own setsid'd child.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let stdout = out_h.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err_h.and_then(|h| h.join().ok()).unwrap_or_default();
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
        timed_out,
        wall: start.elapsed(),
        error,
        enforced,
    }
}
