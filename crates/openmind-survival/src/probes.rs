// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The probes. Each returns a [`ProbeResult`] stamped by `zagreb-time`.

use crate::{level_for_percent, Level};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    /// Europe/Zagreb RFC 3339 with offset.
    pub ts: String,
}

impl ProbeResult {
    pub fn new(name: &str, ok: bool, detail: impl Into<String>) -> Self {
        ProbeResult {
            name: name.to_string(),
            ok,
            detail: detail.into(),
            ts: zagreb_time::now_rfc3339(),
        }
    }
}

/// Injectable command runner: program, argv -> stdout or error text.
pub trait Runner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String>;
}

/// Real runner, no shell. Returns stdout even on non-zero exit (is-active
/// prints its state and exits non-zero when inactive).
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        Command::new(program)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .map_err(|e| e.to_string())
    }
}

/// HTTP/1.0 GET to 127.0.0.1:port, 2.5 s timeouts, no TLS. Ok when 2xx/3xx.
pub fn check_http(name: &str, port: u16, path: &str) -> ProbeResult {
    match http_status(port, path) {
        Ok(code) => ProbeResult::new(name, (200..400).contains(&code), format!("status {code}")),
        Err(e) => ProbeResult::new(name, false, e),
    }
}

fn http_status(port: u16, path: &str) -> Result<u16, String> {
    let t = Duration::from_millis(2500);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, t).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(t)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(t)).map_err(|e| e.to_string())?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut buf = [0u8; 256];
    let mut got = 0;
    while got < buf.len() && !buf[..got].contains(&b'\n') {
        match s.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    let line = String::from_utf8_lossy(&buf[..got]);
    let line = line.lines().next().unwrap_or("");
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("bad status line: {line}"))
}

/// `systemctl is-active <unit>`; ok only when stdout is `active`.
pub fn check_unit(runner: &dyn Runner, unit: &str) -> ProbeResult {
    let name = format!("unit:{unit}");
    match runner.run("systemctl", &["is-active", unit]) {
        Ok(out) => {
            let state = out.trim();
            ProbeResult::new(&name, state == "active", state.to_string())
        }
        Err(e) => ProbeResult::new(&name, false, e),
    }
}

/// Parse /proc/meminfo text into used percent (MemTotal vs MemAvailable).
pub fn parse_meminfo(text: &str) -> Option<f64> {
    let field = |key: &str| -> Option<f64> {
        text.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let total = field("MemTotal:")?;
    let avail = field("MemAvailable:")?;
    if total <= 0.0 {
        return None;
    }
    Some((total - avail) / total * 100.0)
}

/// Parse /proc/loadavg text into (1m, 5m, 15m).
pub fn parse_loadavg(text: &str) -> Option<(f64, f64, f64)> {
    let mut it = text.split_whitespace();
    Some((
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    ))
}

fn level_result(name: &str, pct: f64, what: &str) -> ProbeResult {
    let lv = level_for_percent(pct);
    let tag = match lv {
        Level::Pass => "pass",
        Level::Warn => "warn",
        Level::Crit => "crit",
    };
    ProbeResult::new(name, lv == Level::Pass, format!("{what} {pct:.1}% {tag}"))
}

/// Memory from meminfo text (caller reads /proc/meminfo).
pub fn check_meminfo(text: &str) -> ProbeResult {
    match parse_meminfo(text) {
        Some(p) => level_result("mem", p, "used"),
        None => ProbeResult::new("mem", false, "unparseable meminfo"),
    }
}

/// Load from loadavg text; percent is load1 per core.
pub fn check_loadavg(text: &str, cores: usize) -> ProbeResult {
    match parse_loadavg(text) {
        Some((l1, _, _)) => {
            let pct = l1 / cores.max(1) as f64 * 100.0;
            let mut r = level_result("load", pct, "load1/core");
            r.detail = format!("{} (load1 {l1:.2}, cores {cores})", r.detail);
            r
        }
        None => ProbeResult::new("load", false, "unparseable loadavg"),
    }
}

/// Parse `df -P` output into used percent of the first data row.
pub fn parse_df(text: &str) -> Option<f64> {
    let row = text.lines().nth(1)?;
    let cap = row.split_whitespace().nth(4)?;
    cap.trim_end_matches('%').parse().ok()
}

/// Disk usage through `df -P <path>` (read-only, no shell).
pub fn check_disk(runner: &dyn Runner, path: &str) -> ProbeResult {
    let name = format!("disk:{path}");
    match runner.run("df", &["-P", path]) {
        Ok(out) => match parse_df(&out) {
            Some(p) => level_result(&name, p, "used"),
            None => ProbeResult::new(&name, false, "unparseable df output"),
        },
        Err(e) => ProbeResult::new(&name, false, e),
    }
}

/// Ok when the file exists and its mtime is at most `max_age_secs` old.
pub fn check_freshness(path: &Path, max_age_secs: u64) -> ProbeResult {
    let name = format!("fresh:{}", path.display());
    let mtime = match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) => return ProbeResult::new(&name, false, e.to_string()),
    };
    let age = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or_default()
        .as_secs();
    ProbeResult::new(
        &name,
        age <= max_age_secs,
        format!("age {age}s max {max_age_secs}s"),
    )
}

