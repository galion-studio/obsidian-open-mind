// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Shared plumbing for the Rust `GALION_REAL=1` real-life tests that replaced
//! the old `scripts/real/*.sh` drivers (Rust-only rule, 2026-10-20 20:41).
//!
//! A real test: locates or builds RELEASE binaries, starts them on loopback
//! spare ports, probes them over real sockets, and tears everything down in
//! `Drop` (so a failed assert never leaves a listener behind).
//!
//! Environment:
//! * `GALION_REAL=1`          run the real tests (otherwise they print `skipped` and pass)
//! * `GALION_REAL_VPS=1`      also run the READ-ONLY live VPS probes (ssh GETs only)
//! * `GALION_REAL_BIN_DIR`    use prebuilt release binaries from this dir (no cargo build)
//! * `GALION_REAL_JOBS`       `-j` for the build fallback (default 2)
//! * `GALION_REAL_KEEP=1`     keep scratch dirs for inspection
//!
//! The build fallback uses its OWN target dir (`$CARGO_TARGET_DIR/real-bins`):
//! the outer `cargo test` holds the lock on `$CARGO_TARGET_DIR` while tests run.

use std::fs::File;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// True when `GALION_REAL=1`; otherwise prints why the test is skipped.
pub fn enabled(test: &str) -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("{test}: skipped (set GALION_REAL=1)");
    }
    on
}

/// True when `GALION_REAL_VPS=1` (read-only live VPS probes).
pub fn vps_enabled() -> bool {
    std::env::var("GALION_REAL_VPS").as_deref() == Ok("1")
}

/// The workspace root.
pub fn repo_root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    p.canonicalize().unwrap_or(p)
}

/// Europe/Zagreb RFC 3339 now, e.g. `2026-10-21T02:30:00+02:00`.
pub fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

/// Print one timestamped progress line (shown with `--nocapture`).
pub fn say(msg: impl AsRef<str>) {
    println!("[{}] {}", stamp(), msg.as_ref());
}

/// `$CARGO_TARGET_DIR`, else `<repo>/target`.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

/// Directory holding release binaries for `cargo_args` (e.g.
/// `["-p", "tuner", "-p", "room"]`). With `GALION_REAL_BIN_DIR` set it is
/// returned as is; otherwise a CPU-polite `nice -n 10 cargo build --release
/// --locked` runs into `$CARGO_TARGET_DIR/real-bins`.
pub fn release_bins(cargo_args: &[&str]) -> PathBuf {
    release_bins_for(None, cargo_args)
}

/// Like [`release_bins`] for a cross `--target` triple.
pub fn release_bins_for(triple: Option<&str>, cargo_args: &[&str]) -> PathBuf {
    if let Some(d) = std::env::var_os("GALION_REAL_BIN_DIR") {
        // GALION_REAL_BIN_DIR is <target>/release; a cross build lives in
        // <target>/<triple>/release next to it.
        let d = PathBuf::from(d);
        if let (Some(t), Some(base)) = (triple, d.parent()) {
            let cross = base.join(t).join("release");
            if cross.is_dir() {
                return cross;
            }
        }
        return d;
    }
    let tdir = target_dir().join("real-bins");
    let jobs = std::env::var("GALION_REAL_JOBS").unwrap_or_else(|_| "2".into());
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new("nice");
    cmd.args([
        "-n",
        "10",
        &cargo,
        "build",
        "--release",
        "--locked",
        "-j",
        &jobs,
    ])
    .args(cargo_args)
    .current_dir(repo_root())
    .env("CARGO_TARGET_DIR", &tdir)
    .env("CARGO_INCREMENTAL", "0");
    if let Some(t) = triple {
        cmd.args(["--target", t]);
    }
    say(format!(
        "build: cargo build --release {}",
        cargo_args.join(" ")
    ));
    let st = cmd.status().expect("spawn cargo build");
    assert!(st.success(), "cargo build --release {cargo_args:?} failed");
    match triple {
        Some(t) => tdir.join(t).join("release"),
        None => tdir.join("release"),
    }
}

/// A started process, killed (SIGKILL) and reaped on drop.
pub struct Proc {
    pub name: String,
    pub log: PathBuf,
    child: Option<Child>,
}

impl Proc {
    /// Spawn `cmd` with stdout+stderr appended to `log`.
    pub fn spawn(name: &str, mut cmd: Command, log: &Path) -> Proc {
        let f = File::options()
            .create(true)
            .append(true)
            .open(log)
            .unwrap_or_else(|e| panic!("open {}: {e}", log.display()));
        let f2 = f.try_clone().expect("clone log fd");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(f))
            .stderr(Stdio::from(f2));
        let child = cmd.spawn().unwrap_or_else(|e| panic!("spawn {name}: {e}"));
        say(format!("started {name} pid {}", child.id()));
        Proc {
            name: name.into(),
            log: log.to_path_buf(),
            child: Some(child),
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.as_ref().map(|c| c.id()).unwrap_or(0)
    }

    /// Still running?
    pub fn alive(&mut self) -> bool {
        matches!(self.child.as_mut().map(|c| c.try_wait()), Some(Ok(None)))
    }

