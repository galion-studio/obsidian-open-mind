// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Optional active probe of known compute ports inside one local /24.
//!
//! Off by default: nothing calls [`probe`] unless the operator passes
//! `--probe NET`. The network must be a /24 inside loopback (127/8),
//! RFC 1918 private space or link-local (169.254/16); anything else is
//! refused before a socket is opened. Connections are paced by `rate`
//! (attempts per second, at most [`MAX_RATE`]), each with a short timeout.
//! An open HTTP port gets one small `GET` to fingerprint it.

use crate::{Candidate, DeviceKind, Origin, APPROVAL};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::{Duration, Instant};

/// Highest allowed connection attempts per second.
pub const MAX_RATE: u32 = 100;
/// Default connection attempts per second.
pub const DEFAULT_RATE: u32 = 20;

/// Ports probed by default, with the device each one suggests.
pub const DEFAULT_PORTS: [(DeviceKind, u16); 5] = [
    (DeviceKind::Exo, 52415),
    (DeviceKind::Ollama, 11434),
    (DeviceKind::LlamaRpc, 50052),
    (DeviceKind::LlamaCpp, 8080),
    (DeviceKind::Galion, 7787),
];

/// A validated /24 network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net24(Ipv4Addr);

impl Net24 {
    /// Parse `a.b.c.0/24` (or `a.b.c.d/24`; host bits are dropped).
    /// Refuses prefixes other than /24 and non-local ranges.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (ip, prefix) = text
            .split_once('/')
            .ok_or_else(|| "probe network must be written as A.B.C.0/24".to_string())?;
        if prefix != "24" {
            return Err("probe network must be a /24".into());
        }
        let ip: Ipv4Addr = ip
            .parse()
            .map_err(|_| "bad probe network address".to_string())?;
        Self::from_ip(ip)
    }

    /// The /24 holding `ip`, if it is local.
    pub fn from_ip(ip: Ipv4Addr) -> Result<Self, String> {
        if !(ip.is_loopback() || ip.is_private() || ip.is_link_local()) {
            return Err(format!(
                "refusing to probe {ip}: only loopback, RFC 1918 and link-local /24s are allowed"
            ));
        }
        let [a, b, c, _] = ip.octets();
        Ok(Self(Ipv4Addr::new(a, b, c, 0)))
    }

    /// Host `n` (1..=254) in this /24.
    pub fn host(&self, n: u8) -> Ipv4Addr {
        let [a, b, c, _] = self.0.octets();
        Ipv4Addr::new(a, b, c, n)
    }
}

impl std::fmt::Display for Net24 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/24", self.0)
    }
}

/// Probe settings.
#[derive(Debug, Clone)]
pub struct ProbeOpts {
    pub net: Net24,
    pub ports: Vec<(DeviceKind, u16)>,
    /// Connection attempts per second, clamped to 1..=MAX_RATE.
    pub rate: u32,
    pub connect_timeout: Duration,
    /// First and last host number, inside 1..=254.
    pub hosts: (u8, u8),
}

/// What the probe did.
#[derive(Debug, Clone, Default)]
pub struct ProbeOutcome {
    pub attempts: u32,
    pub open: u32,
    pub elapsed_ms: u128,
    pub candidates: Vec<Candidate>,
}

/// Parse `kind:port,kind:port`.
pub fn parse_ports(text: &str) -> Result<Vec<(DeviceKind, u16)>, String> {
    let mut out = Vec::new();
    for item in text.split(',').filter(|s| !s.is_empty()) {
        let (kind, port) = item
            .split_once(':')
            .ok_or_else(|| format!("bad probe port {item:?}: want kind:port"))?;
        let kind =
            DeviceKind::parse(kind).ok_or_else(|| format!("unknown device kind {kind:?}"))?;
        let port: u16 = port
            .parse()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| format!("bad port in {item:?}"))?;
        out.push((kind, port));
    }
    if out.is_empty() || out.len() > 16 {
        return Err("probe ports: give 1..=16 kind:port pairs".into());
    }
    Ok(out)
}

