// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.9 public surface: DNS resolution, TLS certificate expiry and HTTP
//! status for configured hostnames (the tunnel hostnames, `tuner.galion.app`
//! NXDOMAIN in REVIEW.md:100).
//!
//! Live only ([`Input::live`]). Checks run on a short-lived background
//! thread every `NERVE_SURFACE_EVERY_S` (300 s) so the cycle never waits on
//! the network; each cycle reports the last finished round. One round is
//! read-only: a DNS lookup, then either a TLS 1.2 handshake that stops right
//! after the server's Certificate message (no keys, no data sent) or one
//! HTTP/1.1 `GET`.
//!
//! The TLS check is std-only on purpose (no TLS stack, no crypto, no new
//! dependency): it offers TLS 1.2 only, reads the plaintext Certificate
//! message and takes `notAfter` from the leaf. It is an *expiry* check, not
//! a trust check (chain and name are not verified). A TLS-1.3-only server
//! cannot be read this way and is reported as info. HTTP status over TLS
//! needs a TLS stack (owner decision, see docs/real-tests/cortex-s2-c.md).
//!
//! Settings (sensor off unless `NERVE_SURFACE_TARGETS` is set):
//! - `NERVE_SURFACE_TARGETS=https://galion.app,http://127.0.0.1:8420/health@200`
//!   (`@NNN`: expected status, default any status below 400);
//! - `NERVE_SURFACE_EVERY_S` (300), `NERVE_SURFACE_TIMEOUT_MS` (3000),
//!   `NERVE_SURFACE_WARN_DAYS` (7), `NERVE_SURFACE_CRIT_DAYS` (2).
//!
//! Findings (bleeding, component and cause `edge`, aggregated per check):
//! `surface-dns`, `surface-tls-expired`, `surface-tls-expiring`,
//! `surface-tls-fail`, `surface-http-down`, `surface-http-status` and info
//! `surface-tls-unread` / `surface-note`. Titles name targets by their
//! 1-based position (`#3`) with counts, days and status codes; hostnames and
//! error texts stay in the loopback-only evidence.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Name resolver (the system one live; tests inject a map).
pub type ResolveFn = dyn Fn(&str, u16) -> Result<Vec<SocketAddr>, String> + Send + Sync;
/// Shared resolver handle.
pub type Resolve = Arc<ResolveFn>;

/// One configured target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// 1-based position in `NERVE_SURFACE_TARGETS` (the id in titles).
    pub n: usize,
    pub tls: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub expect: Option<u16>,
}

impl Target {
    /// `https://host[:port][/path][@status]` or `http://...`.
    pub fn parse(n: usize, s: &str) -> Option<Target> {
        let s = s.trim();
        let (s, expect) = match s.rsplit_once('@') {
            Some((a, b)) if b.len() == 3 && b.bytes().all(|c| c.is_ascii_digit()) => {
                (a, b.parse().ok())
            }
            _ => (s, None),
        };
        let (tls, rest) = if let Some(r) = s.strip_prefix("https://") {
            (true, r)
        } else {
            (false, s.strip_prefix("http://")?)
        };
        let (hp, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match hp.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()?),
            None => (hp, if tls { 443 } else { 80 }),
        };
        let ok = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
        if host.is_empty() || !host.chars().all(ok) {
            return None;
        }
        Some(Target {
            n,
            tls,
            host: host.to_ascii_lowercase(),
            port,
            path: path.to_string(),
            expect,
        })
    }
}

/// TLS result of one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tls {
    /// Leaf certificate `notAfter`, unix seconds.
    NotAfter(u64),
    /// The server would not speak TLS 1.2 (expiry not readable here).
    NoTls12(String),
    Failed(String),
}

/// What one round saw for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub n: usize,
    /// Number of addresses, or the resolver error.
    pub dns: Result<usize, String>,
    pub tls: Option<Tls>,
    pub http: Option<Result<u16, String>>,
    /// Every address is in 198.18.0.0/15 (a fake-IP egress resolver, as on
    /// the swarm box): resolution says nothing about public DNS.
    pub proxied: bool,
}

/// Thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub warn_days: u64,
    pub crit_days: u64,
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            warn_days: 7,
            crit_days: 2,
            timeout: Duration::from_millis(3000),
        }
    }
}

/// Sensor `surface`.
pub struct Surface {
    pub targets: Vec<Target>,
    /// Entries of `NERVE_SURFACE_TARGETS` that did not parse.
    pub bad: usize,
    pub limits: Limits,
    pub every_s: u64,
    pub resolve: Resolve,
    next_due: u64,
    worker: Option<JoinHandle<Vec<Outcome>>>,
    /// Last finished round.
    pub last: Option<Vec<Outcome>>,
    /// Rounds started (tests, budget).
    pub rounds: u64,
}

