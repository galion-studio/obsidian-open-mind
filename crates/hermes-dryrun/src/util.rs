// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Small std-only helpers: sha256, a loopback HTTP/1.1 client and server,
//! append-only JSON line logs.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_bytes(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// sha256 + size of a regular file (symlinks refused).
pub fn sha256_file(p: &Path) -> Result<(String, u64), String> {
    let m = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !m.file_type().is_file() {
        return Err(format!("{} is not a regular file", p.display()));
    }
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf).map_err(|e| e.to_string())?;
        if k == 0 {
            break;
        }
        n += k as u64;
        h.update(&buf[..k]);
    }
    Ok((hex(&h.finalize()), n))
}

/// 16 random bytes as 32 hex (from /dev/urandom).
pub fn nonce() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    hex(&b)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Europe/Zagreb wall clock (CET +01:00 / CEST +02:00, EU rule), RFC 3339.
pub fn zagreb(ts: i64) -> String {
    fn civil(days: i64) -> (i64, u32, u32) {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (if m <= 2 { y + 1 } else { y }, m, d)
    }
    fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
    fn last_sunday(y: i64, m: u32) -> i64 {
        let d31 = days_from_civil(y, m, 31);
        let wd = (d31 + 4).rem_euclid(7); // 0 = Sunday
        d31 - wd
    }
    let (y, _, _) = civil(ts.div_euclid(86_400));
    let start = last_sunday(y, 3) * 86_400 + 3600;
    let end = last_sunday(y, 10) * 86_400 + 3600;
    let off = if ts >= start && ts < end { 7200 } else { 3600 };
    let l = ts + off;
    let (yy, mm, dd) = civil(l.div_euclid(86_400));
    let s = l.rem_euclid(86_400);
    format!(
        "{yy:04}-{mm:02}-{dd:02}T{:02}:{:02}:{:02}+{:02}:00",
        s / 3600,
        (s / 60) % 60,
        s % 60,
        off / 3600
    )
}

/// Shared append-only text log.
#[derive(Clone)]
pub struct Log(pub Arc<Mutex<std::fs::File>>);

impl Log {
    pub fn create(p: &Path) -> Result<Log, String> {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(Log(Arc::new(Mutex::new(f))))
    }
    pub fn line(&self, s: &str) {
        if let Ok(mut f) = self.0.lock() {
            let _ = writeln!(f, "{} {s}", zagreb(now_secs()));
        }
    }
    pub fn json(&self, v: &Value) {
        if let Ok(mut f) = self.0.lock() {
            let _ = writeln!(f, "{v}");
        }
    }
}

pub struct HttpResp {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResp {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Plain HTTP/1.1 to 127.0.0.1 only (Connection: close).
pub fn http(
    port: u16,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<HttpResp, String> {
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(3))
        .map_err(|e| format!("connect 127.0.0.1:{port}: {e}"))?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n",
        b.len()
    );
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
    }
    if let Some(k) = bearer {
        req.push_str(&format!("Authorization: Bearer {k}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&b);
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| format!("read: {e}"))?;
    parse_response(&raw)
}

pub fn parse_response(raw: &[u8]) -> Result<HttpResp, String> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no HTTP header end")?;
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("bad status line")?;
    let mut body = raw[end + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    Ok(HttpResp { status, body })
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(i) = b.windows(2).position(|w| w == b"\r\n") {
        let n = usize::from_str_radix(
            String::from_utf8_lossy(&b[..i])
                .split(';')
                .next()
                .unwrap_or("")
                .trim(),
            16,
        )
        .unwrap_or(0);
        if n == 0 || b.len() < i + 2 + n {
            break;
        }
        out.extend_from_slice(&b[i + 2..i + 2 + n]);
        b = &b[(i + 4 + n).min(b.len())..];
    }
    out
}

/// One parsed request to a stub server.
pub struct Req {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn header(&self, n: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

pub fn read_req(s: &mut TcpStream) -> Result<Req, String> {
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut r = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut it = line.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("").to_string();
    let mut headers = vec![];
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let n: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0)
        .min(32 << 20);
    if headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("expect") && v.eq_ignore_ascii_case("100-continue"))
    {
        let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Req {
        method,
        path,
        headers,
        body,
    })
}

pub fn respond(s: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let _ = write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(body);
    let _ = s.flush();
}

pub fn respond_json(s: &mut TcpStream, code: u16, v: &Value) {
    respond(s, code, "application/json", v.to_string().as_bytes())
}

/// Serve `h` on 127.0.0.1:port, one thread per connection.
pub fn serve<F>(port: u16, h: F) -> Result<(), String>
where
    F: Fn(Req, &mut TcpStream) + Send + Sync + 'static,
{
    let l = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    let h = Arc::new(h);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let h = h.clone();
            std::thread::spawn(move || {
                let mut s = s;
                if let Ok(r) = read_req(&mut s) {
                    h(r, &mut s);
                }
            });
        }
    });
    Ok(())
}

/// True when nothing listens on 127.0.0.1:port in this network namespace.
pub fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}