/// Run the probe. Paced: one attempt per `1/rate` seconds.
pub fn probe(opts: &ProbeOpts) -> ProbeOutcome {
    let rate = opts.rate.clamp(1, MAX_RATE);
    let gap = Duration::from_secs(1) / rate;
    let (first, last) = (opts.hosts.0.max(1), opts.hosts.1.min(254));
    let start = Instant::now();
    let mut out = ProbeOutcome::default();
    let mut next = Instant::now();
    for n in first..=last {
        let ip = opts.net.host(n);
        for (kind, port) in &opts.ports {
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
            next = Instant::now() + gap;
            out.attempts += 1;
            let addr = SocketAddr::V4(SocketAddrV4::new(ip, *port));
            let Ok(stream) = TcpStream::connect_timeout(&addr, opts.connect_timeout) else {
                continue;
            };
            out.open += 1;
            let mut txt = BTreeMap::new();
            txt.insert("kind".to_string(), kind.as_str().to_string());
            match kind.fingerprint_path() {
                Some(path) => match http_get(stream, ip, path) {
                    Some((status, marker)) => {
                        txt.insert("probe".into(), "http".into());
                        txt.insert("http_status".into(), status.to_string());
                        txt.insert("http_path".into(), path.into());
                        txt.insert("verified".into(), (status == 200 && marker).to_string());
                    }
                    None => {
                        txt.insert("probe".into(), "tcp-open".into());
                        txt.insert("verified".into(), "false".into());
                    }
                },
                None => {
                    txt.insert("probe".into(), "tcp-open".into());
                    txt.insert("verified".into(), "false".into());
                }
            }
            let id = format!(
                "{}-{}-{}",
                kind.as_str(),
                ip.to_string().replace('.', "-"),
                port
            );
            out.candidates.push(Candidate {
                id,
                instance: format!("{ip}:{port}"),
                service_type: format!("probe:{}", kind.as_str()),
                host: ip.to_string(),
                port: Some(*port),
                addrs: vec![ip.to_string()],
                txt,
                origin: Origin::Probe,
                approval: APPROVAL.to_owned(),
            });
        }
    }
    out.elapsed_ms = start.elapsed().as_millis();
    out
}

/// One `GET`; returns the status code and whether the body looks right.
fn http_get(mut stream: TcpStream, ip: Ipv4Addr, path: &str) -> Option<(u16, bool)> {
    let timeout = Some(Duration::from_millis(800));
    stream.set_read_timeout(timeout).ok()?;
    stream.set_write_timeout(timeout).ok()?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {ip}\r\nUser-Agent: openmind-discovery\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::with_capacity(2048);
    let _ = stream.take(4096).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let status_line = text.lines().next()?;
    let mut parts = status_line.split_whitespace();
    if !parts.next()?.starts_with("HTTP/1.") {
        return None;
    }
    let status: u16 = parts.next()?.parse().ok()?;
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    let marker = body.contains('{')
        && (body.contains("\"status\"")
            || body.contains("\"version\"")
            || body.contains("\"data\"")
            || body.contains("\"models\""));
    Some((status, marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_slash_24s() {
        assert_eq!(
            Net24::parse("127.0.0.0/24").unwrap().to_string(),
            "127.0.0.0/24"
        );
        assert_eq!(
            Net24::parse("192.168.1.77/24").unwrap().host(5),
            Ipv4Addr::new(192, 168, 1, 5)
        );
        assert!(Net24::parse("10.1.2.0/24").is_ok());
        assert!(Net24::parse("172.16.9.0/24").is_ok());
        assert!(Net24::parse("169.254.3.0/24").is_ok());
        assert!(Net24::parse("203.0.113.0/24").is_err());
        assert!(Net24::parse("100.64.0.0/24").is_err());
        assert!(Net24::parse("10.0.0.0/16").is_err());
        assert!(Net24::parse("10.0.0.0").is_err());
        assert!(Net24::parse("nope/24").is_err());
    }

    #[test]
    fn port_list_parsing() {
        let ports = parse_ports("exo:52415,ollama:11434").unwrap();
        assert_eq!(
            ports,
            vec![(DeviceKind::Exo, 52415), (DeviceKind::Ollama, 11434)]
        );
        assert!(parse_ports("").is_err());
        assert!(parse_ports("exo").is_err());
        assert!(parse_ports("exo:0").is_err());
        assert!(parse_ports("toaster:1").is_err());
    }

    #[test]
    fn paced_probe_of_a_closed_port_on_loopback() {
        // A port from slot 11's range that nothing listens on: 127.0.0.1 only.
        let opts = ProbeOpts {
            net: Net24::parse("127.0.0.0/24").unwrap(),
            ports: vec![(DeviceKind::Galion, 19119)],
            rate: 50,
            connect_timeout: Duration::from_millis(100),
            hosts: (1, 3),
        };
        let out = probe(&opts);
        assert_eq!(out.attempts, 3);
        assert!(out.elapsed_ms >= 40, "3 attempts at 50/s take >= 40 ms");
    }

    #[test]
    fn open_port_becomes_a_pending_probe_candidate() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"version\":\"0.0.0\"}",
            );
        });
        let opts = ProbeOpts {
            net: Net24::parse("127.0.0.0/24").unwrap(),
            ports: vec![(DeviceKind::Ollama, port)],
            rate: 100,
            connect_timeout: Duration::from_millis(200),
            hosts: (1, 1),
        };
        let out = probe(&opts);
        server.join().unwrap();
        assert_eq!(out.open, 1);
        let c = &out.candidates[0];
        assert_eq!(c.origin, Origin::Probe);
        assert_eq!(c.approval, APPROVAL);
        assert_eq!(c.txt["verified"], "true");
        assert_eq!(c.txt["http_status"], "200");
    }
}