/// The system resolver (getaddrinfo through std).
pub fn system_resolve() -> Resolve {
    Arc::new(|h: &str, p: u16| {
        (h, p)
            .to_socket_addrs()
            .map(|a| a.collect())
            .map_err(|e| e.to_string())
    })
}

impl Surface {
    pub fn new(targets: Vec<Target>, limits: Limits, every_s: u64, resolve: Resolve) -> Self {
        Surface {
            targets,
            bad: 0,
            limits,
            every_s: every_s.max(1),
            resolve,
            next_due: 0,
            worker: None,
            last: None,
            rounds: 0,
        }
    }

    /// `NERVE_SURFACE_TARGETS` set and non-empty, else None (off).
    pub fn from_env(env: Env) -> Option<Self> {
        let raw = env("NERVE_SURFACE_TARGETS")?;
        let items: Vec<&str> = raw.split(',').filter(|s| !s.trim().is_empty()).collect();
        if items.is_empty() {
            return None;
        }
        let targets: Vec<Target> = items
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Target::parse(i + 1, s))
            .collect();
        let num = |k: &str, d: u64| env(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
        let limits = Limits {
            warn_days: num("NERVE_SURFACE_WARN_DAYS", 7),
            crit_days: num("NERVE_SURFACE_CRIT_DAYS", 2),
            timeout: Duration::from_millis(num("NERVE_SURFACE_TIMEOUT_MS", 3000).max(100)),
        };
        let mut s = Self::new(
            targets,
            limits,
            num("NERVE_SURFACE_EVERY_S", 300),
            system_resolve(),
        );
        s.bad = items.len() - s.targets.len();
        Some(s)
    }

    /// Collect a finished round; start one when due and none is running.
    fn tick(&mut self, now: u64) {
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            if let Ok(r) = self.worker.take().map(|w| w.join()).transpose() {
                self.last = r;
            }
        }
        if self.worker.is_none() && now >= self.next_due {
            self.next_due = now + self.every_s;
            self.rounds += 1;
            let (targets, lim, res) = (self.targets.clone(), self.limits, self.resolve.clone());
            self.worker = std::thread::Builder::new()
                .name("nerve-surface".into())
                .spawn(move || targets.iter().map(|t| check(t, &lim, &*res)).collect())
                .ok();
        }
    }
}

impl Sensor for Surface {
    fn name(&self) -> &'static str {
        "surface"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return;
        }
        self.tick(i.now);
        if let Some(r) = &self.last {
            for f in judge(&self.targets, r, i.now, &self.limits) {
                out.finding(f);
            }
        }
        if self.bad > 0 {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "surface-note",
                    Sev::Info,
                    "edge",
                    format!("{} NERVE_SURFACE_TARGETS entr(ies) ignored (not http(s)://host[:port][/path][@NNN])", self.bad),
                )
                .with_cause("edge"),
            );
        }
    }
}

/// One read-only check of one target.
pub fn check(t: &Target, lim: &Limits, resolve: &ResolveFn) -> Outcome {
    let mut o = Outcome {
        n: t.n,
        dns: Err(String::new()),
        tls: None,
        http: None,
        proxied: false,
    };
    let addrs = match resolve(&t.host, t.port) {
        Ok(a) if !a.is_empty() => a,
        Ok(_) => {
            o.dns = Err("no addresses".into());
            return o;
        }
        Err(e) => {
            o.dns = Err(e);
            return o;
        }
    };
    o.dns = Ok(addrs.len());
    o.proxied = addrs.iter().all(fake_ip);
    let conn = connect(&addrs, lim.timeout);
    if t.tls {
        o.tls = Some(match conn {
            Err(e) => Tls::Failed(format!("connect: {e}")),
            Ok(mut s) => tls_expiry(&mut s, &t.host, lim.timeout),
        });
    } else {
        o.http = Some(conn.and_then(|mut s| http_status(&mut s, t, lim.timeout)));
    }
    o
}

