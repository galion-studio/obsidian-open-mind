// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Sandbox lifecycle on top of the `galion-box` launcher (Galion OS L3).
//!
//! ```text
//! galion-admin sandbox start NAME ... -- CMD
//!   └─ galion-admin __supervise NAME   (own process group, holds NAME.lock, writes NAME.state,
//!        │                             audits sandbox.start / sandbox.exit)
//!        └─ galion-box run ... -- CMD  (PDEATHSIG=SIGTERM: if the supervisor dies, the box dies,
//!                                       and galion-box's own pid-1 kills the whole namespace)
//! ```
//!
//! Liveness is decided by the advisory lock the supervisor holds for its whole life, not by a
//! pid alone, so a recycled pid can never be mistaken for a running sandbox. `stop` signals the
//! box pid only after re-checking its kernel start time. Nothing here uses `su` or root.

use crate::audit;
use crate::timefmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Options for `sandbox start`, translated 1:1 into `galion-box run` flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartOpts {
    pub name: String,
    pub rootfs: PathBuf,
    pub net_lo: bool,
    pub ro_root: bool,
    pub binds_ro: Vec<String>,
    pub timeout_secs: Option<String>,
    pub nproc: Option<u32>,
    pub as_mb: Option<u64>,
    pub nice: Option<u32>,
    pub settle_ms: u64,
    pub argv: Vec<String>,
}

pub fn validate_name(n: &str) -> Result<(), String> {
    let ok = !n.is_empty()
        && n.len() <= 32
        && n.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !n.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "sandbox name {n:?} must be 1-32 chars of a-z, 0-9, '-' (not starting with '-')"
        ))
    }
}

impl StartOpts {
    pub fn validate(&self) -> Result<(), String> {
        validate_name(&self.name)?;
        if !self.rootfs.is_absolute() {
            return Err("--rootfs must be an absolute path".into());
        }
        if self.rootfs == Path::new("/") {
            return Err("--rootfs / is refused (the sandbox must not see the host root)".into());
        }
        if self.argv.is_empty() {
            return Err("missing payload: sandbox start NAME --rootfs DIR -- CMD [ARGS...]".into());
        }
        for b in &self.binds_ro {
            let (s, d) = b.split_once(':').ok_or("--bind-ro needs SRC:DST")?;
            if !s.starts_with('/') || !d.starts_with('/') {
                return Err("--bind-ro paths must be absolute".into());
            }
        }
        if let Some(n) = self.nice {
            if n > 19 {
                return Err("--nice must be 0..19".into());
            }
        }
        Ok(())
    }

    /// `galion-box` argv (without the program).
    pub fn box_args(&self) -> Vec<String> {
        let mut a = vec![
            "run".to_string(),
            "--rootfs".into(),
            self.rootfs.display().to_string(),
            "--net".into(),
            if self.net_lo { "lo" } else { "none" }.into(),
        ];
        if self.ro_root {
            a.push("--ro-root".into());
        }
        for b in &self.binds_ro {
            a.push("--bind-ro".into());
            a.push(b.clone());
        }
        if let Some(t) = &self.timeout_secs {
            a.push("--timeout".into());
            a.push(t.clone());
        }
        if let Some(n) = self.nproc {
            a.push("--nproc".into());
            a.push(n.to_string());
        }
        if let Some(m) = self.as_mb {
            a.push("--as-mb".into());
            a.push(m.to_string());
        }
        if let Some(n) = self.nice {
            a.push("--nice".into());
            a.push(n.to_string());
        }
        a.push("--".into());
        a.extend(self.argv.iter().cloned());
        a
    }
}

/// Audit-safe description of a payload: base name of argv[0] and the argument count only.
pub fn cmd_summary(argv: &[String]) -> String {
    let prog = argv
        .first()
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
        .unwrap_or_default();
    format!("cmd={} argc={}", audit::clean(&prog), argv.len())
}

/// Paths under the admin state dir.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn dir(&self) -> PathBuf {
        self.root.join("sandboxes")
    }
    pub fn lock(&self, n: &str) -> PathBuf {
        self.dir().join(format!("{n}.lock"))
    }
    pub fn state(&self, n: &str) -> PathBuf {
        self.dir().join(format!("{n}.state"))
    }
    pub fn log(&self, n: &str) -> PathBuf {
        self.dir().join(format!("{n}.log"))
    }
    pub fn audit(&self) -> PathBuf {
        self.root.join("audit.jsonl")
    }
}

