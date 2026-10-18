// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Small std-only helpers: clock, bounded subprocess runs, a loopback HTTP
//! client and atomic file writes.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Unix seconds now.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Europe/Zagreb RFC 3339 for a unix second.
pub fn ts(secs: u64) -> String {
    zagreb_time::from_unix(secs as i64)
}

/// Output of a finished command.
#[derive(Debug, Clone)]
pub struct CmdOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Run a command with a hard timeout (the child is killed on expiry). Stdin is
/// closed. Returns `None` when it could not be spawned or timed out.
pub fn run(prog: &str, args: &[&str], timeout: Duration) -> Option<CmdOut> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Drain pipes on threads so a chatty child cannot block on a full pipe.
    let mut so = child.stdout.take()?;
    let mut se = child.stderr.take()?;
    let t_out = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = so.by_ref().take(8 << 20).read_to_end(&mut v);
        v
    });
    let t_err = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = se.by_ref().take(1 << 20).read_to_end(&mut v);
        v
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = t_out.join().unwrap_or_default();
    let err = t_err.join().unwrap_or_default();
    let status = status?;
    Some(CmdOut {
        code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    })
}

/// A parsed HTTP reply from [`http`].
#[derive(Debug, Clone)]
pub struct HttpReply {
    pub code: u16,
    pub body: String,
    pub ms: u64,
}

/// Minimal HTTP/1.0 client for loopback probes (no TLS, no redirects).
/// `body` turns the request into a POST with `application/json`.
pub fn http(
    addr: SocketAddr,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Result<HttpReply, String> {
    let t = Instant::now();
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect: {e}"))?;
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let req = match body {
        Some(b) => format!(
            "POST {path} HTTP/1.0\r\nHost: {addr}\r\nUser-Agent: galion-nerve\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        ),
        None => format!(
            "GET {path} HTTP/1.0\r\nHost: {addr}\r\nUser-Agent: galion-nerve\r\nConnection: close\r\n\r\n"
        ),
    };
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut buf = Vec::new();
    let _ = s.take(512 * 1024).read_to_end(&mut buf);
    let ms = t.elapsed().as_millis() as u64;
    if buf.is_empty() {
        return Err("empty reply".into());
    }
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| "bad status line".to_string())?;
    Ok(HttpReply {
        code,
        body: body.to_string(),
        ms,
    })
}

/// Loopback socket address for a port.
pub fn lo(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// Write a file atomically (tmp + rename) with mode 0600-ish defaults of the
/// process umask.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

/// Append one line to a file (created when missing).
pub fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")
}

/// Least-squares slope of (x, y) points, `None` with fewer than 3 points or
/// no spread in x.
pub fn slope(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 3 {
        return None;
    }
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    if sxx <= f64::EPSILON {
        return None;
    }
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    Some(sxy / sxx)
}

/// Median of a slice (copied and sorted), `None` when empty.
pub fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let m = s.len() / 2;
    Some(if s.len().is_multiple_of(2) {
        (s[m - 1] + s[m]) / 2.0
    } else {
        s[m]
    })
}

/// Human size for KiB values.
pub fn kib(k: u64) -> String {
    if k >= 1024 * 1024 {
        format!("{:.1} GiB", k as f64 / 1048576.0)
    } else if k >= 1024 {
        format!("{:.1} MiB", k as f64 / 1024.0)
    } else {
        format!("{k} KiB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slope_and_median() {
        let pts: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, 2.0 * i as f64 + 1.0)).collect();
        assert!((slope(&pts).unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(slope(&pts[..2]), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn run_timeout_and_output() {
        let o = run(
            "sh",
            &["-c", "echo hi; echo err >&2; exit 3"],
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(o.code, 3);
        assert_eq!(o.stdout.trim(), "hi");
        assert_eq!(o.stderr.trim(), "err");
        assert!(run("sleep", &["5"], Duration::from_millis(200)).is_none());
        assert!(run("/nonexistent/prog", &[], Duration::from_secs(1)).is_none());
    }

    #[test]
    fn ts_is_zagreb() {
        // 2026-10-18 20:00 UTC
        assert_eq!(ts(1_792_353_600), "2026-10-18T22:00:00+02:00");
    }
}