/// Scan `<proc_root>/*/comm` for a process named `cloudflared`.
pub fn check_cloudflared(proc_root: &Path) -> ProbeResult {
    let rd = match std::fs::read_dir(proc_root) {
        Ok(r) => r,
        Err(e) => return ProbeResult::new("cloudflared", false, e.to_string()),
    };
    for ent in rd.flatten() {
        let n = ent.file_name();
        let n = n.to_string_lossy();
        if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(c) = std::fs::read_to_string(ent.path().join("comm")) {
            if c.trim() == "cloudflared" {
                return ProbeResult::new("cloudflared", true, format!("pid {n}"));
            }
        }
    }
    ProbeResult::new("cloudflared", false, "not running")
}

/// Result of one [`http_get`]: elapsed milliseconds and status + body prefix.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpOutcome {
    pub ms: f64,
    /// `(status code, first bytes of the body)` or an error text.
    pub result: Result<(u16, String), String>,
}

/// Plain HTTP/1.0 GET to `addr` (callers pass a loopback default from their
/// config). Reads at most 4 KiB: enough for the status line and a short body.
pub fn http_get(addr: SocketAddr, path: &str, timeout: Duration) -> HttpOutcome {
    let started = std::time::Instant::now();
    let result = http_get_inner(addr, path, timeout);
    HttpOutcome {
        ms: started.elapsed().as_secs_f64() * 1000.0,
        result,
    }
}

fn http_get_inner(addr: SocketAddr, path: &str, t: Duration) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect_timeout(&addr, t).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(t)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(t)).map_err(|e| e.to_string())?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut buf = vec![0u8; 4096];
    let mut got = 0;
    while got < buf.len() {
        match s.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if got == 0 => return Err(format!("read: {e}")),
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf[..got]);
    let line = text.lines().next().unwrap_or("");
    let code = line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| format!("bad status line: {}", truncate(line, 80)))?;
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| truncate(b, 240).to_string())
        .unwrap_or_default();
    Ok((code, body))
}

/// Cut a string to at most `n` bytes on a char boundary.
pub fn truncate(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let mut i = n;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

/// desk_heal.py `probe()` semantics: ok on a code in `ok_codes`; a 503 whose
/// body mentions `loading` also passes when `loading_ok`. Detail strings:
/// `200`, `loading`, `http <code>`, or the error cut to 160 bytes.
pub fn verdict(
    result: &Result<(u16, String), String>,
    ok_codes: &[u16],
    loading_ok: bool,
) -> (bool, String) {
    match result {
        Ok((code, _)) if ok_codes.contains(code) => (true, code.to_string()),
        Ok((503, body)) if loading_ok && body.contains("loading") => (true, "loading".into()),
        Ok((code, _)) => (false, format!("http {code}")),
        Err(e) => (false, truncate(e, 160).to_string()),
    }
}