/// Create a private (0700) directory, refusing a group/world-writable existing one.
pub fn ensure_private_dir(p: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)?;
    let m = fs::metadata(p)?.permissions().mode();
    if m & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is group/world-writable (mode {:o}); refusing",
                p.display(),
                m & 0o777
            ),
        ));
    }
    Ok(())
}

/// The sandbox state file (`key=value` lines).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    pub name: String,
    pub status: String,
    pub supervisor_pid: u32,
    pub box_pid: u32,
    pub box_ticks: u64,
    pub started: String,
    pub cmd: String,
    pub exit: String,
    pub ended: String,
}

impl State {
    pub fn render(&self) -> String {
        format!(
            "name={}\nstatus={}\nsupervisor_pid={}\nbox_pid={}\nbox_ticks={}\nstarted={}\ncmd={}\nexit={}\nended={}\n",
            self.name, self.status, self.supervisor_pid, self.box_pid, self.box_ticks,
            self.started, self.cmd, self.exit, self.ended
        )
    }
    pub fn parse(t: &str) -> State {
        let mut s = State::default();
        for l in t.lines() {
            if let Some((k, v)) = l.split_once('=') {
                let v = v.to_string();
                match k {
                    "name" => s.name = v,
                    "status" => s.status = v,
                    "supervisor_pid" => s.supervisor_pid = v.parse().unwrap_or(0),
                    "box_pid" => s.box_pid = v.parse().unwrap_or(0),
                    "box_ticks" => s.box_ticks = v.parse().unwrap_or(0),
                    "started" => s.started = v,
                    "cmd" => s.cmd = v,
                    "exit" => s.exit = v,
                    "ended" => s.ended = v,
                    _ => {}
                }
            }
        }
        s
    }
}

pub fn write_state(path: &Path, s: &State) -> io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("state.tmp");
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(s.render().as_bytes())?;
    f.sync_data()?;
    fs::rename(&tmp, path)
}

pub fn read_state(path: &Path) -> Option<State> {
    fs::read_to_string(path).ok().map(|t| State::parse(&t))
}

/// (state char, start time in clock ticks) of a pid, from /proc/PID/stat.
pub fn proc_stat(pid: u32) -> Option<(char, u64)> {
    let t = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&t)
}

/// Parse a /proc/PID/stat line (comm may contain spaces and ')').
pub fn parse_stat(t: &str) -> Option<(char, u64)> {
    let rest = &t[t.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let state = f.first()?.chars().next()?;
    let ticks = f.get(19)?.parse().ok()?;
    Some((state, ticks))
}

/// True if the supervisor for `name` holds its lock (= the sandbox is running).
pub fn is_running(p: &Paths, name: &str) -> io::Result<bool> {
    let f = match File::open(p.lock(name)) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    // SAFETY: flock on an fd we own; released when `f` is dropped.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) };
    if r == 0 {
        return Ok(false);
    }
    let e = io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(true)
    } else {
        Err(e)
    }
}

fn try_lock_exclusive(path: &Path) -> io::Result<Option<File>> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    // SAFETY: flock on an fd we own (CLOEXEC by default in std, so children never inherit it).
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        Ok(Some(f))
    } else {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(None)
        } else {
            Err(e)
        }
    }
}

fn log_tail(path: &Path, max: u64) -> String {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let len = f.seek(SeekFrom::End(0)).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(max)));
    let mut s = String::new();
    let _ = f.read_to_string(&mut s);
    s.trim().to_string()
}

/// Environment passed to children: nothing from the host except PATH and TZ.
fn minimal_env(c: &mut Command) {
    c.env_clear().env(
        "PATH",
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
    );
    if let Ok(tz) = std::env::var("TZ") {
        c.env("TZ", tz);
    }
}

