// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Finding the console on the local network, with no internet and no DNS.
//!
//! A device sends one UDP broadcast, `GALION-FLEET? 1`, to port
//! [`PORT`]. A console started with `--instances-lan` answers to the
//! sender with `GALION-FLEET! 1 <http-port> <fleet-id> <name>`. The device
//! takes the console's address from the answer's source IP. Nothing secret
//! is in either packet: joining still needs the one-time code, and an
//! already-joined device only trusts an answer carrying its own fleet id.
//! (mDNS / DNS-SD needs multicast group joins and a record parser; the
//! workspace has one in openmind-discovery. A broadcast is enough here.)

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// Default UDP port.
pub const PORT: u16 = 47_474;
/// What a device asks.
pub const ASK: &str = "GALION-FLEET? 1";
/// How a console's answer starts.
pub const HERE: &str = "GALION-FLEET! 1";

/// A console that answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub addr: SocketAddr,
    pub fleet_id: String,
    pub name: String,
}

/// The answer a console sends.
pub fn answer(http_port: u16, fleet_id: &str, name: &str) -> String {
    let name: String = crate::metrics::clean(name, 40);
    let fid: String = fleet_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(32)
        .collect();
    format!("{HERE} {http_port} {fid} {name}")
}

/// Read an answer that came from `from`.
pub fn parse_answer(text: &str, from: SocketAddr) -> Option<Found> {
    let rest = text.strip_prefix(HERE)?.strip_prefix(' ')?;
    let mut it = rest.splitn(3, ' ');
    let port: u16 = it.next()?.parse().ok().filter(|p| *p > 0)?;
    let fleet_id = it.next()?.to_string();
    if fleet_id.is_empty() || !fleet_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(Found {
        addr: SocketAddr::new(from.ip(), port),
        fleet_id,
        name: crate::metrics::clean(it.next().unwrap_or(""), 40),
    })
}

/// Answer `ASK` packets forever on `sock` (run on its own thread).
/// Packets that are not exactly the question are ignored, and answers are
/// limited to 20 a second so it cannot be used to flood anyone.
pub fn respond(sock: UdpSocket, http_port: u16, fleet_id: String, name: String) {
    let reply = answer(http_port, &fleet_id, &name);
    let mut buf = [0u8; 64];
    let mut window = Instant::now();
    let mut sent = 0u32;
    loop {
        let Ok((n, from)) = sock.recv_from(&mut buf) else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if &buf[..n] != ASK.as_bytes() {
            continue;
        }
        if window.elapsed() >= Duration::from_secs(1) {
            window = Instant::now();
            sent = 0;
        }
        if sent >= 20 {
            continue;
        }
        sent += 1;
        let _ = sock.send_to(reply.as_bytes(), from);
    }
}

/// Bind the responder on `0.0.0.0:port` and start its thread.
pub fn start(
    port: u16,
    http_port: u16,
    fleet_id: String,
    name: String,
) -> std::io::Result<SocketAddr> {
    let sock = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port))?;
    let at = sock.local_addr()?;
    std::thread::Builder::new()
        .name("fleet-lan".into())
        .spawn(move || respond(sock, http_port, fleet_id, name))?;
    Ok(at)
}

/// Ask `targets` (normally the broadcast address) and collect answers for
/// `wait`. Duplicate answers are folded.
pub fn ask(targets: &[SocketAddr], wait: Duration) -> std::io::Result<Vec<Found>> {
    let sock = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    sock.set_broadcast(true)?;
    for t in targets {
        let _ = sock.send_to(ASK.as_bytes(), t);
    }
    let end = Instant::now() + wait;
    let mut out: Vec<Found> = Vec::new();
    let mut buf = [0u8; 256];
    while let Some(left) = end
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
    {
        sock.set_read_timeout(Some(left))?;
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(f) = std::str::from_utf8(&buf[..n])
                    .ok()
                    .and_then(|t| parse_answer(t, from))
                {
                    if !out.contains(&f) {
                        out.push(f);
                    }
                }
            }
            Err(_) => break,
        }
    }
    Ok(out)
}

/// Broadcast on the local network (port [`PORT`] unless given).
pub fn discover(port: u16, wait: Duration) -> std::io::Result<Vec<Found>> {
    ask(&[SocketAddr::from((Ipv4Addr::BROADCAST, port))], wait)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_round_trip() {
        let from: SocketAddr = "192.168.1.7:47474".parse().unwrap();
        let a = answer(8080, "abc123", "Studio console");
        let f = parse_answer(&a, from).unwrap();
        assert_eq!(f.addr, "192.168.1.7:8080".parse().unwrap());
        assert_eq!(f.fleet_id, "abc123");
        assert_eq!(f.name, "Studio console");
        assert!(parse_answer("GALION-FLEET! 1 0 abc x", from).is_none());
        assert!(parse_answer("GALION-FLEET! 1 80 a<b x", from).is_none());
        assert!(parse_answer("hello", from).is_none());
    }

    #[test]
    fn responder_answers_on_loopback() {
        let at = start(0, 18080, "fleet42".into(), "box".into()).unwrap();
        let target = SocketAddr::from(([127, 0, 0, 1], at.port()));
        let found = ask(&[target], Duration::from_millis(500)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].addr, "127.0.0.1:18080".parse().unwrap());
        assert_eq!(found[0].fleet_id, "fleet42");
        // Anything other than the exact question gets no answer.
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        s.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        s.send_to(b"GALION-FLEET? 2", target).unwrap();
        let mut b = [0u8; 64];
        assert!(s.recv_from(&mut b).is_err());
    }
}
