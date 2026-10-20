// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rehearsal harness for tests/real_rehearsal.rs (Rust port of what
//! scripts/real/deploy-sim.sh did with bash, curl, ss, pgrep, jq and timeout).
//! One `Rehearsal` = one fresh simsup root, one port, one log, a list of checks.

#![allow(dead_code)]

use galion_deploy::{sha256, zagreb};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const SLOT_LO: u16 = 19390;
pub const SLOT_HI: u16 = 19399;

pub fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Where the rehearsal binaries live (built by the deploy-sim build, or by
/// tests/real_build.rs).
pub fn sim_bin() -> PathBuf {
    std::env::var("DEPLOY_SIM_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/sim/deploy-sim/bin"))
}

/// Scratch space of this branch's rehearsals (roots and logs).
pub fn sim_home() -> PathBuf {
    std::env::var("DEPLOY_RUST_SIM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/workspace/swarm/sim/deploy-rust"))
}

pub fn galion_deploy() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_galion-deploy"))
}

fn profile_dir() -> PathBuf {
    let me = std::env::current_exe().unwrap();
    me.parent().unwrap().parent().unwrap().to_path_buf()
}

/// The `sm_shim` example (its `data-backup` personality is the galion-backup.sh stub).
pub fn shim() -> PathBuf {
    let p = profile_dir().join("examples/sm_shim");
    if !p.exists() {
        let mut c = Command::new(env!("CARGO"));
        c.args(["build", "-q", "-p", "galion-deploy", "--example", "sm_shim"]);
        if profile_dir().ends_with("release") {
            c.arg("--release");
        }
        assert!(c.status().unwrap().success());
    }
    p
}

pub fn sha16(p: &Path) -> String {
    sha256::file_hex(p)
        .map(|h| h[..16].to_string())
        .unwrap_or_else(|_| "missing".into())
}

pub fn now() -> String {
    zagreb::now()
}

pub fn ms_since(t: Instant) -> u128 {
    t.elapsed().as_millis()
}

/// Raw HTTP/1.0 GET: (status, body). 0 = no answer.
pub fn get(url: &str) -> (u16, String) {
    let Some(rest) = url.strip_prefix("http://") else {
        return (0, String::new());
    };
    let (hp, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let raw = raw_request(
        hp,
        format!("GET {path} HTTP/1.0\r\nHost: {hp}\r\nConnection: close\r\n\r\n").as_bytes(),
        Duration::from_secs(3),
    );
    let text = String::from_utf8_lossy(&raw).to_string();
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|b| b.1.to_string())
        .unwrap_or_default();
    (code, body)
}

pub fn code(url: &str) -> u16 {
    get(url).0
}

/// Send raw bytes, read to EOF (or `limit`).
pub fn raw_request(hostport: &str, req: &[u8], limit: Duration) -> Vec<u8> {
    let Ok(addr) = hostport.parse() else {
        return Vec::new();
    };
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return Vec::new();
    };
    let _ = s.set_read_timeout(Some(limit));
    let _ = s.write_all(req);
    let mut out = Vec::new();
    let t = Instant::now();
    let mut b = [0u8; 8192];
    while t.elapsed() < limit {
        match s.read(&mut b) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&b[..n]),
            Err(_) => break,
        }
    }
    out
}

pub fn wait_code(url: &str, want: u16, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if code(url) == want {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Listening TCP sockets on `port` (from /proc/net/tcp{,6}, state 0A).
pub fn listeners(port: u16) -> usize {
    listeners_in(port, port)
}

pub fn listeners_in(lo: u16, hi: u16) -> usize {
    let mut n = 0;
    for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(t) = fs::read_to_string(f) else {
            continue;
        };
        for l in t.lines().skip(1) {
            let c: Vec<&str> = l.split_whitespace().collect();
            if c.len() < 4 || c[3] != "0A" {
                continue;
            }
            let Some(p) = c[1]
                .rsplit(':')
                .next()
                .and_then(|h| u16::from_str_radix(h, 16).ok())
            else {
                continue;
            };
            if (lo..=hi).contains(&p) {
                n += 1;
            }
        }
    }
    n
}

fn cmdline(pid: u32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|r| {
            r.split(|b| *b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn pids() -> Vec<u32> {
    fs::read_dir("/proc")
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok()))
                .collect()
        })
        .unwrap_or_default()
}

/// PIDs of `<file> --version` processes (exact argv match, no regex quoting games).
pub fn vpids(file: &Path) -> Vec<u32> {
    let f = file.display().to_string();
    pids()
        .into_iter()
        .filter(|p| {
            let c = cmdline(*p);
            c.len() >= 2 && c[0] == f && c[1] == "--version"
        })
        .collect()
}

