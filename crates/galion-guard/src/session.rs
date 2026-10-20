// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Multi-line (per SSH connection) bookkeeping, our own implementation of
//! the observable fail2ban `mlfid` behaviour for the sshd filter:
//!
//! * lines are grouped by connection id (`host sshd[pid]: `);
//! * a failure line without an address (rules 15, 17) is held *pending*
//!   and counted once the same connection names its address;
//! * `nofail` lines (address only) are not failures, unless the connection
//!   has a pending failure or tried more than one user name;
//! * `Accepted ...` clears pending failures (not counted), except when the
//!   connection tried several user names: then it counts as one failure;
//! * `forget` lines (disconnect/close) drop the connection state.

use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;

use crate::filter::{Host, Match};

/// Result of feeding one matched line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Count one failure for this address.
    Failure {
        ip: IpAddr,
        rule: usize,
    },
    /// Successful login from this address (used by `protect_accepted`).
    Accepted {
        ip: IpAddr,
    },
    /// A failure whose host is not an IP literal: never banned, reported.
    Unresolved {
        host: String,
        rule: usize,
    },
    Nothing,
}

#[derive(Debug, Default, Clone)]
struct Conn {
    ip: Option<IpAddr>,
    dns: Option<String>,
    users: BTreeSet<String>,
    pending: bool,
    last: i64,
}

/// Bounded connection cache.
#[derive(Debug, Default)]
pub struct Sessions {
    conns: HashMap<String, Conn>,
}

/// Max tracked connections; the oldest are dropped first.
pub const MAX_CONNS: usize = 4096;
/// Idle connections are dropped after this many seconds.
pub const CONN_TTL: i64 = 600;

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.conns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.conns.is_empty()
    }

    pub fn feed(&mut self, m: &Match, t: i64) -> Outcome {
        let mut conn = self.conns.remove(&m.mlfid).unwrap_or_default();
        conn.last = t;
        match &m.host {
            Some(Host::Ip(ip)) => conn.ip = Some(*ip),
            Some(Host::Dns(d)) => conn.dns = Some(d.clone()),
            None => {}
        }
        if let Some(u) = &m.user {
            if conn.users.len() < 64 {
                conn.users.insert(u.clone());
            }
        }
        let multi_user = conn.users.len() > 1;
        if m.gained {
            conn.pending = false;
        }
        let failure = (!m.nofail && !m.gained) || conn.pending || multi_user;
        let out = if failure {
            if let Some(ip) = conn.ip {
                conn.pending = false;
                Outcome::Failure { ip, rule: m.rule }
            } else if let Some(d) = &conn.dns {
                conn.pending = false;
                Outcome::Unresolved {
                    host: d.clone(),
                    rule: m.rule,
                }
            } else {
                conn.pending = true;
                Outcome::Nothing
            }
        } else if m.gained {
            conn.ip
                .map_or(Outcome::Nothing, |ip| Outcome::Accepted { ip })
        } else {
            Outcome::Nothing
        };
        if !m.forget {
            self.conns.insert(m.mlfid.clone(), conn);
            if self.conns.len() > MAX_CONNS {
                self.gc(t);
            }
        }
        out
    }

    /// Drop idle connections, then the oldest ones if still over the cap.
    pub fn gc(&mut self, now: i64) {
        self.conns.retain(|_, c| now - c.last <= CONN_TTL);
        if self.conns.len() > MAX_CONNS {
            let mut v: Vec<(i64, String)> = self
                .conns
                .iter()
                .map(|(k, c)| (c.last, k.clone()))
                .collect();
            v.sort();
            let drop = self.conns.len() - MAX_CONNS;
            for (_, k) in v.into_iter().take(drop) {
                self.conns.remove(&k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::SshdFilter;

    fn run(lines: &[&str]) -> Vec<Outcome> {
        let f = SshdFilter::new();
        let mut s = Sessions::new();
        lines
            .iter()
            .filter_map(|l| f.matches(l))
            .map(|m| s.feed(&m, 0))
            .filter(|o| *o != Outcome::Nothing)
            .collect()
    }
    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn plain_failure_counts_and_close_does_not() {
        let o = run(&[
            "h sshd[1]: Invalid user a from 203.0.113.1 port 1",
            "h sshd[1]: Connection closed by invalid user a 203.0.113.1 port 1 [preauth]",
        ]);
        assert_eq!(
            o,
            vec![Outcome::Failure {
                ip: ip("203.0.113.1"),
                rule: 5
            }]
        );
    }

    #[test]
    fn pending_failure_resolved_by_later_address() {
        let o = run(&[
            "h sshd[2]: Disconnecting: Too many authentication failures [preauth]",
            "h sshd[2]: Connection closed by 203.0.113.2 port 9 [preauth]",
        ]);
        assert_eq!(
            o,
            vec![Outcome::Failure {
                ip: ip("203.0.113.2"),
                rule: 19
            }]
        );
    }

    #[test]
    fn pending_before_address_learnt_from_connection_from() {
        let o = run(&[
            "h sshd[3]: Connection from 203.0.113.3 port 9 on 10.0.0.1 port 22",
            "h sshd[3]: User bob not allowed because account is locked",
        ]);
        assert_eq!(
            o,
            vec![Outcome::Failure {
                ip: ip("203.0.113.3"),
                rule: 15
            }]
        );
    }

    #[test]
    fn accepted_is_not_a_failure() {
        let o = run(&[
            "h sshd[4]: Failed publickey for root from 203.0.113.4 port 9 ssh2: ED25519 SHA256:a",
            "h sshd[4]: Accepted publickey for root from 203.0.113.4 port 9 ssh2: ED25519 SHA256:b",
        ]);
        assert_eq!(
            o,
            vec![Outcome::Accepted {
                ip: ip("203.0.113.4")
            }]
        );
    }

    #[test]
    fn several_users_on_one_connection_count() {
        let o = run(&[
            "h sshd[5]: Failed publickey for root from 203.0.113.5 port 9 ssh2: ED25519 SHA256:a",
            "h sshd[5]: Failed publickey for admin from 203.0.113.5 port 9 ssh2: ED25519 SHA256:a",
        ]);
        assert_eq!(
            o,
            vec![Outcome::Failure {
                ip: ip("203.0.113.5"),
                rule: 3
            }]
        );
    }

    #[test]
    fn separate_connections_are_separate() {
        let o = run(&[
            "h sshd[6]: Disconnecting: Too many authentication failures [preauth]",
            "h sshd[7]: Connection closed by 203.0.113.7 port 9 [preauth]",
        ]);
        assert!(o.is_empty());
    }

    #[test]
    fn dns_failure_is_unresolved() {
        let o = run(&["h sshd[8]: Invalid user a from evil.example port 1"]);
        assert_eq!(
            o,
            vec![Outcome::Unresolved {
                host: "evil.example".into(),
                rule: 5
            }]
        );
    }

    #[test]
    fn cache_is_bounded() {
        let f = SshdFilter::new();
        let mut s = Sessions::new();
        for i in 0..(MAX_CONNS + 50) {
            let l =
                format!("h sshd[{i}]: Connection from 203.0.113.9 port {i} on 10.0.0.1 port 22");
            let m = f.matches(&l).unwrap();
            s.feed(&m, i as i64 / 100);
        }
        assert!(s.len() <= MAX_CONNS);
    }
}
