// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Building blocks for the Rust ports of the old `scripts/real/*.sh`
//! real-life tests (swarm/realscript-port-*). Each port is a gated
//! integration test under `crates/galion-realtest/tests/real_sh_*.rs` that
//! builds the release binaries it needs, starts them on loopback, probes
//! them with real requests, prints PASS/FAIL lines with Zagreb (+02:00 /
//! +01:00) stamps and tears everything down (also on panic, via `Drop`).
//!
//! Environment:
//! * `GALION_REAL=1`              run at all (otherwise every port skips).
//! * `GALION_REAL_SKIP_BUILD=1`   do not run `cargo build`; binaries must exist.
//! * `GALION_REAL_JOBS`           `cargo build -j` (default 2, CPU-polite).
//! * `GALION_REAL_PORT_BASE`      move a port's whole port block (each port
//!   documents its default block and the offsets it uses, all < 10).
//! * `CARGO_TARGET_DIR`           where release binaries are built / found.

use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// True when `GALION_REAL=1`; otherwise prints a SKIP line.
pub fn enabled(name: &str) -> bool {
    if std::env::var("GALION_REAL").as_deref() == Ok("1") {
        return true;
    }
    eprintln!("SKIP {name}: set GALION_REAL=1 (real-life test: builds release binaries, binds loopback ports)");
    false
}

/// Zagreb wall clock with explicit offset.
pub fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

/// Print a timestamped progress line.
pub fn say(msg: &str) {
    println!("[{}] {msg}", stamp());
}

/// Repository root (two levels above this crate).
pub fn repo_root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    p.canonicalize().unwrap_or(p)
}

/// `CARGO_TARGET_DIR` or `<repo>/target`.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

/// `<target>/release/<name>`.
pub fn release_bin(name: &str) -> PathBuf {
    target_dir().join("release").join(name)
}

/// Port block: `GALION_REAL_PORT_BASE` or the port's historic default.
pub fn port_base(default: u16) -> u16 {
    match std::env::var("GALION_REAL_PORT_BASE") {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("GALION_REAL_PORT_BASE={v} is not a port")),
        _ => default,
    }
}

/// The cargo that runs this test (falls back to `cargo` on PATH).
pub fn cargo() -> PathBuf {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

/// `nice -n 10 cargo build --release -j N <args>` in the repo root (or
/// `dir`), with `extra_env`. Skipped with `GALION_REAL_SKIP_BUILD=1`.
pub fn build_in(dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) {
    if std::env::var("GALION_REAL_SKIP_BUILD").as_deref() == Ok("1") {
        say(&format!(
            "build skipped (GALION_REAL_SKIP_BUILD=1): {args:?}"
        ));
        return;
    }
    let jobs = std::env::var("GALION_REAL_JOBS").unwrap_or_else(|_| "2".into());
    say(&format!("build (release, nice -n 10, -j {jobs}): {args:?}"));
    let st = Command::new("nice")
        .args(["-n", "10"])
        .arg(cargo())
        .args(["build", "-q", "--release", "-j", &jobs])
        .args(args)
        .current_dir(dir)
        .env("CARGO_INCREMENTAL", "0")
        .envs(extra_env.iter().copied())
        .status()
        .expect("run cargo build");
    assert!(st.success(), "cargo build {args:?} failed");
}

/// [`build_in`] at the repo root.
pub fn build(args: &[&str]) {
    build_in(&repo_root(), args, &[]);
}

/// Panics unless the file exists and is executable-sized.
pub fn need_bin(p: &Path) -> PathBuf {
    let md = fs::metadata(p).unwrap_or_else(|_| panic!("missing release binary {}", p.display()));
    println!("  {} bytes  {}", md.len(), p.display());
    p.to_path_buf()
}

/// Fresh private scratch dir under the system temp dir.
pub fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now() // repo-check: allow (unique dir name, not a timestamp)
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&d, fs::Permissions::from_mode(0o700));
    }
    d
}