/// `sandbox start`: spawn the supervisor and wait `settle_ms` for an early failure.
pub fn start(p: &Paths, box_bin: &Path, o: &StartOpts) -> Result<String, String> {
    o.validate()?;
    ensure_private_dir(&p.root).map_err(|e| e.to_string())?;
    ensure_private_dir(&p.dir()).map_err(|e| e.to_string())?;
    if is_running(p, &o.name).map_err(|e| e.to_string())? {
        return Err(format!("sandbox {} is already running", o.name));
    }
    // Fail closed: no sandbox starts if its start could not be audited.
    audit::check_tail(&p.audit()).map_err(|e| e.to_string())?;
    reconcile(p, &o.name);
    let _ = fs::remove_file(p.state(&o.name));
    let log = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(p.log(&o.name))
        .map_err(|e| format!("log file: {e}"))?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut c = Command::new(me);
    minimal_env(&mut c);
    c.arg("--state")
        .arg(&p.root)
        .arg("__supervise")
        .arg(&o.name)
        .arg(cmd_summary(&o.argv))
        .arg(box_bin)
        .args(o.box_args())
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2)
        .process_group(0);
    let mut child = c.spawn().map_err(|e| {
        let _ = audit::append(
            &p.audit(),
            "sandbox.start",
            &o.name,
            "failed",
            "supervisor spawn error",
        );
        format!("spawning supervisor: {e}")
    })?;
    let deadline = Instant::now() + Duration::from_millis(o.settle_ms);
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            let s = read_state(&p.state(&o.name));
            let tail = log_tail(&p.log(&o.name), 600);
            return match s {
                Some(s) if s.exit == "exit:0" => Ok(format!(
                    "sandbox {} ran and finished within {} ms (exit 0)",
                    o.name, o.settle_ms
                )),
                Some(s) => Err(format!(
                    "sandbox {} failed to start: box {} \n{tail}",
                    o.name,
                    if s.exit.is_empty() {
                        "did not run".into()
                    } else {
                        s.exit
                    }
                )),
                None => {
                    let _ = audit::append(
                        &p.audit(),
                        "sandbox.start",
                        &o.name,
                        "failed",
                        &format!("supervisor exited {st} before starting the box"),
                    );
                    Err(format!(
                        "sandbox {} failed to start (supervisor {st})\n{tail}",
                        o.name
                    ))
                }
            };
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let s = read_state(&p.state(&o.name)).unwrap_or_default();
    Ok(format!(
        "sandbox {} started: box pid {}, {} (log {})",
        o.name,
        s.box_pid,
        s.cmd,
        p.log(&o.name).display()
    ))
}

fn exit_label(st: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (st.code(), st.signal()) {
        (Some(c), _) => format!("exit:{c}"),
        (None, Some(s)) => format!("signal:{s}"),
        _ => "unknown".into(),
    }
}

/// The hidden `__supervise` process. Returns the process exit code.
pub fn supervise(p: &Paths, name: &str, cmd: &str, box_bin: &Path, box_args: &[String]) -> i32 {
    if validate_name(name).is_err() {
        eprintln!("galion-admin supervisor: bad name");
        return 2;
    }
    let _lock = match try_lock_exclusive(&p.lock(name)) {
        Ok(Some(f)) => f,
        Ok(None) => {
            eprintln!("galion-admin supervisor: sandbox {name} is already running");
            return 5;
        }
        Err(e) => {
            eprintln!("galion-admin supervisor: lock: {e}");
            return 1;
        }
    };
    let mut s = State {
        name: name.into(),
        status: "starting".into(),
        // SAFETY: trivial getter.
        supervisor_pid: unsafe { libc::getpid() } as u32,
        started: timefmt::now_local(),
        cmd: cmd.into(),
        ..State::default()
    };
    let mut c = Command::new(box_bin);
    minimal_env(&mut c);
    c.args(box_args).stdin(Stdio::null());
    // SAFETY: prctl is async-signal-safe; runs in the child between fork and exec.
    unsafe {
        c.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM as libc::c_ulong) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            s.status = "exited".into();
            s.exit = format!("spawn-error:{}", e.kind());
            s.ended = timefmt::now_local();
            let _ = write_state(&p.state(name), &s);
            let _ = audit::append(
                &p.audit(),
                "sandbox.start",
                name,
                "failed",
                &format!("{cmd} galion-box spawn error: {}", e.kind()),
            );
            eprintln!(
                "galion-admin supervisor: cannot run {}: {e}",
                box_bin.display()
            );
            return 1;
        }
    };
    s.box_pid = child.id();
    s.box_ticks = proc_stat(s.box_pid).map(|x| x.1).unwrap_or(0);
    s.status = "running".into();
    if let Err(e) = write_state(&p.state(name), &s) {
        eprintln!("galion-admin supervisor: state: {e}");
    }
    let _ = audit::append(
        &p.audit(),
        "sandbox.start",
        name,
        "launched",
        &format!("{cmd} box_pid={}", s.box_pid),
    );
    let st = loop {
        match child.wait() {
            Ok(st) => break Some(st),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break None,
        }
    };
    s.status = "exited".into();
    s.exit = st.map(exit_label).unwrap_or_else(|| "unknown".into());
    s.ended = timefmt::now_local();
    let _ = write_state(&p.state(name), &s);
    let _ = audit::append(&p.audit(), "sandbox.exit", name, &s.exit, cmd);
    0
}