fn connect(addrs: &[SocketAddr], timeout: Duration) -> Result<TcpStream, String> {
    let mut last = String::from("no address");
    for a in addrs.iter().take(4) {
        match TcpStream::connect_timeout(a, timeout) {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(timeout));
                let _ = s.set_write_timeout(Some(timeout));
                return Ok(s);
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// One `GET`, status line only (at most 1 KiB read).
fn http_status(s: &mut TcpStream, t: &Target, timeout: Duration) -> Result<u16, String> {
    let host = if t.port == 80 {
        t.host.clone()
    } else {
        format!("{}:{}", t.host, t.port)
    };
    let req = format!(

        "GET {} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: galion-nerve-surface\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        t.path
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 256];
    while !buf.contains(&b'\n') && buf.len() < 1024 && Instant::now() < deadline {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    parse_status_line(&String::from_utf8_lossy(&buf)).ok_or_else(|| "no HTTP status line".into())
}

/// `HTTP/1.x NNN ...` -> NNN.
pub fn parse_status_line(s: &str) -> Option<u16> {
    let mut f = s.lines().next()?.split_whitespace();
    f.next()?.strip_prefix("HTTP/")?;
    let code: u16 = f.next()?.parse().ok()?;
    (100..=599).contains(&code).then_some(code)
}

fn tls_expiry(s: &mut TcpStream, host: &str, timeout: Duration) -> Tls {
    if let Err(e) = s.write_all(&client_hello(host)) {
        return Tls::Failed(format!("write: {e}"));
    }
    let deadline = Instant::now() + timeout;
    let mut r = Deadline(s, deadline);
    match read_leaf_cert(&mut r) {
        Ok(der) => match cert_not_after(&der) {
            Some(t) => Tls::NotAfter(t),
            None => Tls::Failed("certificate: notAfter not parsed".into()),
        },
        Err(FlightError::Alert(70)) | Err(FlightError::Alert(40)) => {
            Tls::NoTls12("server refused TLS 1.2 (alert)".into())
        }
        Err(e) => Tls::Failed(e.to_string()),
    }
}

/// A reader that gives up at a deadline (socket read timeout per call).
struct Deadline<'a>(&'a mut TcpStream, Instant);

impl Read for Deadline<'_> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        if Instant::now() >= self.1 {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.0.read(b)
    }
}

/// A TLS 1.2 ClientHello with SNI (only what a server needs to send its
/// certificate; the handshake is abandoned after it).
pub fn client_hello(host: &str) -> Vec<u8> {
    let mut body = vec![0x03, 0x03];
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    for i in 0..32u64 {
        // Not a secret: no key exchange ever completes.
        body.push((seed.rotate_left((i * 7) as u32) >> 3) as u8 ^ i as u8);
    }
    body.push(0); // session id
    let suites: [u16; 12] = [
        0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc009, 0xc013, 0xc00a, 0xc014, 0x009c,
        0x002f,
    ];
    put16(&mut body, (suites.len() * 2) as u16);
    for s in suites {
        put16(&mut body, s);
    }
    body.extend_from_slice(&[1, 0]); // compression: null
    let mut ext = vec![];
    let name = host.as_bytes();
    if !name.is_empty() && host.parse::<std::net::IpAddr>().is_err() {
        put16(&mut ext, 0x0000);
        put16(&mut ext, name.len() as u16 + 5);
        put16(&mut ext, name.len() as u16 + 3);
        ext.push(0);
        put16(&mut ext, name.len() as u16);
        ext.extend_from_slice(name);
    }
    for (ty, data) in [
        (
            0x000a_u16,
            &[0x00, 0x06, 0x00, 0x1d, 0x00, 0x17, 0x00, 0x18][..],
        ),
        (0x000b, &[0x01, 0x00][..]),
        (
            0x000d,
            &[
                0x00, 0x10, 0x04, 0x03, 0x05, 0x03, 0x06, 0x03, 0x08, 0x04, 0x08, 0x05, 0x08, 0x06,
                0x04, 0x01, 0x05, 0x01,
            ][..],
        ),
        (0xff01, &[0x00][..]),
    ] {
        put16(&mut ext, ty);
        put16(&mut ext, data.len() as u16);
        ext.extend_from_slice(data);
    }
    put16(&mut body, ext.len() as u16);
    body.extend_from_slice(&ext);
    let mut hs = vec![1];
    put24(&mut hs, body.len());
    hs.extend_from_slice(&body);
    let mut rec = vec![0x16, 0x03, 0x01];
    put16(&mut rec, hs.len() as u16);
    rec.extend_from_slice(&hs);
    rec
}

fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn put24(v: &mut Vec<u8>, x: usize) {
    v.extend_from_slice(&(x as u32).to_be_bytes()[1..]);
}

/// Why the server flight did not yield a certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlightError {
    Io(String),
    /// TLS alert description (70 protocol_version, 40 handshake_failure).
    Alert(u8),
    Protocol(&'static str),
}

impl std::fmt::Display for FlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlightError::Io(e) => write!(f, "read: {e}"),
            FlightError::Alert(d) => write!(f, "TLS alert {d}"),
            FlightError::Protocol(e) => write!(f, "protocol: {e}"),
        }
    }
}

