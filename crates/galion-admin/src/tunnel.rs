// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! `tunnel status`: read-only reachability of the admin tunnel's LOCAL ends.
//!
//! The plan's admin link is a reverse tunnel to the VPS; on the device each tunnel shows up as
//! a loopback port (ssh -L/-R forward, adb wireless port, ...). This module only opens a TCP
//! connection to those loopback ports and closes it: no bytes are sent, nothing is started,
//! no DNS is used. Non-loopback addresses and host names are refused at parse time, so a
//! config typo can never make the device probe the internet (plan goal: anonymous on the wire).

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub name: String,
    pub addr: SocketAddr,
}

/// Parse `name = 127.0.0.1:PORT` lines; `#` comments and blank lines are ignored.
pub fn parse_config(text: &str) -> Result<Vec<Endpoint>, String> {
    let mut out: Vec<Endpoint> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, addr) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected `name = 127.0.0.1:PORT`", i + 1))?;
        let name = name.trim();
        if name.is_empty()
            || name.len() > 32
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("line {}: bad tunnel name {name:?}", i + 1));
        }
        let addr: SocketAddr = addr.trim().parse().map_err(|_| {
            format!(
                "line {}: {:?} is not IP:PORT (host names are refused: no DNS)",
                i + 1,
                addr.trim()
            )
        })?;
        if !addr.ip().is_loopback() {
            return Err(format!(
                "line {}: {addr} is not loopback; tunnel status only checks local tunnel ends",
                i + 1
            ));
        }
        if addr.port() == 0 {
            return Err(format!("line {}: port 0", i + 1));
        }
        if out.iter().any(|e| e.name == name) {
            return Err(format!("line {}: duplicate name {name:?}", i + 1));
        }
        out.push(Endpoint {
            name: name.to_string(),
            addr,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Up { ms: u128 },
    Down { reason: String },
}

/// Open and immediately close one TCP connection.
pub fn probe(addr: &SocketAddr, timeout: Duration) -> State {
    let t = Instant::now();
    match TcpStream::connect_timeout(addr, timeout) {
        Ok(s) => {
            drop(s);
            State::Up {
                ms: t.elapsed().as_millis(),
            }
        }
        Err(e) => State::Down {
            reason: match e.kind() {
                std::io::ErrorKind::ConnectionRefused => "refused".into(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timeout".into(),
                _ => e.to_string(),
            },
        },
    }
}

pub fn render(e: &Endpoint, s: &State) -> String {
    match s {
        State::Up { ms } => format!("{:<16} {:<22} up    {ms} ms", e.name, e.addr),
        State::Down { reason } => format!("{:<16} {:<22} down  {reason}", e.name, e.addr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loopback_only() {
        let c = "# admin tunnel ends\nadb = 127.0.0.1:5555\n\nssh_rev=[::1]:2222 # v6\n";
        let v = parse_config(c).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "adb");
        assert_eq!(v[1].addr.port(), 2222);
        assert!(parse_config("x = 1.1.1.1:443")
            .unwrap_err()
            .contains("not loopback"));
        assert!(parse_config("x = localhost:22")
            .unwrap_err()
            .contains("no DNS"));
        assert!(parse_config("x = vps.example:22").is_err());
        assert!(parse_config("x 127.0.0.1:22").is_err());
        assert!(parse_config("bad name = 127.0.0.1:22").is_err());
        assert!(parse_config("a=127.0.0.1:1\na=127.0.0.1:2").is_err());
        assert!(parse_config("a=127.0.0.1:0").is_err());
        assert_eq!(parse_config("").unwrap(), vec![]);
    }

    #[test]
    fn probe_up_and_refused() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        assert!(matches!(
            probe(&a, Duration::from_secs(2)),
            State::Up { .. }
        ));
        drop(l);
        assert_eq!(
            probe(&a, Duration::from_secs(2)),
            State::Down {
                reason: "refused".into()
            }
        );
    }
}
