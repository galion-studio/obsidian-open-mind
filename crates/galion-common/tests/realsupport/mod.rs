// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Shared plumbing for the real-life tests (`real_migrate`, `real_common`):
//! build release binaries (also from an old revision, in a throw-away git
//! worktree), start them with a clean environment, wait for the port, sample
//! memory and threads from `/proc`, and tear everything down on drop.
//! This replaces the old `scripts/real/*.sh` drivers (Rust-only rule).

// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// True when `GALION_REAL=1`; otherwise prints why the test is skipped.
pub fn enabled(what: &str) -> bool {
    if std::env::var("GALION_REAL").ok().as_deref() == Some("1") {
        return true;
    }
    eprintln!("skipped {what}: set GALION_REAL=1 (and run with --ignored)");
    false
}

/// Europe/Zagreb wall clock with offset, for the report.
pub fn now() -> String {
    galion_common::time::now_rfc3339()
}

pub fn port_base(default: u16) -> u16 {
    std::env::var("GALION_REAL_PORT_BASE")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(default)
}

pub fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// The workspace root (two levels above this crate).
pub fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// Where the nested builds go: `$CARGO_TARGET_DIR/real-<tag>`, never the
/// directory the outer `cargo test` is using (no lock contention).
pub fn target_dir(tag: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("target"));
    base.join(format!("real-{tag}"))
}

/// A scratch directory removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let p = std::env::temp_dir().join(format!("{name}.{}.{nanos}", std::process::id()));
        std::fs::create_dir_all(&p).expect("scratch dir");
        Scratch(p)
    }
    pub fn join(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `nice -n 10 cargo build -q -j 3 --release -p ...` in `tree`, into
/// `target`. Returns `target/release`.
pub fn build(tree: &Path, target: &Path, packages: &[&str]) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new("nice");
    cmd.args(["-n", "10"])
        .arg(&cargo)
        .args(["build", "-q", "-j", "3", "--release"])
        .current_dir(tree)
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_INCREMENTAL", "0");
    for p in packages {
        cmd.args(["-p", p]);
    }
    let st = cmd.status().expect("run cargo build");
    assert!(st.success(), "cargo build failed in {}", tree.display());
    target.join("release")
}

/// Removes a `git worktree` on drop (also when a build fails).
struct Worktree<'a> {
    repo: &'a Path,
    path: PathBuf,
}

impl Drop for Worktree<'_> {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(self.repo)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .status();
        let _ = Command::new("git")
            .arg("-C")
            .arg(self.repo)
            .args(["worktree", "prune"])
            .status();
    }
}

/// Build `packages` at git revision `rev` in a temporary worktree under
/// `scratch`, then copy `bins` to `scratch/oldbin` (the worktree is removed
/// right after the build). Returns the directory with the copied binaries.
pub fn build_old(rev: &str, scratch: &Scratch, packages: &[&str], bins: &[&str]) -> PathBuf {
    let repo = workspace();
    let path = scratch.join("old-tree");
    let st = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "add", "-q", "--detach"])
        .arg(&path)
        .arg(rev)
        .status()
        .expect("git worktree add");
    assert!(st.success(), "git worktree add {rev} failed");
    let tree = Worktree { repo: &repo, path };
    let release = build(&tree.path, &target_dir("old"), packages);
    drop(tree);
    let out = scratch.join("oldbin");
    std::fs::create_dir_all(&out).unwrap();
    for b in bins {
        std::fs::copy(release.join(b), out.join(b)).unwrap_or_else(|e| panic!("copy old {b}: {e}"));
    }
    out
}

/// A started server; killed and reaped on drop.
pub struct Proc {
    pub name: String,
    pub child: Child,
}

impl Proc {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start `bin` with a clean environment (PATH, HOME=/nonexistent, no
/// openmind.toml, plus `envs`), stderr to `log`, and wait until `port`
/// accepts connections (20 s).
pub fn start(
    name: &str,
    bin: &Path,
    args: &[&str],
    envs: &[(&str, String)],
    port: u16,
    log: &Path,
) -> Proc {
    let log = std::fs::File::create(log).expect("log");
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .env("OPENMIND_CONFIG", "/nonexistent/openmind.toml")
        .stdout(Stdio::null())
        .stderr(log);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("{}: {e}", bin.display()));
    let mut p = Proc {
        name: name.to_string(),
        child,
    };
    let t = Instant::now();
    while TcpStream::connect_timeout(&addr(port), Duration::from_millis(100)).is_err() {
        assert!(p.alive(), "{name} ({}) exited at start", bin.display());
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "{name} did not listen on {port}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    p
}

/// `VmHWM`, `VmRSS` and `Threads` from `/proc/<pid>/status`.
pub fn proc_status(pid: u32) -> Option<(String, u64)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let mut out = String::new();
    let mut threads = 0;
    for l in s.lines() {
        for k in ["VmHWM:", "VmRSS:", "Threads:"] {
            if let Some(v) = l.strip_prefix(k) {
                let v = v.split_whitespace().next().unwrap_or("");
                out.push_str(&format!(" {k}{v}"));
                if k == "Threads:" {
                    threads = v.parse().unwrap_or(0);
                }
            }
        }
    }
    Some((out, threads))
}

/// Print `<label> <name>: VmHWM:.. VmRSS:.. Threads:..` for every process.
pub fn mem(label: &str, procs: &[Proc]) {
    for p in procs {
        match proc_status(p.pid()) {
            Some((s, _)) => println!("{label} {}:{s}", p.name),
            None => println!("{label} {}: DEAD", p.name),
        }
    }
}

/// Samples the thread count of every process once a second (a sleeping
/// thread, no busy loop) and keeps the peak.
pub struct Sampler {
    stop: Arc<AtomicBool>,
    peaks: Arc<Mutex<Vec<(String, u64)>>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    pub fn start(procs: &[Proc]) -> Sampler {
        let watch: Vec<(String, u32)> = procs.iter().map(|p| (p.name.clone(), p.pid())).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let peaks = Arc::new(Mutex::new(
            watch
                .iter()
                .map(|(n, _)| (n.clone(), 0))
                .collect::<Vec<_>>(),
        ));
        let (s, pk) = (stop.clone(), peaks.clone());
        let handle = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                let mut pk = pk.lock().unwrap();
                for (i, (_, pid)) in watch.iter().enumerate() {
                    let t = proc_status(*pid).map_or(0, |(_, t)| t);
                    pk[i].1 = pk[i].1.max(t);
                }
            }
        });
        Sampler {
            stop,
            peaks,
            handle: Some(handle),
        }
    }

    /// Stop sampling and print the peaks, sorted by name.
    pub fn finish(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        let mut pk = self.peaks.lock().unwrap().clone();
        pk.sort();
        println!("peak threads seen by the 1 s sampler:");
        for (n, t) in pk {
            println!("  {n} {t}");
        }
    }
}

/// Print `name: alive` / `name: DIED` for each process; returns the dead.
pub fn alive_report(procs: &mut [Proc]) -> Vec<String> {
    let mut dead = Vec::new();
    for p in procs.iter_mut() {
        if p.alive() {
            println!("{}: alive", p.name);
        } else {
            println!("{}: DIED", p.name);
            dead.push(p.name.clone());
        }
    }
    dead
}