/// Read server records until the Certificate handshake message; return the
/// leaf certificate DER. Works on a socket or a captured flight (fixture).
pub fn read_leaf_cert<R: Read>(r: &mut R) -> Result<Vec<u8>, FlightError> {
    const CAP: usize = 256 * 1024;
    let mut hs: Vec<u8> = vec![];
    let mut total = 0usize;
    loop {
        let mut h = [0u8; 5];
        r.read_exact(&mut h)
            .map_err(|e| FlightError::Io(e.to_string()))?;
        let len = u16::from_be_bytes([h[3], h[4]]) as usize;
        total += 5 + len;
        if h[1] != 3 || len > 18_432 || total > CAP {
            return Err(FlightError::Protocol("not a TLS record"));
        }
        let mut body = vec![0u8; len];
        r.read_exact(&mut body)
            .map_err(|e| FlightError::Io(e.to_string()))?;
        match h[0] {
            22 => hs.extend_from_slice(&body),
            21 => return Err(FlightError::Alert(body.get(1).copied().unwrap_or(0))),
            _ => {
                return Err(FlightError::Protocol(
                    "unexpected record before certificate",
                ))
            }
        }
        let mut p = 0;
        while hs.len() >= p + 4 {
            let ty = hs[p];
            let n = u32::from_be_bytes([0, hs[p + 1], hs[p + 2], hs[p + 3]]) as usize;
            if hs.len() < p + 4 + n {
                break; // message continues in the next record
            }
            let m = &hs[p + 4..p + 4 + n];
            match ty {
                2 if m.get(..2) != Some(&[3, 3]) => {
                    return Err(FlightError::Protocol("server hello is not TLS 1.2"))
                }
                11 => {
                    let n1 = m
                        .get(3..6)
                        .map(|b| u32::from_be_bytes([0, b[0], b[1], b[2]]) as usize)
                        .ok_or(FlightError::Protocol("short certificate"))?;
                    return m
                        .get(6..6 + n1)
                        .map(<[u8]>::to_vec)
                        .ok_or(FlightError::Protocol("short certificate"));
                }
                14 => return Err(FlightError::Protocol("no certificate sent")),
                _ => {}
            }
            p += 4 + n;
        }
    }
}

/// One DER element at `p`: (tag, content start, content end).
fn der_at(b: &[u8], p: usize) -> Option<(u8, usize, usize)> {
    let tag = *b.get(p)?;
    let l = *b.get(p + 1)? as usize;
    let (len, start) = if l < 0x80 {
        (l, p + 2)
    } else {
        let k = l & 0x7f;
        if k == 0 || k > 4 {
            return None;
        }
        let mut n = 0usize;
        for i in 0..k {
            n = (n << 8) | *b.get(p + 2 + i)? as usize;
        }
        (n, p + 2 + k)
    };
    let end = start.checked_add(len)?;
    (end <= b.len()).then_some((tag, start, end))
}

/// `tbsCertificate.validity.notAfter` of an X.509 DER certificate.
pub fn cert_not_after(der: &[u8]) -> Option<u64> {
    let (t, s, _) = der_at(der, 0)?;
    if t != 0x30 {
        return None;
    }
    let (t, mut p, tbs_end) = der_at(der, s)?;
    if t != 0x30 {
        return None;
    }
    // [0] version (optional), serial, signature, issuer, validity
    if der_at(der, p)?.0 == 0xa0 {
        p = der_at(der, p)?.2;
    }
    for want in [0x02, 0x30, 0x30] {
        let (t, _, e) = der_at(der, p)?;
        if t != want || e > tbs_end {
            return None;
        }
        p = e;
    }
    let (t, vs, _) = der_at(der, p)?;
    if t != 0x30 {
        return None;
    }
    let nb_end = der_at(der, vs)?.2;
    let (t, s, e) = der_at(der, nb_end)?;
    parse_asn1_time(t, std::str::from_utf8(&der[s..e]).ok()?)
}

/// UTCTime (0x17, `YYMMDDHHMMSSZ`) or GeneralizedTime (0x18,
/// `YYYYMMDDHHMMSSZ`) -> unix seconds.
pub fn parse_asn1_time(tag: u8, s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (y, rest) = match tag {
        0x17 if s.len() == 12 => {
            let yy: i64 = s.get(..2)?.parse().ok()?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &s[2..])
        }
        0x18 if s.len() == 14 => (s.get(..4)?.parse().ok()?, &s[4..]),
        _ => return None,
    };
    let n = |i: usize| -> Option<i64> { rest.get(i..i + 2)?.parse().ok() };
    let (mo, d, h, mi, se) = (n(0)?, n(2)?, n(4)?, n(6)?, n(8)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + se).ok()
}

