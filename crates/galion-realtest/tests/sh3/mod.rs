// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Shared plumbing for the `real_sh_*` tests that replaced the last third of
//! `scripts/real/*.sh` (realscript-port-3, Rust-only rule 2026-10-20 20:41).
//!
//! Environment (same names as realscript-port-1/-2 so one invocation runs all):
//! * `GALION_REAL=1`        run the real tests (otherwise they print `skipped` and pass)
//! * `GALION_REAL_VPS=1`    also run the READ-ONLY live VPS probes (ssh, GET only)
//! * `GALION_REAL_VPS_HOST` ssh alias of the VPS (default `slavik-vps`)
//! * `GALION_REAL_BIN_DIR`  prebuilt release dir (`<target>/release`); no cargo build
//! * `GALION_REAL_JOBS`     `-j` for the build fallback (default 2)
//!
//! The build fallback builds into `$CARGO_TARGET_DIR/real-bins`, because the
//! outer `cargo test` holds the lock on `$CARGO_TARGET_DIR` while tests run.
//! It runs plain `nice -n 10 cargo` (not `cargo-slot`): the outer test run
//! already holds a build slot, and a nested slot wait could deadlock.

#![allow(dead_code)]

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Swarm wave-4 slot 114 owns 127.0.0.1:19840-19849.
pub const PORT_BASE: u16 = 19840;

pub fn enabled(test: &str) -> bool {
    let on = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !on {
        eprintln!("{test}: skipped (set GALION_REAL=1)");
    }
    on
}

pub fn vps_enabled() -> bool {
    std::env::var("GALION_REAL_VPS").as_deref() == Ok("1")
}

pub fn vps_host() -> String {
    std::env::var("GALION_REAL_VPS_HOST").unwrap_or_else(|_| "slavik-vps".into())
}

pub fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

pub fn say(msg: impl AsRef<str>) {
    println!("[{}] {}", stamp(), msg.as_ref());
}

pub fn repo_root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    p.canonicalize().unwrap_or(p)
}

pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| "cargo".into())
}

/// `nice -n 10 cargo <args>` into `$CARGO_TARGET_DIR/real-bins`, panicking on failure.
pub fn cargo_build(args: &[&str], env: &[(String, String)]) -> PathBuf {
    let td = target_dir().join("real-bins");
    let jobs = std::env::var("GALION_REAL_JOBS").unwrap_or_else(|_| "2".into());
    say(format!(
        "build: nice -n 10 cargo {} -j {jobs}",
        args.join(" ")
    ));
    let mut c = Command::new("nice");
    c.arg("-n")
        .arg("10")
        .arg(cargo())
        .args(args)
        .arg("-j")
        .arg(&jobs)
        .current_dir(repo_root())
        .env("CARGO_TARGET_DIR", &td)
        .env("CARGO_INCREMENTAL", "0");
    for (k, v) in env {
        c.env(k, v);
    }
    let st = c.status().expect("run cargo");
    assert!(st.success(), "cargo {} failed", args.join(" "));
    td
}

/// Release dir for `pkgs` (optionally cross `triple`). Uses
/// `GALION_REAL_BIN_DIR` (cross builds next to it in `../<triple>/release`)
/// or builds with `--release --locked`.
pub fn release_bins(
    triple: Option<&str>,
    build_args: &[&str],
    env: &[(String, String)],
) -> PathBuf {
    if let Some(d) = std::env::var_os("GALION_REAL_BIN_DIR") {
        let d = PathBuf::from(d);
        return match (triple, d.parent()) {
            (Some(t), Some(base)) => base.join(t).join("release"),
            _ => d,
        };
    }
    let mut args = vec!["build", "--release", "--locked"];
    args.extend_from_slice(build_args);
    if let Some(t) = triple {
        args.extend_from_slice(&["--target", t]);
    }
    let td = cargo_build(&args, env);
    match triple {
        Some(t) => td.join(t).join("release"),
        None => td.join("release"),
    }
}

pub fn need(p: &Path) -> PathBuf {
    assert!(
        p.exists(),
        "missing {} (build it or set GALION_REAL_BIN_DIR)",
        p.display()
    );
    p.to_path_buf()
}

