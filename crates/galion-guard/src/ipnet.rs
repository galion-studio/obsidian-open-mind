// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Minimal CIDR handling for the allowlist (no dependency).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Fold an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) to IPv4 so a mapped
/// form can never dodge the allowlist or produce a second ban entry.
pub fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

/// An address block, e.g. `127.0.0.0/8` or `2001:db8::/32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl Net {
    /// Parse `a.b.c.d`, `a.b.c.d/n`, `v6`, `v6/n`. Host bits may be set
    /// (fail2ban accepts `127.0.0.1/8`); they are masked off.
    pub fn parse(s: &str) -> Result<Net, String> {
        let s = s.trim();
        let (a, p) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let addr: IpAddr = a
            .trim_matches(|c| c == '[' || c == ']')
            .parse()
            .map_err(|_| format!("bad address {s:?}"))?;
        let addr = canonical(addr);
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match p {
            None => max,
            Some(p) => {
                let n: u8 = p.parse().map_err(|_| format!("bad prefix in {s:?}"))?;
                if n > max {
                    return Err(format!("prefix too long in {s:?}"));
                }
                n
            }
        };
        Ok(Net {
            addr: mask(addr, prefix),
            prefix,
        })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        if ip.is_ipv4() != self.addr.is_ipv4() {
            return false;
        }
        mask(ip, self.prefix) == self.addr
    }

    pub fn is_v4(&self) -> bool {
        self.addr.is_ipv4()
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

fn mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let bits = u32::from(v4);
            let m = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix as u32)
            };
            IpAddr::V4(Ipv4Addr::from(bits & m))
        }
        IpAddr::V6(v6) => {
            let bits = u128::from(v6);
            let m = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix as u32)
            };
            IpAddr::V6(Ipv6Addr::from(bits & m))
        }
    }
}

/// Addresses that may never be banned, whatever the config says:
/// loopback (127.0.0.0/8, ::1), unspecified, and IPv6 link-local fe80::/10
/// is NOT included (it is routable on the LAN segment and may be hostile).
pub fn is_hard_protected(ip: IpAddr) -> bool {
    let ip = canonical(ip);
    ip.is_loopback() || ip.is_unspecified()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr() {
        let n = Net::parse("127.0.0.1/8").unwrap();
        assert_eq!(n.to_string(), "127.0.0.0/8");
        assert!(n.contains("127.255.0.9".parse().unwrap()));
        assert!(n.contains("::ffff:127.0.0.2".parse().unwrap()));
        assert!(!n.contains("128.0.0.1".parse().unwrap()));
        assert!(!n.contains("::1".parse().unwrap()));
        let six = Net::parse("2001:db8::/32").unwrap();
        assert!(six.contains("2001:db8:1::5".parse().unwrap()));
        assert!(!six.contains("2001:db9::5".parse().unwrap()));
        assert!(Net::parse("1.2.3.4/33").is_err());
        assert!(Net::parse("nope").is_err());
        assert_eq!(Net::parse("0.0.0.0/0").unwrap().prefix, 0);
    }

    #[test]
    fn hard_protected() {
        assert!(is_hard_protected("127.0.0.1".parse().unwrap()));
        assert!(is_hard_protected("127.9.9.9".parse().unwrap()));
        assert!(is_hard_protected("::1".parse().unwrap()));
        assert!(is_hard_protected("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_hard_protected("0.0.0.0".parse().unwrap()));
        assert!(!is_hard_protected("203.0.113.9".parse().unwrap()));
    }
}