fn wait_stopped(p: &Paths, name: &str, d: Duration) -> bool {
    let end = Instant::now() + d;
    loop {
        if !is_running(p, name).unwrap_or(true) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Signal the box pid only if it is still the process we started (same kernel start time).
fn signal_box(s: &State, sig: libc::c_int) -> Result<bool, String> {
    match proc_stat(s.box_pid) {
        Some((st, ticks)) if ticks == s.box_ticks && st != 'Z' => {
            // SAFETY: kill(2) on a verified pid.
            let r = unsafe { libc::kill(s.box_pid as libc::pid_t, sig) };
            if r == 0 {
                Ok(true)
            } else {
                Err(format!("kill: {}", io::Error::last_os_error()))
            }
        }
        Some((_, ticks)) if ticks != s.box_ticks => {
            Err("box pid was recycled by another process; refusing to signal it".into())
        }
        _ => Ok(false),
    }
}

/// `sandbox stop`: SIGTERM the box, wait `grace`, then SIGKILL. Ok(None) = was not running.
pub fn stop(p: &Paths, name: &str, grace: Duration) -> Result<Option<String>, String> {
    validate_name(name)?;
    if !is_running(p, name).map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let s = read_state(&p.state(name)).ok_or("sandbox is running but has no state file")?;
    if s.box_pid == 0 {
        return Err("sandbox is still starting; try again".into());
    }
    let t = Instant::now();
    signal_box(&s, libc::SIGTERM)?;
    let mut how = "SIGTERM";
    if !wait_stopped(p, name, grace) {
        signal_box(&s, libc::SIGKILL)?;
        how = "SIGKILL after grace";
        if !wait_stopped(p, name, Duration::from_secs(5)) {
            let _ = audit::append(
                &p.audit(),
                "sandbox.stop",
                name,
                "failed",
                "still running after SIGKILL",
            );
            return Err(format!("sandbox {name} is still running after SIGKILL"));
        }
    }
    let end = read_state(&p.state(name)).unwrap_or_default();
    let ms = t.elapsed().as_millis();
    let _ = audit::append(
        &p.audit(),
        "sandbox.stop",
        name,
        "ok",
        &format!("{how} -> {} in {ms} ms", end.exit),
    );
    Ok(Some(format!(
        "sandbox {name} stopped ({how}, box {}, {ms} ms)",
        end.exit
    )))
}

/// If the supervisor died without writing an exit record (kill -9, OOM, reboot), record that
/// once as `lost` in the state file and the audit log, under the sandbox lock so concurrent
/// callers cannot double-record it.
pub fn reconcile(p: &Paths, name: &str) {
    let Ok(Some(_lock)) = try_lock_exclusive(&p.lock(name)) else {
        return;
    };
    let Some(mut s) = read_state(&p.state(name)) else {
        return;
    };
    if s.status != "running" && s.status != "starting" {
        return;
    }
    let box_alive = proc_stat(s.box_pid).is_some_and(|(st, t)| t == s.box_ticks && st != 'Z');
    s.status = "lost".into();
    s.exit = "lost".into();
    s.ended = timefmt::now_local();
    let _ = write_state(&p.state(name), &s);
    let _ = audit::append(
        &p.audit(),
        "sandbox.lost",
        name,
        "supervisor-gone",
        &format!("{} box_pid={} box_alive={box_alive}", s.cmd, s.box_pid),
    );
}

/// One status line per known sandbox (or just `only`).
pub fn status(p: &Paths, only: Option<&str>) -> Result<Vec<(String, bool)>, String> {
    let mut names: Vec<String> = match only {
        Some(n) => {
            validate_name(n)?;
            vec![n.to_string()]
        }
        None => match fs::read_dir(p.dir()) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    e.file_name()
                        .to_str()
                        .and_then(|f| f.strip_suffix(".state"))
                        .map(str::to_string)
                })
                .collect(),
            Err(_) => Vec::new(),
        },
    };
    names.sort();
    let mut out = Vec::new();
    for n in names {
        let running = is_running(p, &n).map_err(|e| e.to_string())?;
        if !running {
            reconcile(p, &n);
        }
        let line = match (read_state(&p.state(&n)), running) {
            (None, false) => format!("{n:<16} unknown"),
            (None, true) => format!("{n:<16} starting"),
            (Some(s), true) => format!(
                "{n:<16} running  box_pid={} since {} {}",
                s.box_pid, s.started, s.cmd
            ),
            (Some(s), false) if s.status == "lost" => format!(
                "{n:<16} lost     supervisor died without recording an exit (box_pid {}, noticed {}) {}",
                s.box_pid, s.ended, s.cmd
            ),
            (Some(s), false) if s.status == "exited" => format!(
                "{n:<16} exited   {} at {} (started {}) {}",
                s.exit, s.ended, s.started, s.cmd
            ),
            (Some(s), false) => format!(
                "{n:<16} stale    supervisor gone without a record (last status {}, box_pid {})",
                s.status, s.box_pid
            ),
        };
        out.push((line, running));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(validate_name("work-1").is_ok());
        for bad in ["", "-x", "UP", "a/b", "a.b", "../x", &"a".repeat(33)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    fn opts() -> StartOpts {
        StartOpts {
            name: "w".into(),
            rootfs: "/var/tmp/r".into(),
            argv: vec!["/bin/galion-box".into(), "sleep".into(), "60".into()],
            ..Default::default()
        }
    }

    #[test]
    fn box_args_default_and_full() {
        assert_eq!(
            opts().box_args(),
            [
                "run",
                "--rootfs",
                "/var/tmp/r",
                "--net",
                "none",
                "--",
                "/bin/galion-box",
                "sleep",
                "60"
            ]
        );
        let mut o = opts();
        o.net_lo = true;
        o.ro_root = true;
        o.binds_ro = vec!["/a:/b".into()];
        o.timeout_secs = Some("1.5".into());
        o.nproc = Some(16);
        o.as_mb = Some(256);
        o.nice = Some(19);
        let a = o.box_args().join(" ");
        assert_eq!(
            a,
            "run --rootfs /var/tmp/r --net lo --ro-root --bind-ro /a:/b --timeout 1.5 --nproc 16 --as-mb 256 --nice 19 -- /bin/galion-box sleep 60"
        );
    }

    #[test]
    fn validation_refuses() {
        let mut o = opts();
        o.rootfs = "/".into();
        assert!(o.validate().is_err());
        o.rootfs = "rel".into();
        assert!(o.validate().is_err());
        let mut o = opts();
        o.argv.clear();
        assert!(o.validate().is_err());
        let mut o = opts();
        o.binds_ro = vec!["nocolon".into()];
        assert!(o.validate().is_err());
        let mut o = opts();
        o.nice = Some(20);
        assert!(o.validate().is_err());
        assert!(opts().validate().is_ok());
    }

    #[test]
    fn summary_never_has_args() {
        let s = cmd_summary(&[
            "/usr/bin/curl".into(),
            "-H".into(),
            "Authorization: Bearer SECRET".into(),
        ]);
        assert_eq!(s, "cmd=curl argc=3");
        assert!(!s.contains("SECRET"));
    }

    #[test]
    fn state_roundtrip() {
        let s = State {
            name: "w".into(),
            status: "running".into(),
            supervisor_pid: 10,
            box_pid: 11,
            box_ticks: 12345,
            started: "2026-10-21T08:30:00+02:00".into(),
            cmd: "cmd=sleep argc=2".into(),
            exit: String::new(),
            ended: String::new(),
        };
        assert_eq!(State::parse(&s.render()), s);
    }

    #[test]
    fn stat_parse_with_odd_comm() {
        let line = "1234 (a) b (c) S 1 1234 1234 0 -1 4194560 100 0 0 0 0 0 0 0 20 0 1 0 987654 1000 10 18446744073709551615";
        assert_eq!(parse_stat(line), Some(('S', 987_654)));
        assert!(proc_stat(std::process::id()).is_some());
        assert_eq!(parse_stat("garbage"), None);
    }
}
