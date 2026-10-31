// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Connection monitor for the dry-run network namespace.
//!
//! Samples /proc/self/net/{tcp,tcp6,udp,udp6,raw,raw6} every 25 ms (they
//! list every socket of THIS network namespace, whichever process owns it)
//! and records any socket whose peer is not loopback, plus any loopback
//! socket that touches a port outside the allow list (e.g. a DNS stub on
//! 127.0.0.53, or a live service port that has no stub). The namespace has
//! only `lo` and a default route into `lo`, so a connect to the internet
//! cannot leave; it stays visible here as SYN_SENT, which is what makes an
//! attempt provable.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The harness's DNS sink inside the namespace.
pub const DNS_PORT: u16 = 53;

pub const CANARY: (Ipv4Addr, u16) = (Ipv4Addr::new(192, 0, 2, 1), 9);

#[derive(Clone, Debug)]
pub struct Sock {
    pub proto: &'static str,
    pub local: String,
    pub lport: u16,
    pub remote: String,
    pub rport: u16,
    pub state: String,
    pub loopback_peer: bool,
    pub unspecified_peer: bool,
}

fn v4(h: &str) -> Option<Ipv4Addr> {
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(Ipv4Addr::from(n.swap_bytes()))
}

fn v6(h: &str) -> Option<Ipv6Addr> {
    if h.len() != 32 {
        return None;
    }
    let mut b = [0u8; 16];
    for w in 0..4 {
        let n = u32::from_str_radix(&h[w * 8..w * 8 + 8], 16).ok()?;
        b[w * 4..w * 4 + 4].copy_from_slice(&n.to_le_bytes());
    }
    Some(Ipv6Addr::from(b))
}

fn addr(h: &str) -> Option<(String, u16, bool, bool)> {
    let (a, p) = h.split_once(':')?;
    let port = u16::from_str_radix(p, 16).ok()?;
    if a.len() == 8 {
        let ip = v4(a)?;
        Some((ip.to_string(), port, ip.is_loopback(), ip.is_unspecified()))
    } else {
        let ip = v6(a)?;
        let lo = ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|x| x.is_loopback());
        Some((ip.to_string(), port, lo, ip.is_unspecified()))
    }
}

const TCP_STATES: [&str; 13] = [
    "?",
    "ESTABLISHED",
    "SYN_SENT",
    "SYN_RECV",
    "FIN_WAIT1",
    "FIN_WAIT2",
    "TIME_WAIT",
    "CLOSE",
    "CLOSE_WAIT",
    "LAST_ACK",
    "LISTEN",
    "CLOSING",
    "NEW_SYN_RECV",
];

pub fn sample() -> Vec<Sock> {
    let mut out = vec![];
    for (proto, file) in [
        ("tcp", "tcp"),
        ("tcp6", "tcp6"),
        ("udp", "udp"),
        ("udp6", "udp6"),
        ("raw", "raw"),
        ("raw6", "raw6"),
    ] {
        let Ok(t) = std::fs::read_to_string(format!("/proc/self/net/{file}")) else {
            continue;
        };
        for line in t.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 4 {
                continue;
            }
            let (Some(l), Some(r)) = (addr(f[1]), addr(f[2])) else {
                continue;
            };
            let st = usize::from_str_radix(f[3], 16).unwrap_or(0);
            let state = if proto.starts_with("tcp") {
                TCP_STATES.get(st).unwrap_or(&"?").to_string()
            } else {
                format!("{st:02X}")
            };
            out.push(Sock {
                proto,
                local: l.0,
                lport: l.1,
                remote: r.0,
                rport: r.1,
                state,
                loopback_peer: r.2,
                unspecified_peer: r.3,
            });
        }
    }
    out
}

#[derive(Default)]
pub struct Report {
    pub samples: u64,
    pub violations: BTreeMap<String, String>,
    pub canary_seen: bool,
    pub loopback_ports: BTreeSet<u16>,
}

pub struct Watch {
    pub stop: Arc<AtomicBool>,
    pub report: Arc<Mutex<Report>>,
}

impl Watch {
    pub fn start(allowed: BTreeSet<u16>, log: crate::util::Log) -> Watch {
        let stop = Arc::new(AtomicBool::new(false));
        let report = Arc::new(Mutex::new(Report::default()));
        let (st, rp, al) = (stop.clone(), report.clone(), allowed);
        std::thread::spawn(move || {
            while !st.load(Ordering::SeqCst) {
                let socks = sample();
                let mut r = rp.lock().unwrap();
                r.samples += 1;
                for s in socks {
                    if s.unspecified_peer {
                        // An unconnected UDP/raw socket is how a resolver or a
                        // datagram sender talks (sendto without connect): only
                        // the harness's own DNS sink (127.0.0.1:53) may exist.
                        let lo_local = s.local.starts_with("127.") || s.local == "::1";
                        if !s.proto.starts_with("tcp")
                            && !(lo_local && (s.lport == DNS_PORT || al.contains(&s.lport)))
                        {
                            let k = format!("{} socket {}:{} (unconnected datagram/raw socket: resolver or sender)", s.proto, s.local, s.lport);
                            if let std::collections::btree_map::Entry::Vacant(e) =
                                r.violations.entry(k.clone())
                            {
                                log.line(&format!("VIOLATION {k}"));
                                e.insert(crate::util::zagreb(crate::util::now_secs()));
                            }
                            continue;
                        }
                        // listening / unconnected: only loopback binds are expected
                        if s.state == "LISTEN" && !lo_local {
                            let k = format!(
                                "{} listen on non-loopback {}:{}",
                                s.proto, s.local, s.lport
                            );
                            if let std::collections::btree_map::Entry::Vacant(e) =
                                r.violations.entry(k.clone())
                            {
                                log.line(&format!("VIOLATION {k}"));
                                e.insert(crate::util::zagreb(crate::util::now_secs()));
                            }
                        }
                        continue;
                    }
                    if s.remote == CANARY.0.to_string() && s.rport == CANARY.1 {
                        r.canary_seen = true;
                        continue;
                    }
                    let ok = s.loopback_peer && (al.contains(&s.rport) || al.contains(&s.lport));
                    if s.loopback_peer {
                        r.loopback_ports.insert(if al.contains(&s.lport) {
                            s.lport
                        } else {
                            s.rport
                        });
                    }
                    if !ok {
                        let k = format!(
                            "{} {}:{} -> {}:{} {}",
                            s.proto, s.local, s.lport, s.remote, s.rport, s.state
                        );
                        if let std::collections::btree_map::Entry::Vacant(e) =
                            r.violations.entry(k.clone())
                        {
                            log.line(&format!("VIOLATION {k}"));
                            e.insert(crate::util::zagreb(crate::util::now_secs()));
                        }
                    }
                }
                drop(r);
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        });
        Watch { stop, report }
    }
}

/// Network facts of this namespace: interface names (from /proc/net/dev).
pub fn interfaces() -> Vec<String> {
    std::fs::read_to_string("/proc/self/net/dev")
        .unwrap_or_default()
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_proc_addrs() {
        assert_eq!(addr("0100007F:1F90").unwrap().0, "127.0.0.1");
        assert_eq!(addr("0100007F:1F90").unwrap().1, 8080);
        assert!(addr("0100007F:1F90").unwrap().2);
        assert_eq!(addr("01010101:01BB").unwrap().0, "1.1.1.1");
        assert!(!addr("01010101:01BB").unwrap().2);
        let l6 = addr("00000000000000000000000001000000:0050").unwrap();
        assert_eq!(l6.0, "::1");
        assert!(l6.2);
    }
}
