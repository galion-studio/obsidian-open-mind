// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

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

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant's
/// public-domain `days_from_civil` algorithm, re-derived here).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn num_at(s: &str, a: usize, b: usize) -> Option<i64> {
    let t = s.get(a..b)?;
    if !t.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

/// Parse an RFC 3339 / ISO 8601 stamp (`2026-10-20T19:06:05+02:00`,
/// `...Z`, fractional seconds allowed, space instead of `T` allowed) into unix
/// seconds. Pure; `None` on anything malformed.
pub fn parse_rfc3339(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }
    let (y, mo, d) = (num_at(s, 0, 4)?, num_at(s, 5, 7)?, num_at(s, 8, 10)?);
    let (h, mi, se) = (num_at(s, 11, 13)?, num_at(s, 14, 16)?, num_at(s, 17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        let n = r.bytes().take_while(u8::is_ascii_digit).count();
        rest = &r[n..];
    }
    let off: i64 = match rest {
        "" | "Z" | "z" => 0,
        r if r.len() == 6 && (r.starts_with('+') || r.starts_with('-')) => {
            let v = num_at(r, 1, 3)? * 3600 + num_at(r, 4, 6)? * 60;
            if r.starts_with('-') {
                -v
            } else {
                v
            }
        }
        r if r.len() == 5 && (r.starts_with('+') || r.starts_with('-')) => {
            let v = num_at(r, 1, 3)? * 3600 + num_at(r, 3, 5)? * 60;
            if r.starts_with('-') {
                -v
            } else {
                v
            }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo as u32, d as u32);
    let t = days * 86_400 + h * 3600 + mi * 60 + se - off;
    u64::try_from(t).ok()
}

/// Parse a common-log-format stamp `20/Oct/2026:19:04:34 +0200` into unix seconds.
pub fn parse_clf_time(s: &str) -> Option<u64> {
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    let (dt, off) = s.split_once(' ').unwrap_or((s, "+0000"));
    let mut it = dt.splitn(4, ['/', ':']);
    let d = it.next()?;
    let mon = it.next()?;
    let y = it.next()?;
    let hms = it.next()?;
    let mo = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == mon)?
        + 1;
    let iso = format!("{y}-{mo:02}-{:0>2}T{hms}{off}", d);
    parse_rfc3339(&iso)
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

#[cfg(test)]
mod time_tests {
    use super::*;

    #[test]
    fn rfc3339_and_clf() {
        assert_eq!(
            parse_rfc3339("2026-10-18T22:00:00+02:00"),
            Some(1_792_353_600)
        );
        assert_eq!(parse_rfc3339("2026-10-18T20:00:00Z"), Some(1_792_353_600));
        assert_eq!(
            parse_rfc3339("2026-10-18 20:00:00.123Z"),
            Some(1_792_353_600)
        );
        assert_eq!(
            parse_rfc3339("2026-10-18T22:00:00+0200"),
            Some(1_792_353_600)
        );
        assert_eq!(parse_rfc3339("garbage"), None);
        assert_eq!(parse_rfc3339("2026-13-18T22:00:00+02:00"), None);
        assert_eq!(
            parse_clf_time("[18/Oct/2026:22:00:00 +0200]"),
            Some(1_792_353_600)
        );
        assert_eq!(
            ts(parse_rfc3339("2026-10-20T19:06:05+02:00").unwrap()),
            "2026-10-20T19:06:05+02:00"
        );
    }
}

/// Unix seconds from a persisted JSON value, tolerant of older state-dir
/// layouts: an integer, a non-negative float (truncated), a numeric string or
/// an RFC 3339 stamp. `None` for anything else.
pub fn json_secs(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite() && *f >= 0.0)
                .map(|f| f as u64)
        }),
        serde_json::Value::String(s) => s.trim().parse::<u64>().ok().or_else(|| parse_rfc3339(s)),
        _ => None,
    }
}

/// Keep a copy of a state file that exists but cannot be read as the
/// expected JSON at `<name>.corrupt-<now>` (next to it) before anything
/// overwrites it. Returns the copy's path, or `None` when nothing was copied.
pub fn quarantine(path: &Path, now: u64) -> Option<std::path::PathBuf> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let dst = path.with_file_name(format!("{name}.corrupt-{now}"));
    std::fs::copy(path, &dst).ok().map(|_| dst)
}
