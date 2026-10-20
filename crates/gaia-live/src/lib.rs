// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Gaia live.
//!
//! **M1, this module: read-only live health of the real services.**
//! Three kinds of evidence, all read-only:
//! * an HTTP `GET` to a loopback port (the only method the probes send;
//!   see [`GET_ONLY`]), with connect/read timeouts and a 64 KiB read cap;
//! * `systemctl is-active <unit>` (a query; nothing is started or stopped);
//! * the age of a state file (metadata only, content is never read).
//!
//! Each probe belongs to one GAIA [`System`], so a failing service degrades
//! the owning subordinate function in `gaia status --live`.
//!
//! **M2, [`heal`] and [`nerve`]: the first apply path**, limited to
//! box-local test services on 127.0.0.1:19000-19999, gated by a HADES
//! approval token and capped at 3 restarts per service per hour. The only
//! non-GET request is [`nerve::post`] to a loopback nerve.

pub mod heal;
pub mod nerve;

use gaia_core::{Json, System};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// The one HTTP method the health probes emit (the nerve client in
/// [`nerve`] is the only POST, and only to loopback).
pub const GET_ONLY: &str = "GET";
/// Bytes read from any probed response, at most.
pub const MAX_READ: usize = 64 * 1024;

/// What counts as "up" for an HTTP probe.
#[derive(Clone, Debug, PartialEq)]
pub enum Expect {
    /// 2xx and a JSON body whose `status` is "ok" (or `ok` is true).
    JsonOk,
    /// 2xx and the body (trimmed) equals this text.
    Body(String),
    /// Any 2xx.
    Status2xx,
    /// Any HTTP answer below 500 (the server is alive; e.g. a 404 page).
    Alive,
}

#[derive(Clone, Debug)]
pub struct HttpCheck {
    pub port: u16,
    pub path: String,
    pub expect: Expect,
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub name: String,
    pub system: System,
    pub http: Option<HttpCheck>,
    pub unit: Option<String>,
    /// State file that must have been modified within `max_age`.
    pub fresh_file: Option<(PathBuf, Duration)>,
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub host: String,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub systemd: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            host: "127.0.0.1".into(),
            connect_timeout: Duration::from_millis(1500),
            read_timeout: Duration::from_millis(3000),
            systemd: true,
        }
    }
}

/// The live stack on the Galion VPS (ports and units as deployed 2026-10-18).
pub fn default_probes() -> Vec<Probe> {
    let http = |port: u16, path: &str, expect: Expect| {
        Some(HttpCheck {
            port,
            path: path.into(),
            expect,
        })
    };
    let unit = |u: &str| Some(u.to_string());
    vec![
        Probe {
            name: "openmind-memory".into(),
            system: System::MemoryGateway,
            http: http(8420, "/health", Expect::JsonOk),
            unit: unit("openmind-memory.service"),
            fresh_file: None,
        },
        Probe {
            name: "hermes-rs".into(),
            system: System::HermesAgents,
            http: http(8642, "/health", Expect::JsonOk),
            unit: unit("hermes-rs.service"),
            fresh_file: None,
        },
        Probe {
            name: "link-watch".into(),
            system: System::LinkWatch,
            http: None,
            unit: unit("link-watch-rs.service"),
            fresh_file: Some((
                PathBuf::from("/opt/galion/galion-light/state/link.json"),
                Duration::from_secs(300),
            )),
        },
        Probe {
            name: "tuner".into(),
            system: System::Tuner,
            http: http(8650, "/health", Expect::JsonOk),
            unit: unit("tuner.service"),
            fresh_file: None,
        },
        Probe {
            name: "galion-one".into(),
            system: System::GalionOne,
            http: http(8091, "/health", Expect::Body("ok".into())),
            unit: unit("galion-one.service"),
            fresh_file: None,
        },
        Probe {
            name: "console".into(),
            system: System::Console,
            http: http(8080, "/", Expect::Alive),
            unit: unit("console.service"),
            fresh_file: None,
        },
    ]
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Up,
    /// Some evidence good, some bad (e.g. port answers but unit inactive).
    Degraded,
    Down,
}