/// Days since 1970-01-01 (H. Hinnant's civil algorithm, public domain).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Pure verdict over one round (unit tests and fixtures drive this).
pub fn judge(targets: &[Target], round: &[Outcome], now: u64, lim: &Limits) -> Vec<Finding> {
    let host = |n: usize| {
        targets
            .iter()
            .find(|t| t.n == n)
            .map(|t| {
                format!(
                    "{}://{}:{}{}",
                    if t.tls { "https" } else { "http" },
                    t.host,
                    t.port,
                    t.path
                )
            })
            .unwrap_or_default()
    };
    struct Acc {
        id: &'static str,
        sev: Sev,
        items: Vec<String>,
        ev: Vec<String>,
    }
    let mut acc: Vec<Acc> = vec![];
    let mut add = |id: &'static str, sev: Sev, item: String, ev: String| match acc
        .iter_mut()
        .find(|a| a.id == id)
    {
        Some(a) => {
            a.sev = a.sev.max(sev);
            a.items.push(item);
            a.ev.push(ev);
        }
        None => acc.push(Acc {
            id,
            sev,
            items: vec![item],
            ev: vec![ev],
        }),
    };
    for o in round {
        let h = host(o.n);
        if let Err(e) = &o.dns {
            add(
                "surface-dns",
                Sev::Error,
                format!("#{}", o.n),
                format!("#{} {h}: {e}", o.n),
            );
            continue;
        }
        if o.proxied {
            add(
                "surface-dns-proxied",
                Sev::Info,
                format!("#{}", o.n),
                format!(
                    "#{} {h}: resolves to a fake-IP egress proxy (198.18.0.0/15)",
                    o.n
                ),
            );
        }
        match &o.tls {
            Some(Tls::NotAfter(t)) if *t <= now => {
                let d = (now - t) / 86_400;
                add(
                    "surface-tls-expired",
                    Sev::Critical,
                    format!("#{} ({d} d ago)", o.n),
                    format!("#{} {h}: notAfter {}", o.n, crate::util::ts(*t)),
                );
            }
            Some(Tls::NotAfter(t)) => {
                let d = (t - now) / 86_400;
                if d < lim.warn_days {
                    let sev = if d < lim.crit_days {
                        Sev::Error
                    } else {
                        Sev::Warn
                    };
                    add(
                        "surface-tls-expiring",
                        sev,
                        format!("#{} ({d} d)", o.n),
                        format!("#{} {h}: notAfter {}", o.n, crate::util::ts(*t)),
                    );
                }
            }
            Some(Tls::Failed(e)) => add(
                "surface-tls-fail",
                Sev::Error,
                format!("#{}", o.n),
                format!("#{} {h}: {e}", o.n),
            ),
            Some(Tls::NoTls12(e)) => add(
                "surface-tls-unread",
                Sev::Info,
                format!("#{}", o.n),
                format!("#{} {h}: {e}", o.n),
            ),
            None => {}
        }
        match &o.http {
            Some(Err(e)) => add(
                "surface-http-down",
                Sev::Error,
                format!("#{}", o.n),
                format!("#{} {h}: {e}", o.n),
            ),
            Some(Ok(code)) => {
                let want = targets.iter().find(|t| t.n == o.n).and_then(|t| t.expect);
                let ok = match want {
                    Some(w) => *code == w,
                    None => *code < 400,
                };
                if !ok {
                    let w = want.map_or("<400".to_string(), |w| w.to_string());
                    let sev = if *code >= 500 { Sev::Error } else { Sev::Warn };
                    add(
                        "surface-http-status",
                        sev,
                        format!("#{} ({code}, want {w})", o.n),
                        format!("#{} {h}: HTTP {code}, want {w}", o.n),
                    );
                }
            }
            None => {}
        }
    }
    let m = round.len();
    acc.into_iter()
        .map(|a| {
            let k = a.items.len();
            let what = match a.id {
                "surface-dns" => "do not resolve",
                "surface-tls-expired" => "serve an expired TLS certificate",
                "surface-tls-expiring" => "have a TLS certificate expiring soon",
                "surface-tls-fail" => "fail the TLS handshake",
                "surface-tls-unread" => "do not offer TLS 1.2 (expiry not readable)",
                "surface-dns-proxied" => {
                    "resolve through a fake-IP proxy (DNS verdict not authoritative)"
                }
                "surface-http-down" => "do not answer HTTP",
                _ => "answer an unexpected HTTP status",
            };
            let fix = match a.id {
                "surface-dns" => "add or fix the DNS record (owner; no DNS changes by the nerve)",
                "surface-tls-expired" | "surface-tls-expiring" => {
                    "renew or re-issue the edge certificate (owner)"
                }
                "surface-tls-unread" => "none needed; expiry check needs TLS 1.2 or a TLS stack",
                "surface-dns-proxied" => {
                    "run the DNS check from a host with a real resolver (the VPS)"
                }
                _ => "check the tunnel ingress rule and the origin service behind it",
            };
            Finding::new(
                FindingKind::Bleeding,
                a.id,
                a.sev,
                "edge",
                format!("{k} of {m} public target(s) {what}: {}", a.items.join(" ")),
            )
            .with_evidence(a.ev)
            .with_fix(fix)
            .with_cause("edge")
        })
        .collect()
}