    /// SIGKILL and reap.
    pub fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// SIGTERM (graceful), wait up to `grace`, then SIGKILL.
    pub fn stop(&mut self, grace: Duration) {
        if let Some(c) = self.child.as_mut() {
            let _ = Command::new("kill")
                .args(["-TERM", &c.id().to_string()])
                .status();
            let end = Instant::now() + grace;
            while Instant::now() < end {
                if let Ok(Some(_)) = c.try_wait() {
                    self.child = None;
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self.kill();
    }

    /// Wait for a process that exits on its own; returns its exit code.
    pub fn wait(&mut self, timeout: Duration) -> Option<i32> {
        let c = self.child.as_mut()?;
        let end = Instant::now() + timeout;
        loop {
            if let Ok(Some(st)) = c.try_wait() {
                self.child = None;
                return st.code();
            }
            if Instant::now() > end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The process's log file so far.
    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.kill();
    }
}

/// True when something accepts a TCP connection on `addr`.
pub fn port_open(addr: &str) -> bool {
    crate::http::port_open(addr)
}

/// Wait until `addr` accepts connections.
pub fn wait_port(addr: &str, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if port_open(addr) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Wait until `GET path` on `addr` answers any HTTP status.
pub fn wait_http(addr: &str, path: &str, timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if crate::http::request(addr, "GET", path, &[], b"", Duration::from_secs(1)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Panic unless every loopback port in `ports` is free (another run or agent
/// may be using it; a clear message beats a confusing failure later).
pub fn require_free(ports: &[u16]) {
    for p in ports {
        assert!(
            !port_open(&format!("127.0.0.1:{p}")),
            "port 127.0.0.1:{p} is already in use; stop the other run first"
        );
    }
}

/// Wait until none of `ports` accepts connections; returns the ones still open.
pub fn ports_closed(ports: &[u16], timeout: Duration) -> Vec<u16> {
    let end = Instant::now() + timeout;
    loop {
        let open: Vec<u16> = ports
            .iter()
            .copied()
            .filter(|p| {
                format!("127.0.0.1:{p}")
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .is_some_and(|a| {
                        TcpStream::connect_timeout(&a, Duration::from_millis(200)).is_ok()
                    })
            })
            .collect();
        if open.is_empty() || Instant::now() > end {
            return open;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// `GALION_REAL_PORT_BASE` (to move a test into another agent's slot range),
/// else `default`.
pub fn port_base(default: u16) -> u16 {
    std::env::var("GALION_REAL_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A private scratch directory, removed on drop unless `GALION_REAL_KEEP=1`.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("galion-real-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("create scratch dir");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
        }
        Scratch(d)
    }
    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::env::var("GALION_REAL_KEEP").as_deref() == Ok("1") {
            eprintln!("kept scratch dir {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Run a command to completion, capturing stdout and stderr.
pub fn run(cmd: &mut Command) -> Output {
    cmd.stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("spawn {cmd:?}: {e}"))
}

/// Run and parse stdout as JSON (panics with stderr on bad output).
pub fn run_json(cmd: &mut Command) -> (i32, serde_json::Value) {
    let out = run(cmd);
    let code = out.status.code().unwrap_or(-1);
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "{cmd:?}: stdout is not JSON ({e}); exit {code}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (code, v)
}

/// Named PASS/FAIL checks collected over a scenario (the old scripts'
/// `check NAME GOT WANT`), asserted at the end so every step is reported.
#[derive(Default)]
pub struct Checks {
    pub pass: u32,
    pub fail: u32,
    pub failed: Vec<String>,
}

impl Checks {
    pub fn new() -> Checks {
        Checks::default()
    }
    pub fn ok(&mut self, name: &str) {
        self.pass += 1;
        say(format!("PASS {name}"));
    }
    pub fn bad(&mut self, name: &str) {
        self.fail += 1;
        self.failed.push(name.to_string());
        say(format!("FAIL {name}"));
    }
    /// Record `got == want`.
    pub fn eq<T: PartialEq + std::fmt::Debug>(&mut self, name: &str, got: T, want: T) {
        if got == want {
            self.pass += 1;
            say(format!("PASS {name} ({got:?})"));
        } else {
            self.bad(&format!("{name}: got {got:?} want {want:?}"));
        }
    }
    /// Record a boolean condition.
    pub fn that(&mut self, name: &str, cond: bool) {
        if cond {
            self.ok(name)
        } else {
            self.bad(name)
        }
    }
    /// Panic with the failed list unless every check passed.
    pub fn finish(&self, test: &str) {
        say(format!(
            "{test}: RESULT pass={} fail={}",
            self.pass, self.fail
        ));
        assert!(
            self.fail == 0,
            "{test}: {} failed checks:\n  {}",
            self.fail,
            self.failed.join("\n  ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_count_and_report() {
        let mut c = Checks::new();
        c.eq("same", 1, 1);
        c.that("true", true);
        c.eq("differs", "a", "b");
        assert_eq!((c.pass, c.fail), (2, 1));
        assert!(c.failed[0].contains("differs"));
    }

    #[test]
    fn scratch_is_private_and_removed() {
        let p;
        {
            let s = Scratch::new("unit");
            p = s.0.clone();
            assert!(p.is_dir());
        }
        if std::env::var("GALION_REAL_KEEP").as_deref() != Ok("1") {
            assert!(!p.exists());
        }
    }

    #[test]
    fn stamp_is_zagreb() {
        assert!(zagreb_time::require_rfc3339(&stamp()).is_ok());
    }
}