pub fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// A scratch dir removed on drop.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
    pub fn put(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::env::var("GALION_REAL_KEEP").as_deref() != Ok("1") {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Children killed and reaped on drop (also on a failed assert).
#[derive(Default)]
pub struct Procs(pub Vec<(String, Child)>);

impl Procs {
    pub fn spawn(&mut self, name: &str, cmd: &mut Command) -> u32 {
        let c = cmd
            .stdin(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {name}: {e}"));
        let pid = c.id();
        self.0.push((name.to_string(), c));
        pid
    }
    pub fn stop(&mut self, name: &str) {
        if let Some(i) = self.0.iter().position(|(n, _)| n == name) {
            let (_, mut c) = self.0.remove(i);
            let _ = c.kill();
            let _ = c.wait();
        }
    }
    pub fn stop_all(&mut self) {
        for (_, mut c) in self.0.drain(..) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Procs {
    fn drop(&mut self) {
        self.stop_all();
    }
}

pub fn port_open(port: u16) -> bool {
    let a: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok()
}

pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    cond()
}

pub fn wait_port(port: u16, timeout: Duration) -> bool {
    wait_until(timeout, || port_open(port))
}

pub fn require_free(ports: &[u16]) {
    for p in ports {
        assert!(
            !port_open(*p),
            "port {p} is already in use; refusing to run"
        );
    }
}

pub fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("run command")
}

pub fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// `VmRSS` / `VmHWM` of a pid from /proc.
pub fn proc_field(pid: u32, field: &str) -> Option<String> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix(&format!("{field}:")))
        .map(|v| v.trim().to_string())
}

pub fn alive(pid: u32) -> bool {
    // A zombie still has /proc/<pid>; treat state Z as gone.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => !s
            .rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z'),
        Err(_) => false,
    }
}

/// Pids whose /proc/<pid>/exe is `bin` (leftover check).
pub fn pids_running(bin: &Path) -> Vec<u32> {
    let want = bin.canonicalize().unwrap_or(bin.to_path_buf());
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if std::fs::read_link(e.path().join("exe")).is_ok_and(|p| p == want) && alive(pid) {
                v.push(pid);
            }
        }
    }
    v
}

/// ELF facts without readelf: (machine name, has PT_INTERP, NEEDED count unknown -> PT_DYNAMIC present).
pub fn elf_facts(bin: &Path) -> (String, bool, bool) {
    let b = std::fs::read(bin).expect("read ELF");
    assert_eq!(&b[..4], b"\x7fELF", "{} is not ELF", bin.display());
    let is64 = b[4] == 2;
    let le = |o: usize, n: usize| -> u64 {
        let mut v = 0u64;
        for i in (0..n).rev() {
            v = (v << 8) | u64::from(b[o + i]);
        }
        v
    };
    let machine = match le(18, 2) {
        0x3e => "x86-64",
        0xb7 => "AArch64",
        0x28 => "ARM",
        m => return (format!("0x{m:x}"), false, false),
    }
    .to_string();
    let (phoff, phentsize, phnum) = if is64 {
        (le(32, 8) as usize, le(54, 2) as usize, le(56, 2) as usize)
    } else {
        (le(28, 4) as usize, le(42, 2) as usize, le(44, 2) as usize)
    };
    let (mut interp, mut dynamic) = (false, false);
    for i in 0..phnum {
        match le(phoff + i * phentsize, 4) {
            3 => interp = true,
            2 => dynamic = true,
            _ => {}
        }
    }
    (machine, interp, dynamic)
}

/// Raw bytes request/response over loopback (server closes the connection).
pub fn raw(port: u16, req: &[u8], timeout: Duration) -> Vec<u8> {
    use std::io::{Read, Write};
    let a: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let Ok(mut s) = TcpStream::connect_timeout(&a, timeout) else {
        return Vec::new();
    };
    s.set_read_timeout(Some(timeout)).unwrap();
    let _ = s.write_all(req);
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    buf
}

pub fn status_of(resp: &[u8]) -> u16 {
    String::from_utf8_lossy(resp)
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

pub fn body_of(resp: &[u8]) -> String {
    let t = String::from_utf8_lossy(resp);
    t.split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default()
}

pub fn get(port: u16, path: &str) -> Vec<u8> {
    raw(
        port,
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
        Duration::from_secs(20),
    )
}

/// Tally of named checks; prints PASS/FAIL lines and asserts at the end.
#[derive(Default)]
pub struct Checks {
    pub pass: usize,
    pub fail: Vec<String>,
}

impl Checks {
    pub fn ok(&mut self, label: &str, ok: bool, detail: &str) -> bool {
        if ok {
            self.pass += 1;
            println!("  PASS {label} {detail}");
        } else {
            println!("  FAIL {label} {detail}");
            self.fail.push(label.to_string());
        }
        ok
    }
    pub fn finish(&self, test: &str) {
        say(format!(
            "{test}: {} passed, {} failed",
            self.pass,
            self.fail.len()
        ));
        assert!(self.fail.is_empty(), "{test} failed: {:?}", self.fail);
    }
}