/// 198.18.0.0/15 (RFC 2544 benchmark range, used by fake-IP DNS proxies).
fn fake_ip(a: &SocketAddr) -> bool {
    match a.ip() {
        std::net::IpAddr::V4(v) => v.octets()[0] == 198 && (v.octets()[1] & 0xfe) == 18,
        std::net::IpAddr::V6(_) => false,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;
    use std::net::TcpListener;

    pub fn tlv(tag: u8, c: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        match c.len() {
            n if n < 0x80 => v.push(n as u8),
            n if n < 0x100 => v.extend_from_slice(&[0x81, n as u8]),
            n => v.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]),
        }
        v.extend_from_slice(c);
        v
    }

    /// A structurally valid (unsigned) X.509 certificate with `not_after`.
    pub fn cert(not_after: &str) -> Vec<u8> {
        let tag = if not_after.len() == 13 { 0x17 } else { 0x18 };
        let validity = [tlv(0x17, b"260101000000Z"), tlv(tag, not_after.as_bytes())].concat();
        let name = tlv(
            0x30,
            &tlv(
                0x31,
                &tlv(
                    0x30,
                    &[tlv(0x06, &[0x55, 4, 3]), tlv(0x0c, b"fixture")].concat(),
                ),
            ),
        );
        let tbs = [
            tlv(0xa0, &tlv(0x02, &[2])),
            tlv(0x02, &[0x01, 0x23]),
            tlv(0x30, &tlv(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 4, 3, 2])),
            name.clone(),
            tlv(0x30, &validity),
            name,
            tlv(0x30, &[0u8; 200]), // pushes lengths into long form
        ]
        .concat();
        tlv(
            0x30,
            &[
                tlv(0x30, &tbs),
                tlv(0x30, &tlv(0x06, &[0x2a])),
                tlv(0x03, &[0, 1, 2]),
            ]
            .concat(),
        )
    }

    /// Server flight: ServerHello + Certificate + ServerHelloDone, the
    /// certificate message split across two records.
    pub fn flight(der: &[u8]) -> Vec<u8> {
        let mut hello = vec![3, 3];
        hello.extend_from_slice(&[7u8; 32]);
        hello.extend_from_slice(&[0, 0xc0, 0x2f, 0]);
        let mut msgs = vec![2];
        put24(&mut msgs, hello.len());
        msgs.extend_from_slice(&hello);
        let mut certs = vec![];
        put24(&mut certs, der.len() + 3);
        put24(&mut certs, der.len());
        certs.extend_from_slice(der);
        msgs.push(11);
        put24(&mut msgs, certs.len());
        msgs.extend_from_slice(&certs);
        msgs.extend_from_slice(&[14, 0, 0, 0]);
        let cut = msgs.len() / 2;
        let mut out = vec![];
        for part in [&msgs[..cut], &msgs[cut..]] {
            out.extend_from_slice(&[22, 3, 3]);
            put16(&mut out, part.len() as u16);
            out.extend_from_slice(part);
        }
        out
    }

    /// Fake TLS 1.2 server on loopback: checks the SNI it is sent, then
    /// answers `reply` (a flight or an alert). Returns the port.
    pub fn fake_tls(reply: Vec<u8>, want_sni: &'static str) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut h = [0u8; 5];
                if c.read_exact(&mut h).is_err() {
                    continue;
                }
                let mut b = vec![0u8; u16::from_be_bytes([h[3], h[4]]) as usize];
                let _ = c.read_exact(&mut b);
                assert_eq!(h[0], 22);
                assert!(
                    b.windows(want_sni.len()).any(|w| w == want_sni.as_bytes()),
                    "SNI sent"
                );
                let _ = c.write_all(&reply);
            }
        });
        port
    }

    /// Read a request head (until a blank line or EOF).
    pub fn read_head(c: &mut std::net::TcpStream) {
        let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
        let mut got = vec![];
        let mut b = [0u8; 512];
        while !got.windows(4).any(|w| w == b"\r\n\r\n") {
            match c.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => got.extend_from_slice(&b[..n]),
            }
        }
    }

    pub fn fake_http(status: u16) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                read_head(&mut c);
                let _ = c.write_all(
                    format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                );
            }
        });
        port
    }

    fn local() -> Resolve {
        Arc::new(|h: &str, p: u16| match h {
            "nx.fixture" => Err("failed to lookup address information".into()),
            _ => Ok(vec![SocketAddr::from(([127, 0, 0, 1], p))]),
        })
    }

    // 2026-10-20 23:00:00+02:00
    const NOW: u64 = 1_792_530_000;

    #[test]
    fn asn1_time_and_cert_not_after() {
        assert_eq!(parse_asn1_time(0x17, "700101000000Z"), Some(0));
        assert_eq!(parse_asn1_time(0x17, "261020210000Z"), Some(NOW));
        assert_eq!(parse_asn1_time(0x18, "20261020210000Z"), Some(NOW));
        assert_eq!(parse_asn1_time(0x17, "491231235959Z"), Some(2_524_607_999));
        assert_eq!(parse_asn1_time(0x17, "261320210000Z"), None);
        assert_eq!(parse_asn1_time(0x17, "2610202100Z"), None);
        assert_eq!(
            cert_not_after(&cert("261030210000Z")),
            Some(NOW + 10 * 86_400)
        );
        assert_eq!(
            cert_not_after(&cert("20261030210000Z")),
            Some(NOW + 10 * 86_400)
        );
        assert_eq!(cert_not_after(&[0x30, 0x03, 0x02, 0x01]), None, "truncated");
        assert_eq!(cert_not_after(b""), None);
    }

    #[test]
    fn flight_parser_split_records_alerts_and_garbage() {
        let der = cert("261030210000Z");
        assert_eq!(read_leaf_cert(&mut &flight(&der)[..]), Ok(der));
        assert_eq!(
            read_leaf_cert(&mut &[21u8, 3, 3, 0, 2, 2, 70][..]),
            Err(FlightError::Alert(70))
        );
        assert!(matches!(
            read_leaf_cert(&mut &b"HTTP/1.1 400 Bad\r\n\r\n"[..]),
            Err(FlightError::Protocol(_))
        ));
        assert!(matches!(
            read_leaf_cert(&mut &[22u8, 3, 3, 0, 9, 2][..]),
            Err(FlightError::Io(_))
        ));
        let ch = client_hello("galion.app");
        assert_eq!(&ch[..3], &[0x16, 3, 1]);
        assert_eq!(u16::from_be_bytes([ch[3], ch[4]]) as usize, ch.len() - 5);
        assert!(ch.windows(10).any(|w| w == b"galion.app"));
        assert!(
            !client_hello("127.0.0.1")
                .windows(9)
                .any(|w| w == b"127.0.0.1"),
            "no SNI for IPs"
        );
    }

    #[test]
    fn target_parse() {
        let t = Target::parse(3, " https://Galion.App ").unwrap();
        assert_eq!(
            (
                t.n,
                t.tls,
                t.host.as_str(),
                t.port,
                t.path.as_str(),
                t.expect
            ),
            (3, true, "galion.app", 443, "/", None)
        );
        let t = Target::parse(1, "http://127.0.0.1:8420/health?x=1@204").unwrap();
        assert_eq!(
            (t.port, t.path.as_str(), t.expect),
            (8420, "/health?x=1", Some(204))
        );
        assert!(Target::parse(1, "ftp://x").is_none());
        assert!(Target::parse(1, "https://").is_none());
        assert!(Target::parse(1, "https://a b").is_none());
        assert!(Target::parse(1, "http://x:99999").is_none());
        assert_eq!(
            parse_status_line("HTTP/1.1 503 Service Unavailable\r\n"),
            Some(503)
        );
        assert_eq!(parse_status_line("SSH-2.0-OpenSSH\r\n"), None);
    }

    #[test]
    fn judge_aggregates_and_keeps_hosts_out_of_titles() {
        let ts: Vec<Target> = [
            "https://nx.fixture",
            "https://old.fixture",
            "https://soon.fixture",
            "https://fine.fixture",
            "http://origin.fixture/health@200",
            "http://down.fixture",
            "https://tls13.fixture",
        ]
        .iter()
        .enumerate()
        .map(|(i, s)| Target::parse(i + 1, s).unwrap())
        .collect();
        let o = |n, dns: Result<usize, String>, tls, http| Outcome {
            n,
            dns,
            tls,
            http,
            proxied: false,
        };
        let round = [
            o(1, Err("NXDOMAIN".into()), None, None),
            o(2, Ok(1), Some(Tls::NotAfter(NOW - 2 * 86_400)), None),
            o(3, Ok(2), Some(Tls::NotAfter(NOW + 5 * 86_400)), None),
            o(4, Ok(2), Some(Tls::NotAfter(NOW + 60 * 86_400)), None),
            o(5, Ok(1), None, Some(Ok(503))),
            o(6, Ok(1), None, Some(Err("connection refused".into()))),
            o(7, Ok(1), Some(Tls::NoTls12("alert".into())), None),
        ];
        let f = judge(&ts, &round, NOW, &Limits::default());
        let got: Vec<(&str, Sev, &str)> = f
            .iter()
            .map(|f| (f.id.as_str(), f.sev, f.title.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    "surface-dns",
                    Sev::Error,
                    "1 of 7 public target(s) do not resolve: #1"
                ),
                (
                    "surface-tls-expired",
                    Sev::Critical,
                    "1 of 7 public target(s) serve an expired TLS certificate: #2 (2 d ago)"
                ),
                (
                    "surface-tls-expiring",
                    Sev::Warn,
                    "1 of 7 public target(s) have a TLS certificate expiring soon: #3 (5 d)"
                ),
                (
                    "surface-http-status",
                    Sev::Error,
                    "1 of 7 public target(s) answer an unexpected HTTP status: #5 (503, want 200)"
                ),
                (
                    "surface-http-down",
                    Sev::Error,
                    "1 of 7 public target(s) do not answer HTTP: #6"
                ),
                (
                    "surface-tls-unread",
                    Sev::Info,
                    "1 of 7 public target(s) do not offer TLS 1.2 (expiry not readable): #7"
                ),
            ]
        );
        assert!(f
            .iter()
            .all(|f| f.cause.as_deref() == Some("edge") && f.kind == FindingKind::Bleeding));
        assert!(f[0].evidence[0].contains("nx.fixture"));
        for x in &f {
            let ev = x.to_nerve_event(NOW).to_string();
            assert!(!ev.contains(".fixture") && !ev.contains("NXDOMAIN"), "{ev}");
        }
        let short = Limits {
            crit_days: 6,
            ..Limits::default()
        };
        assert_eq!(judge(&ts, &round[2..3], NOW, &short)[0].sev, Sev::Error);
        assert!(judge(&ts, &round[3..4], NOW, &Limits::default()).is_empty());
    }

    #[test]
    fn check_against_local_fixture_servers() {
        let lim = Limits {
            timeout: Duration::from_secs(2),
            ..Limits::default()
        };
        let res = local();
        let p = fake_tls(flight(&cert("261030210000Z")), "cert.fixture");
        let t = Target::parse(1, &format!("https://cert.fixture:{p}")).unwrap();
        assert_eq!(
            check(&t, &lim, &*res).tls,
            Some(Tls::NotAfter(NOW + 10 * 86_400))
        );
        let p = fake_tls(vec![21, 3, 3, 0, 2, 2, 70], "tls13.fixture");
        let t = Target::parse(2, &format!("https://tls13.fixture:{p}")).unwrap();
        assert!(matches!(check(&t, &lim, &*res).tls, Some(Tls::NoTls12(_))));
        let p = fake_http(503);
        let t = Target::parse(3, &format!("http://h.fixture:{p}/x@200")).unwrap();
        assert_eq!(check(&t, &lim, &*res).http, Some(Ok(503)));
        let t = Target::parse(4, "https://nx.fixture").unwrap();
        let o = check(&t, &lim, &*res);
        assert!(o.dns.is_err() && o.tls.is_none());
        let dead = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let t = Target::parse(5, &format!("http://d.fixture:{dead}")).unwrap();
        assert!(matches!(check(&t, &lim, &*res).http, Some(Err(_))));
    }

    #[test]
    fn sensor_runs_in_background_only_when_live() {
        let p = fake_http(500);
        let env = move |k: &str| match k {
            "NERVE_SURFACE_TARGETS" => Some(format!("http://w.fixture:{p}/, nonsense")),
            "NERVE_SURFACE_EVERY_S" => Some("60".into()),
            _ => None,
        };
        assert!(Surface::from_env(&|_| None).is_none());
        assert!(
            Surface::from_env(&|k| (k == "NERVE_SURFACE_TARGETS").then(|| " , ".into())).is_none()
        );
        let mut s = Surface::from_env(&env).unwrap();
        assert_eq!((s.targets.len(), s.bad, s.every_s), (1, 1, 60));
        s.resolve = local();
        let mut reg = Sensors::empty();
        reg.add(s);
        let logs = LogInsights::default();
        let mut i = Input::new(NOW, &[], &logs);
        let cx = reg.run(&i);
        assert!(
            cx.findings().next().is_none(),
            "replay/tests: nothing, no network"
        );
        i.live = true;
        let t = Instant::now();
        let mut cx = reg.run(&i);
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "cycle does not wait on the round"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cx.bleeding.iter().any(|f| f.id == "surface-http-status")
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(50));
            cx = reg.run(&i);
        }
        let f = cx
            .bleeding
            .iter()
            .find(|f| f.id == "surface-http-status")
            .expect("round reported");
        assert_eq!(
            f.title,
            "1 of 1 public target(s) answer an unexpected HTTP status: #1 (500, want <400)"
        );
        assert_eq!(f.source, "surface");
        assert!(cx
            .bleeding
            .iter()
            .any(|f| f.id == "surface-note" && f.sev == Sev::Info));
    }

    #[test]
    fn fake_ip_resolver_is_flagged_not_trusted() {
        assert!(fake_ip(&SocketAddr::from(([198, 18, 0, 1], 443))));
        assert!(fake_ip(&SocketAddr::from(([198, 19, 255, 1], 443))));
        assert!(!fake_ip(&SocketAddr::from(([198, 20, 0, 1], 443))));
        assert!(!fake_ip(&SocketAddr::from(([104, 21, 0, 1], 443))));
        let ts = [Target::parse(1, "https://tuner.fixture").unwrap()];
        let round = [Outcome {
            n: 1,
            dns: Ok(1),
            tls: Some(Tls::Failed("read: failed to fill whole buffer".into())),
            http: None,
            proxied: true,
        }];
        let f = judge(&ts, &round, NOW, &Limits::default());
        let ids: Vec<(&str, Sev)> = f.iter().map(|f| (f.id.as_str(), f.sev)).collect();
        assert_eq!(
            ids,
            [
                ("surface-dns-proxied", Sev::Info),
                ("surface-tls-fail", Sev::Error)
            ]
        );
    }
}