/// Removes a directory on drop (unless `GALION_REAL_KEEP=1`).
pub struct TempDir(pub PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        if std::env::var("GALION_REAL_KEEP").as_deref() == Ok("1") {
            eprintln!("GALION_REAL_KEEP=1: leaving {}", self.0.display());
        } else {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

/// True when something accepts on 127.0.0.1:`port`.
pub fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// Panics when any of `ports` is already taken (refuse to start).
pub fn require_free(ports: &[u16]) {
    for &p in ports {
        assert!(
            !port_open(p),
            "port {p} is already in use; refusing to start"
        );
    }
}

/// Wait until `cond` holds, polling every 100 ms, up to `timeout`.
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if cond() {
            return true;
        }
        if Instant::now() > end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Wait until 127.0.0.1:`port` accepts (up to `timeout`).
pub fn wait_port(port: u16, timeout: Duration) -> bool {
    wait_until(timeout, || port_open(port))
}

/// Children killed (SIGTERM via `kill`, then `Child::kill`) and reaped on drop.
#[derive(Default)]
pub struct Procs {
    pub children: Vec<(String, Child)>,
    /// Extra pids (not our direct children, e.g. a restarted service).
    pub pids: Vec<u32>,
}

impl Procs {
    pub fn new() -> Procs {
        Procs::default()
    }

    /// Spawn and remember a child; returns its pid.
    pub fn spawn(&mut self, name: &str, mut cmd: Command) -> u32 {
        let child = cmd.spawn().unwrap_or_else(|e| panic!("start {name}: {e}"));
        let pid = child.id();
        println!("  started {name} pid {pid}");
        self.children.push((name.to_string(), child));
        pid
    }

    /// The child by name.
    pub fn get(&mut self, name: &str) -> Option<&mut Child> {
        self.children
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c)
    }

    /// Kill and reap one child now (SIGTERM, then SIGKILL after 5 s).
    pub fn stop(&mut self, name: &str) -> Option<std::process::ExitStatus> {
        let i = self.children.iter().position(|(n, _)| n == name)?;
        let (_, mut c) = self.children.remove(i);
        Some(term_wait(&mut c, Duration::from_secs(5)))
    }

    /// Process RSS lines (`name pid kB`) for the children still alive.
    pub fn rss(&self) -> Vec<String> {
        self.children
            .iter()
            .map(|(n, c)| {
                format!(
                    "{n} {} kB",
                    rss_kb(c.id()).map_or("-".into(), |k| k.to_string())
                )
            })
            .collect()
    }
}

impl Drop for Procs {
    fn drop(&mut self) {
        for p in &self.pids {
            signal(*p, "TERM");
        }
        for (_, c) in &mut self.children {
            term_wait(c, Duration::from_secs(3));
        }
    }
}

/// `kill -SIG pid` (quiet). True when the signal was delivered.
pub fn signal(pid: u32, sig: &str) -> bool {
    Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// True while `pid` exists (signal 0).
pub fn alive(pid: u32) -> bool {
    signal(pid, "0")
}

/// SIGTERM, wait up to `grace`, then SIGKILL; returns the exit status.
pub fn term_wait(c: &mut Child, grace: Duration) -> std::process::ExitStatus {
    if let Ok(Some(st)) = c.try_wait() {
        return st;
    }
    signal(c.id(), "TERM");
    let end = Instant::now() + grace;
    while Instant::now() < end {
        if let Ok(Some(st)) = c.try_wait() {
            return st;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = c.kill();
    c.wait().expect("reap child")
}

/// Wait up to `timeout` for a child to exit by itself.
pub fn wait_exit(c: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Ok(Some(st)) = c.try_wait() {
            return Some(st);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// VmRSS of a pid in kB.
pub fn rss_kb(pid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse().ok())
}

/// `/proc/<pid>/status` field (e.g. "Threads", "VmHWM") first token.
pub fn proc_field(pid: u32, field: &str) -> Option<String> {
    let s = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let key = format!("{field}:");
    s.lines()
        .find_map(|l| l.strip_prefix(key.as_str()))
        .map(|v| v.trim().to_string())
}

/// Run a command to completion, capturing output.
pub fn run(mut cmd: Command) -> Output {
    cmd.stdin(Stdio::null());
    cmd.output().expect("run command")
}

/// Exit code or -1 for a signal.
pub fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

/// UTF-8 (lossy) stdout.
pub fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// UTF-8 (lossy) stderr.
pub fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Read a file to string ("" when missing).
pub fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

/// Last `n` lines of a file (for failure diagnostics).
pub fn tail(p: &Path, n: usize) -> String {
    let s = read(p);
    let v: Vec<&str> = s.lines().collect();
    v[v.len().saturating_sub(n)..].join("\n")
}

/// File mode bits (e.g. 0o600) as octal text "600".
#[cfg(unix)]
pub fn mode(p: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).map_or("-".into(), |m| {
        format!("{:o}", m.permissions().mode() & 0o777)
    })
}

/// Random hex from /dev/urandom (throwaway test keys; never printed).
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("read /dev/urandom");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Path dotted lookup in JSON: `j(&v, "a.b.0.c")`.
pub fn j<'a>(v: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    let mut cur = v;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        cur = match part.parse::<usize>() {
            Ok(i) if cur.is_array() => &cur[i],
            _ => &cur[part],
        };
    }
    cur
}

/// JSON value as plain text (strings unquoted, null as "null").
pub fn jt(v: &serde_json::Value, path: &str) -> String {
    match j(v, path) {
        serde_json::Value::String(s) => s.clone(),
        // jq -r prints 24.0 as 24; keep that so ported checks compare alike.
        serde_json::Value::Number(n) if !n.is_i64() && !n.is_u64() => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// Parse JSON or `Null`.
pub fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s.trim()).unwrap_or(serde_json::Value::Null)
}

/// PASS/FAIL recorder. Every check prints one line; [`Checks::finish`]
/// panics with the failed labels so `cargo test` reports the port as failed.
#[derive(Default)]
pub struct Checks {
    pub pass: usize,
    pub failed: Vec<String>,
}

impl Checks {
    pub fn new() -> Checks {
        Checks::default()
    }

    /// Record one boolean check with a detail string.
    pub fn ok(&mut self, label: &str, ok: bool, detail: &str) -> bool {
        if ok {
            self.pass += 1;
            println!(
                "  PASS {label}{}",
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({detail})")
                }
            );
        } else {
            println!(
                "  FAIL {label}{}",
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            );
            self.failed.push(label.to_string());
        }
        ok
    }

    /// Record `want == got`.
    pub fn eq(&mut self, label: &str, want: impl AsRef<str>, got: impl AsRef<str>) -> bool {
        let (w, g) = (want.as_ref(), got.as_ref());
        if w == g {
            self.ok(label, true, g)
        } else {
            self.ok(label, false, &format!("want {w:?}, got {g:?}"))
        }
    }

    /// Record "`hay` contains `needle`".
    pub fn has(&mut self, label: &str, needle: &str, hay: &str) -> bool {
        let ok = hay.contains(needle);
        let shown: String = hay.chars().take(240).collect();
        self.ok(
            label,
            ok,
            &if ok {
                needle.to_string()
            } else {
                format!("want {needle:?} in {shown:?}")
            },
        )
    }

    /// Print the totals and panic when anything failed.
    pub fn finish(&self, name: &str) {
        say(&format!(
            "{name}: {} passed, {} failed",
            self.pass,
            self.failed.len()
        ));
        assert!(
            self.failed.is_empty(),
            "{name}: failed checks: {:?}",
            self.failed
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_paths() {
        let v = json(r#"{"a":{"b":[{"c":"x"},{"c":2}]}}"#);
        assert_eq!(jt(&v, "a.b.0.c"), "x");
        assert_eq!(jt(&v, "a.b.1.c"), "2");
        assert_eq!(jt(&v, "a.nope"), "null");
    }

    #[test]
    fn checks_count() {
        let mut c = Checks::new();
        c.eq("same", "1", "1");
        c.has("sub", "b", "abc");
        assert_eq!(c.pass, 2);
        c.eq("diff", "1", "2");
        assert_eq!(c.failed, vec!["diff".to_string()]);
    }

    #[test]
    fn port_base_default() {
        if std::env::var("GALION_REAL_PORT_BASE").is_err() {
            assert_eq!(port_base(19160), 19160);
        }
    }

    #[test]
    fn random_hex_len() {
        assert_eq!(random_hex(8).len(), 16);
    }
}