/// PIDs whose argv[0] lives under `dir` (cleanup of a rehearsal root).
pub fn pids_under(dir: &Path) -> Vec<u32> {
    let d = dir.display().to_string();
    pids()
        .into_iter()
        .filter(|p| {
            cmdline(*p)
                .first()
                .map(|a| a.starts_with(&d))
                .unwrap_or(false)
        })
        .collect()
}

pub fn signal(pid: u32, sig: &str) -> bool {
    Command::new("kill")
        .args([&format!("-{sig}"), &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// `install -m 0755 src dst` (like coreutils: unlink first, so a running binary is
/// replaced, not written into: ETXTBSY)
pub fn install(src: &Path, dst: &Path) {
    let _ = fs::remove_file(dst);
    fs::copy(src, dst)
        .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", src.display(), dst.display()));
    fs::set_permissions(dst, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Top-level keys of a JSON object (enough to count /desk keys; strings and
/// nesting are skipped properly).
pub fn json_top_keys(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut keys = Vec::new();
    let mut depth = 0i32;
    let mut i = 0;
    let mut expect_key = false;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if depth == 1 && expect_key {
                    keys.push(String::from_utf8_lossy(&b[start..i.min(b.len())]).to_string());
                    expect_key = false;
                }
            }
            b'{' | b'[' => {
                depth += 1;
                if depth == 1 {
                    expect_key = true;
                }
            }
            b'}' | b']' => depth -= 1,
            b',' if depth == 1 => expect_key = true,
            _ => {}
        }
        i += 1;
    }
    keys
}

pub struct DeployResult {
    pub rc: i32,
    pub out: String,
    pub ms: u128,
    pub timed_out: bool,
}

pub struct Running {
    pub child: Child,
    pub t0: Instant,
    reader: std::thread::JoinHandle<String>,
    limit: Duration,
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    /// Wait up to the outer limit; on expiry SIGTERM (like coreutils `timeout`),
    /// then SIGKILL after 30 s more. rc 124 = the outer limit fired.
    pub fn finish(mut self) -> DeployResult {
        let mut timed_out = false;
        let mut termed_at: Option<Instant> = None;
        let st = loop {
            if let Ok(Some(st)) = self.child.try_wait() {
                break st;
            }
            if termed_at.is_none() && self.t0.elapsed() > self.limit {
                timed_out = true;
                signal(self.child.id(), "TERM");
                termed_at = Some(Instant::now());
            }
            if termed_at
                .map(|t| t.elapsed() > Duration::from_secs(30))
                .unwrap_or(false)
            {
                let _ = self.child.kill();
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let out = self.reader.join().unwrap_or_default();
        use std::os::unix::process::ExitStatusExt;
        let rc = if timed_out {
            124
        } else {
            st.code().unwrap_or_else(|| 128 + st.signal().unwrap_or(0))
        };
        DeployResult {
            rc,
            out,
            ms: self.t0.elapsed().as_millis(),
            timed_out,
        }
    }
}

pub struct Probe {
    child: Child,
    stop: PathBuf,
}

pub struct Rehearsal {
    pub name: String,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub port: u16,
    pub denv: Vec<(String, String)>,
    pub checks: Vec<(String, bool, String)>,
    pub downtime: Vec<(String, u64)>,
    pub codes: Vec<i32>,
    log: File,
    pub log_path: PathBuf,
    sup: Option<Child>,
}

impl Rehearsal {
    pub fn new(name: &str, port: u16) -> Rehearsal {
        assert!(
            (SLOT_LO..=SLOT_HI).contains(&port),
            "slot 39 uses 19390-19399 only"
        );
        let home = sim_home();
        let root = home.join(format!("root-{name}"));
        let _ = fs::remove_dir_all(&root);
        for d in [
            "units",
            "opt/galion/rust/bin",
            "var/backups/galion",
            "bad",
            "var/tmp/galion-cutover",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::set_permissions(
            root.join("var/tmp/galion-cutover"),
            fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let bin = sim_bin();
        // per-rehearsal copies, so parallel rehearsals never match each other's processes
        for b in ["sim-broken", "sim-hang"] {
            install(&bin.join(b), &root.join("bad").join(b));
        }
        install(&shim(), &root.join("opt/galion/rust/bin/data-backup"));
        fs::create_dir_all(home.join("runs")).unwrap();
        let stamp = zagreb::compact(zagreb::now_secs());
        let log_path = home.join("runs").join(format!("{name}-{stamp}.log"));
        let log = File::create(&log_path).unwrap();
        let mut r = Rehearsal {
            name: name.into(),
            root,
            bin,
            port,
            denv: Vec::new(),
            checks: Vec::new(),
            downtime: Vec::new(),
            codes: Vec::new(),
            log,
            log_path,
            sup: None,
        };
        r.line(&format!(
            "rehearsal {name}: root {} port {port}, galion-deploy {} (sha {}), sim bins {}",
            r.root.display(),
            galion_deploy().display(),
            sha16(&galion_deploy()),
            r.bin.display()
        ));
        r
    }

    pub fn line(&mut self, s: &str) {
        let _ = writeln!(self.log, "{s}");
        let _ = self.log.flush();
        eprintln!("[{}] {s}", self.name);
    }

    pub fn say(&mut self, s: &str) {
        let l = format!("\n[{}] == {s}", now());
        self.line(&l);
    }

    pub fn check(&mut self, name: &str, ok: bool, detail: &str) {
        self.line(&format!(
            "CHECK {}  {name} ({detail})",
            if ok { "PASS" } else { "FAIL" }
        ));
        self.checks.push((name.into(), ok, detail.into()));
    }

    pub fn broken(&self) -> PathBuf {
        self.root.join("bad/sim-broken")
    }
    pub fn hang(&self) -> PathBuf {
        self.root.join("bad/sim-hang")
    }
    pub fn stage(&self) -> PathBuf {
        self.root.join("var/tmp/galion-cutover")
    }
    pub fn bdir(&self) -> PathBuf {
        self.root.join("var/backups/galion/deploy")
    }
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn ctl(&mut self, args: &[&str]) -> (i32, String) {
        let o = Command::new(self.bin.join("simsup"))
            .arg("--root")
            .arg(&self.root)
            .arg("ctl")
            .args(args)
            .output()
            .unwrap();
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr),
        )
    }

    /// `ctl` and log it like the bash `x` helper.
    pub fn x_ctl(&mut self, args: &[&str]) -> i32 {
        let (rc, out) = self.ctl(args);
        self.line(&format!("$ ctl {}", args.join(" ")));
        for l in out.lines() {
            self.line(&format!("  {l}"));
        }
        self.line(&format!("  -> exit={rc}"));
        rc
    }

    pub fn active(&mut self, unit: &str) -> String {
        self.ctl(&["is-active", unit]).1.trim().to_string()
    }

    pub fn main_pid(&mut self, unit: &str) -> u32 {
        let (_, s) = self.ctl(&["show", unit]);
        s.lines()
            .find_map(|l| l.strip_prefix("MainPID="))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    pub fn journal_count(&self, unit: &str, needle: &str) -> usize {
        fs::read_to_string(self.root.join("journal").join(format!("{unit}.log")))
            .map(|t| t.matches(needle).count())
            .unwrap_or(0)
    }

    pub fn sup_start(&mut self) {
        let errlog = File::create(self.root.join("simsup.log")).unwrap();
        let c = Command::new(self.bin.join("simsup"))
            .arg("--root")
            .arg(&self.root)
            .arg("daemon")
            .stdout(Stdio::null())
            .stderr(errlog)
            .spawn()
            .unwrap();
        let pid = c.id();
        self.sup = Some(c);
        let sock = self.root.join("ctl.sock");
        let t = Instant::now();
        while !sock.exists() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(100));
        }
        self.line(&format!("simsup pid {pid}, root {}", self.root.display()));
    }

    pub fn sup_stop(&mut self) {
        if let Some(mut c) = self.sup.take() {
            let _ = self.ctl(&["shutdown"]);
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(15) {
                if let Ok(Some(_)) = c.try_wait() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
        let n = listeners(self.port);
        self.line(&format!(
            "simsup stopped; leftover listeners on :{}: {n}",
            self.port
        ));
    }

    /// Start a galion-deploy (direct backend, sim paths) in the background.
    pub fn spawn_deploy(&mut self, limit: u64, args: &[&str], extra: &[(&str, &str)]) -> Running {
        let ctl = format!(
            "{} --root {} ctl",
            self.bin.join("simsup").display(),
            self.root.display()
        );
        let mut note = String::new();
        for (k, v) in extra {
            note += &format!(", {k}={v}");
        }
        self.line(&format!(
            "$ galion-deploy {}   (direct backend{note}, outer limit {limit}s)",
            args.join(" ")
        ));
        let mut c = Command::new(galion_deploy());
        c.args(args)
            .envs(self.denv.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .env("GALION_DEPLOY_SM", "direct")
            .env("GALION_DEPLOY_CTL", ctl)
            .env("GALION_DEPLOY_BACKUP_DIR", self.bdir())
            .env(
                "GALION_DEPLOY_DATA_BACKUP",
                self.root.join("opt/galion/rust/bin/data-backup"),
            )
            .env("SHIM_DATA_SLEEP", "3")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra {
            c.env(k, v);
        }
        let mut child = c.spawn().unwrap();
        let mut so = child.stdout.take().unwrap();
        let mut se = child.stderr.take().unwrap();
        let reader = std::thread::spawn(move || {
            let e = std::thread::spawn(move || {
                let mut s = String::new();
                let _ = se.read_to_string(&mut s);
                s
            });
            let mut s = String::new();
            let _ = so.read_to_string(&mut s);
            s + &e.join().unwrap_or_default()
        });
        Running {
            child,
            t0: Instant::now(),
            reader,
            limit: Duration::from_secs(limit),
        }
    }

    pub fn finish(&mut self, r: Running) -> DeployResult {
        let d = r.finish();
        for l in d.out.lines() {
            self.line(l);
        }
        self.line(&format!(
            "  -> exit={} elapsed_ms={}{}",
            d.rc,
            d.ms,
            if d.timed_out {
                " (outer limit fired)"
            } else {
                ""
            }
        ));
        self.codes.push(d.rc);
        d
    }

    pub fn deploy(&mut self, limit: u64, args: &[&str], extra: &[(&str, &str)]) -> DeployResult {
        let r = self.spawn_deploy(limit, args, extra);
        self.finish(r)
    }

    pub fn probe_start(&mut self, path: &str, every_ms: u32) -> Probe {
        let stop = self.root.join("probe.stop");
        let _ = fs::remove_file(&stop);
        let child = Command::new(self.bin.join("simsup"))
            .args([
                "probe",
                &format!("127.0.0.1:{}", self.port),
                path,
                &every_ms.to_string(),
                stop.to_str().unwrap(),
                "300",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        Probe { child, stop }
    }

    /// Longest outage in ms; a window that never ended in an OK answer counts with
    /// its whole fail span (same rule as the bash harness).
    pub fn probe_stop(&mut self, p: Probe) -> u64 {
        fs::write(&p.stop, "").unwrap();
        let out = p.child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let mut down = 0u64;
        let mut never = false;
        for l in text.lines() {
            self.line(&format!("  probe| {l}"));
            let field = |k: &str| {
                l.split_whitespace()
                    .find_map(|w| w.strip_prefix(k))
                    .and_then(|v| v.parse::<u64>().ok())
            };
            if let Some(v) = field("until_ok_ms=") {
                down = down.max(v);
            }
            if l.contains("next_ok=never") {
                never = true;
                down = down.max(field("fail_span_ms=").unwrap_or(0));
            }
        }
        if never {
            self.line("  probe| NOTE: service was still down when the probe stopped");
        }
        down
    }

    pub fn downtime(&mut self, label: &str, ms: u64) {
        self.line(&format!("DOWNTIME {} {label} {ms} ms", self.name));
        self.downtime.push((label.into(), ms));
    }

    pub fn latest_backup(&self, unit: &str) -> Option<PathBuf> {
        let mut v: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(self.bdir())
            .ok()?
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.starts_with(&format!("{unit}-")) && n.ends_with(".bin")
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        v.sort();
        v.pop().map(|x| x.1)
    }

    pub fn backups_of(&self, unit: &str) -> Vec<PathBuf> {
        fs::read_dir(self.bdir())
            .map(|r| {
                r.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let n = p.file_name().unwrap().to_string_lossy().to_string();
                        n.starts_with(&format!("{unit}-")) && n.ends_with(".bin")
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Print the summary; returns the number of failed checks.
    pub fn summary(&mut self) -> usize {
        let pass = self.checks.iter().filter(|c| c.1).count();
        let fail = self.checks.len() - pass;
        let mut s = format!("\n== summary {}: {pass} pass, {fail} fail", self.name);
        for (n, ok, d) in self.checks.clone() {
            s += &format!("\n  {n}|{}|{d}", if ok { "PASS" } else { "FAIL" });
        }
        for (l, ms) in self.downtime.clone() {
            s += &format!("\n  DOWNTIME {l} {ms} ms");
        }
        self.line(&s);
        self.line(&format!("log: {}", self.log_path.display()));
        fail
    }
}

impl Drop for Rehearsal {
    fn drop(&mut self) {
        if self.sup.is_some() {
            self.sup_stop();
        }
        // anything still running from this root (stray --version, orphans)
        for p in pids_under(&self.root) {
            signal(p, "KILL");
        }
        for p in vpids(&self.root) {
            signal(p, "KILL");
        }
    }
}