impl State {
    pub fn name(&self) -> &'static str {
        match self {
            State::Up => "up",
            State::Degraded => "degraded",
            State::Down => "down",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpOutcome {
    pub ok: bool,
    pub status: Option<u16>,
    pub latency: Duration,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub name: String,
    pub system: System,
    pub state: State,
    pub http: Option<HttpOutcome>,
    /// `systemctl is-active` answer ("active", "inactive", "failed", ...).
    pub unit: Option<(String, String)>,
    pub file_age_s: Option<u64>,
    pub notes: Vec<String>,
}

impl ProbeResult {
    pub fn to_json(&self) -> Json {
        let mut j = Json::obj()
            .with("service", self.name.as_str())
            .with("system", self.system.name())
            .with("function", self.system.function().name())
            .with("state", self.state.name());
        if let Some(h) = &self.http {
            j = j.with(
                "http",
                Json::obj()
                    .with("ok", h.ok)
                    .with("status", h.status.map(u64::from))
                    .with("latency_us", h.latency.as_micros() as u64)
                    .with("detail", h.detail.as_str()),
            );
        }
        if let Some((u, s)) = &self.unit {
            j = j.with(
                "unit",
                Json::obj()
                    .with("name", u.as_str())
                    .with("active", s.as_str()),
            );
        }
        if let Some(a) = self.file_age_s {
            j = j.with("state_file_age_s", a);
        }
        j.with(
            "notes",
            Json::Arr(self.notes.iter().map(|n| Json::from(n.as_str())).collect()),
        )
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve: {e}"))?
        .next()
        .ok_or_else(|| "no address".into())
}

/// The exact bytes of a probe request (GET, no body, no credentials).
pub fn request_bytes(host: &str, path: &str) -> String {
    format!(
        "{GET_ONLY} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: gaia-live/{}\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// One read-only HTTP GET.
pub fn http_get(host: &str, c: &HttpCheck, o: &Opts) -> HttpOutcome {
    let t0 = Instant::now();
    let fail = |detail: String| HttpOutcome {
        ok: false,
        status: None,
        latency: t0.elapsed(),
        detail,
    };
    let addr = match resolve(host, c.port) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let mut s = match TcpStream::connect_timeout(&addr, o.connect_timeout) {
        Ok(s) => s,
        Err(e) => return fail(format!("connect: {}", e.kind())),
    };
    let _ = s.set_read_timeout(Some(o.read_timeout));
    let _ = s.set_write_timeout(Some(o.read_timeout));
    if let Err(e) = s.write_all(request_bytes(host, &c.path).as_bytes()) {
        return fail(format!("write: {}", e.kind()));
    }
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = t0 + o.connect_timeout + o.read_timeout;
    loop {
        if Instant::now() > deadline {
            return fail("read: deadline".into());
        }
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() >= MAX_READ || complete(&buf) {
                    break;
                }
            }
            Err(e) => {
                if buf.is_empty() {
                    return fail(format!("read: {}", e.kind()));
                }
                break;
            }
        }
    }
    let latency = t0.elapsed();
    let text = String::from_utf8_lossy(&buf[..buf.len().min(MAX_READ)]).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: Option<u16> = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok());
    let Some(code) = status else {
        return HttpOutcome {
            ok: false,
            status: None,
            latency,
            detail: "not an HTTP response".into(),
        };
    };
    let two = (200..300).contains(&code);
    let (ok, detail) = match &c.expect {
        Expect::Alive => (code < 500, format!("HTTP {code}")),
        Expect::Status2xx => (two, format!("HTTP {code}")),
        Expect::Body(want) => {
            let b = body.trim();
            (
                two && b == want,
                format!(
                    "HTTP {code}, body {}",
                    if b == want { "matches" } else { "differs" }
                ),
            )
        }
        Expect::JsonOk => {
            let squeezed: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            let good = squeezed.contains("\"status\":\"ok\"") || squeezed.contains("\"ok\":true");
            (
                two && good,
                format!(
                    "HTTP {code}, {}",
                    if good { "status ok" } else { "status not ok" }
                ),
            )
        }
    };
    HttpOutcome {
        ok,
        status,
        latency,
        detail,
    }
}

/// Whole response received (Content-Length satisfied).
fn complete(buf: &[u8]) -> bool {
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
    head.lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .is_some_and(|n| buf.len() >= end + 4 + n)
}

/// `systemctl is-active UNIT` (read-only query). None when systemctl is
/// missing (phones, laptops, containers).
pub fn unit_state(unit: &str) -> Option<String> {
    if !unit
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
    {
        return Some("invalid-unit-name".into());
    }
    let out = std::process::Command::new("systemctl")
        .args(["is-active", "--", unit])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(if s.is_empty() { "unknown".into() } else { s })
}

fn file_age(p: &PathBuf) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(m)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

pub fn probe(p: &Probe, o: &Opts) -> ProbeResult {
    let mut good = 0;
    let mut bad = 0;
    let mut notes = Vec::new();
    let http = p.http.as_ref().map(|c| {
        let h = http_get(&o.host, c, o);
        if h.ok {
            good += 1;
        } else {
            bad += 1;
            notes.push(format!("GET :{}{} -> {}", c.port, c.path, h.detail));
        }
        h
    });
    let unit = match (&p.unit, o.systemd) {
        (Some(u), true) => unit_state(u).map(|s| {
            if s == "active" {
                good += 1;
            } else {
                bad += 1;
                notes.push(format!("{u} is {s}"));
            }
            (u.clone(), s)
        }),
        _ => None,
    };
    let mut file_age_s = None;
    if let Some((f, max)) = &p.fresh_file {
        match file_age(f) {
            Some(a) => {
                file_age_s = Some(a);
                if a <= max.as_secs() {
                    good += 1;
                } else {
                    bad += 1;
                    notes.push(format!(
                        "{} not updated for {a}s (max {}s)",
                        f.display(),
                        max.as_secs()
                    ));
                }
            }
            None => {
                bad += 1;
                notes.push(format!("{} missing", f.display()));
            }
        }
    }
    let state = match (good, bad) {
        (_, 0) if good > 0 => State::Up,
        (0, _) => State::Down,
        _ => State::Degraded,
    };
    ProbeResult {
        name: p.name.clone(),
        system: p.system,
        state,
        http,
        unit,
        file_age_s,
        notes,
    }
}

/// Probe everything in parallel (one short-lived thread per probe).
pub fn probe_all(probes: &[Probe], o: &Opts) -> Vec<ProbeResult> {
    let hs: Vec<_> = probes
        .iter()
        .cloned()
        .map(|p| {
            let o = o.clone();
            std::thread::spawn(move || probe(&p, &o))
        })
        .collect();
    hs.into_iter()
        .zip(probes)
        .map(|(h, p)| {
            h.join().unwrap_or_else(|_| ProbeResult {
                name: p.name.clone(),
                system: p.system,
                state: State::Down,
                http: None,
                unit: None,
                file_age_s: None,
                notes: vec!["probe thread panicked".into()],
            })
        })
        .collect()
}
